"""CPU contract tests for the H3 live preview node, against a real ComfyUI checkout.

Run with ComfyUI's Python: test-h3-preview.py --comfy PATH [--tae taeh3.safetensors].
No GPU, model weights or server are needed; --tae adds a real taeh3 decode.
"""
import argparse
import importlib.util
import io
from pathlib import Path
import struct
import sys
import threading
import types
import unittest

parser = argparse.ArgumentParser()
parser.add_argument("--comfy", required=True)
parser.add_argument("--tae")
options = parser.parse_args()
sys.argv = [sys.argv[0]]
sys.path.insert(0, options.comfy)
import comfy.cli_args
comfy.cli_args.args.cpu = True
import torch
from PIL import Image
from comfy.nested_tensor import NestedTensor
import comfy.model_patcher
import comfy.patcher_extension

package = types.ModuleType("mooshie_preview_test")
package.__path__ = [str(Path(__file__).resolve().parents[1] / "comfyui-nodes")]
sys.modules[package.__name__] = package
spec = importlib.util.spec_from_file_location(package.__name__ + ".h3_preview", Path(package.__path__[0]) / "h3_preview.py")
preview = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = preview
spec.loader.exec_module(preview)


class FakeVae:
    """Stands in for taeh3: 16x spatial upsampling, H3's 17k+5 frame grid."""

    def __init__(self):
        self.shapes = []

    def decode(self, latent):
        self.shapes.append(tuple(latent.shape))
        _, _, t, h, w = latent.shape
        frames = 1 + (t - 1) * 4 if t > 1 else 1
        return torch.rand(1, frames, h * 16, w * 16, 3)


def av_x0(t=7, h=30, w=52):
    return NestedTensor((torch.randn(2, 24, t, h, w), torch.randn(2, 32, 2, 37)))


def read_webp(data):
    image = Image.open(io.BytesIO(data))
    return image, getattr(image, "n_frames", 1)


class HelperTests(unittest.TestCase):
    def test_frame_plan_keeps_real_clip_length(self):
        indices, duration = preview.frame_plan(124, 12, 72)
        self.assertEqual(len(indices), 62)
        self.assertEqual((indices[0], indices[-1]), (0, 123))
        self.assertAlmostEqual(len(indices) * duration / 1000, 124 / 24, delta=0.05)
        # A long clip hits the frame cap but still plays for its real duration.
        indices, duration = preview.frame_plan(362, 12, 72)
        self.assertEqual(len(indices), 72)
        self.assertAlmostEqual(72 * duration / 1000, 362 / 24, delta=0.1)
        self.assertEqual(preview.frame_plan(1, 12, 72)[0], [0])

    def test_latent_shrinks_to_preview_size_and_keeps_time(self):
        video = torch.randn(1, 24, 7, 30, 52)
        small = preview.preview_latent(video, 384)
        self.assertEqual(small.shape[:3], video.shape[:3])
        self.assertLessEqual(16 * max(small.shape[-2:]), 384)
        tiny = torch.randn(1, 24, 7, 9, 16)
        self.assertIs(preview.preview_latent(tiny, 384), tiny)

    def test_video_part_takes_first_video_item_from_av_latent(self):
        x0 = av_x0()
        part = preview.video_part(x0)
        self.assertEqual(tuple(part.shape), (1, 24, 7, 30, 52))
        self.assertTrue(torch.equal(part, x0.tensors[0][:1]))

    def test_send_preview_uses_preview_event_with_webp_code(self):
        sent = []
        server = types.SimpleNamespace(client_id="app", send_sync=lambda *a: sent.append(a))
        sys.modules["server"] = types.SimpleNamespace(PromptServer=types.SimpleNamespace(instance=server))
        try:
            preview.send_preview(b"WEBPDATA")
        finally:
            del sys.modules["server"]
        event, payload, sid = sent[0]
        self.assertEqual((event, sid), (1, "app"))
        self.assertEqual(struct.unpack(">I", payload[:4])[0], 3)
        self.assertEqual(payload[4:], b"WEBPDATA")


class WrapperTests(unittest.TestCase):
    def run_steps(self, wrapper, steps, callback, after_step=None):
        def executor(noise, latent, sampler, sigmas, mask, cb, disable_pbar, seed, latent_shapes=None):
            for i in range(steps):
                x0 = av_x0()
                cb(i, x0, x0, steps)
                if after_step is not None:
                    after_step(i)  # a real step takes seconds; let the encoder run
            return "samples"
        return wrapper(executor, None, None, None, None, None, callback, False, 0, latent_shapes=None)

    def test_every_step_reaches_the_original_callback_and_previews_are_animated_webp(self):
        vae, sent, calls = FakeVae(), [], []
        done = threading.Event()

        def send(data):
            sent.append(data)
            done.set()

        wrapper = preview.make_wrapper(vae, 384, 12, 72, 60, send=send)
        result = self.run_steps(wrapper, 3, lambda *a: calls.append(a[0]),
                                after_step=lambda i: i == 0 and done.wait(10))
        self.assertEqual(result, "samples")
        self.assertEqual(calls, [0, 1, 2])
        self.assertTrue(done.is_set())
        image, frames = read_webp(sent[0])
        self.assertEqual(image.format, "WEBP")
        self.assertGreater(frames, 1)
        self.assertLessEqual(max(image.size), 384)
        self.assertEqual(vae.shapes[0][:3], (1, 24, 7))
        # Step 1 decodes unless the encoder is still finishing; step 2 never does.
        self.assertIn(len(vae.shapes), (1, 2))

    def test_final_step_is_never_decoded(self):
        vae, calls = FakeVae(), []
        wrapper = preview.make_wrapper(vae, 384, 12, 72, 60, send=lambda data: None)
        self.run_steps(wrapper, 1, lambda *a: calls.append(a[0]))
        self.assertEqual(calls, [0])
        self.assertEqual(vae.shapes, [])

    def test_busy_encoder_skips_decodes_without_blocking_sampling(self):
        vae, calls = FakeVae(), []
        release = threading.Event()
        wrapper = preview.make_wrapper(vae, 384, 12, 72, 60, send=lambda data: release.wait(10))
        self.run_steps(wrapper, 8, lambda *a: calls.append(a[0]))
        release.set()
        self.assertEqual(calls, list(range(8)))
        # One decode is being sent, at most one more waits; the rest are skipped.
        self.assertLessEqual(len(vae.shapes), 2)

    def test_decode_failure_never_breaks_sampling(self):
        class BrokenVae:
            def decode(self, latent):
                raise RuntimeError("no taeh3")
        calls = []
        wrapper = preview.make_wrapper(BrokenVae(), 384, 12, 72, 60, send=lambda data: None)
        self.assertEqual(self.run_steps(wrapper, 3, lambda *a: calls.append(a[0])), "samples")
        self.assertEqual(calls, [0, 1, 2])

    def test_node_attaches_wrapper_to_a_clone_only(self):
        import comfy.sampler_helpers
        base = comfy.model_patcher.ModelPatcher(torch.nn.Linear(1, 1), torch.device("cpu"), torch.device("cpu"))
        (patched,) = preview.MooshieH3LivePreview().apply(base, FakeVae(), 384, 12, 72, 60)
        kind = comfy.patcher_extension.WrappersMP.OUTER_SAMPLE

        def collected(model):
            # The same merge-then-collect the guider performs at sample time.
            options = comfy.model_patcher.create_model_options_clone(model.model_options)
            options.setdefault("transformer_options", {})
            comfy.sampler_helpers.prepare_model_patcher(model, {}, options)
            return comfy.patcher_extension.get_all_wrappers(kind, options, is_model_options=True)

        self.assertEqual(len(collected(patched)), 1)
        self.assertEqual(len(collected(base)), 0)


@unittest.skipUnless(options.tae, "pass --tae to decode with the real taeh3 weights")
class RealTaeTests(unittest.TestCase):
    def test_taeh3_decodes_preview_frames(self):
        import comfy.sd
        import comfy.utils
        vae = comfy.sd.VAE(sd=comfy.utils.load_torch_file(options.tae))
        vae.throw_exception_if_invalid()
        frames, duration = preview.decode_frames(vae, torch.randn(1, 24, 7, 18, 32), 256, 12, 72)
        self.assertEqual(frames.dtype.name, "uint8")
        self.assertEqual(frames.shape[1:], (144, 256, 3))
        self.assertEqual(frames.shape[0], 11)  # 22 decoded frames at 12 of 24 fps
        image, count = read_webp(preview.encode_webp(frames, duration, 60))
        self.assertEqual((image.size, count), ((256, 144), 11))


if __name__ == "__main__":
    unittest.main(verbosity=2)
