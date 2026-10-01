"""Mid-timeline keyframes: the Director's AddGuide anchoring against real ComfyUI nodes.

Run with ComfyUI's Python: test-h3-director-guides.py --comfy PATH.
Uses ComfyUI's own MiniMaxH3AddGuide with a stand-in VAE; no weights or GPU needed.
"""
import argparse
import importlib.util
from pathlib import Path
import sys
import types
import unittest

parser = argparse.ArgumentParser()
parser.add_argument("--comfy", required=True)
options = parser.parse_args()
sys.argv = [sys.argv[0]]
sys.path.insert(0, options.comfy)
import comfy.cli_args
comfy.cli_args.args.cpu = True
import torch
from comfy.nested_tensor import NestedTensor

core_spec = importlib.util.spec_from_file_location(
    "h3_core_test", Path(options.comfy) / "comfy_extras" / "nodes_minimax_h3.py")
h3 = importlib.util.module_from_spec(core_spec)
core_spec.loader.exec_module(h3)

# minimax_media registers its HTTP routes at import; give it a server that ignores them.
import server
_routes = types.SimpleNamespace(get=lambda path: (lambda fn: fn), post=lambda path: (lambda fn: fn))
server.PromptServer.instance = types.SimpleNamespace(routes=_routes)

root = Path(__file__).resolve().parents[1] / "comfyui-nodes" / "minimax_director"
package = types.ModuleType("director_test")
package.__path__ = [str(root)]
sys.modules[package.__name__] = package
for name in ("minimax_core", "minimax_plan", "minimax_media", "minimax_director"):
    spec = importlib.util.spec_from_file_location(f"{package.__name__}.{name}", root / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
director = sys.modules["director_test.minimax_director"]
plan = sys.modules["director_test.minimax_plan"]


class FakeVae:
    def __init__(self):
        self.encoded = []

    def encode(self, frames):
        self.encoded.append(tuple(frames.shape))
        return torch.zeros(1, 24, 1, frames.shape[1] // 16, frames.shape[2] // 16)


def av_latent(length=124, height=128, width=128):
    _, latent_t, audio_t = h3.temporal_shape(length)
    video = torch.zeros(1, 24, latent_t, height // 16, width // 16)
    return {"samples": NestedTensor((video, torch.zeros(1, 32, 2, audio_t)))}


def event(role, start, frames=1):
    return {"role": role, "rel_start_f": start, "tensor": torch.rand(frames, 128, 128, 3)}


def base_conditioning():
    # What MiniMaxH3ImageToVideo leaves behind for a first-frame clip.
    return [[torch.zeros(1, 4, 8), {"minimax_keyframes": [{"resolved_frame_index": 0}]}]]


def keyframe_indices(conditioning):
    return [k["resolved_frame_index"] for k in conditioning[0][1]["minimax_keyframes"]]


class GuideFrameIndexTests(unittest.TestCase):
    def test_maps_timeline_frames_to_the_24_fps_grid(self):
        self.assertEqual(director.guide_frame_index(60, 24, 124), 60)
        self.assertEqual(director.guide_frame_index(60, 30, 124), 48)

    def test_never_lands_on_the_first_or_last_frame(self):
        self.assertEqual(director.guide_frame_index(0, 24, 124), 1)
        self.assertEqual(director.guide_frame_index(500, 24, 124), 122)


class AnchorTests(unittest.TestCase):
    def run_anchor(self, events, mm=h3, fit=lambda t: t):
        vae = FakeVae()
        conditioning, anchored = director.anchor_middle_frames(
            mm, base_conditioning(), av_latent(), vae, events, 24.0, 124, fit)
        return conditioning, anchored, vae

    def test_middle_images_follow_the_existing_keyframes_in_time_order(self):
        events = [event(plan.ROLE_FIRST, 0), event(plan.ROLE_MIDDLE, 90),
                  event(plan.ROLE_MIDDLE, 30), event(plan.ROLE_LAST, 123)]
        conditioning, anchored, vae = self.run_anchor(events)
        self.assertEqual(anchored, [30, 90])
        self.assertEqual(keyframe_indices(conditioning), [0, 30, 90])
        self.assertEqual(len(vae.encoded), 2)
        for keyframe in conditioning[0][1]["minimax_keyframes"][1:]:
            self.assertEqual(tuple(keyframe["latent"].shape), (1, 24, 1, 8, 8))

    def test_input_conditioning_is_not_mutated(self):
        original = base_conditioning()
        director.anchor_middle_frames(h3, original, av_latent(), FakeVae(),
                                      [event(plan.ROLE_MIDDLE, 30)], 24.0, 124, lambda t: t)
        self.assertEqual(keyframe_indices(original), [0])

    def test_two_images_on_one_frame_keep_the_first(self):
        events = [event(plan.ROLE_MIDDLE, 30), event(plan.ROLE_MIDDLE, 30.2)]
        conditioning, anchored, _ = self.run_anchor(events)
        self.assertEqual(anchored, [30])
        self.assertEqual(keyframe_indices(conditioning), [0, 30])

    def test_a_clip_segment_contributes_its_first_frame(self):
        fitted = []
        conditioning, anchored, vae = self.run_anchor(
            [event(plan.ROLE_MIDDLE, 40, frames=12)], fit=lambda t: fitted.append(t.shape[0]) or t)
        self.assertEqual(fitted, [1])
        self.assertEqual(anchored, [40])

    def test_edges_only_and_missing_addguide_leave_conditioning_alone(self):
        conditioning, anchored, vae = self.run_anchor([event(plan.ROLE_FIRST, 0)])
        self.assertEqual((anchored, keyframe_indices(conditioning), vae.encoded), ([], [0], []))
        old_comfy = types.SimpleNamespace(MiniMaxH3ImageToVideo=h3.MiniMaxH3ImageToVideo)
        conditioning, anchored, vae = self.run_anchor([event(plan.ROLE_MIDDLE, 30)], mm=old_comfy)
        self.assertEqual((anchored, keyframe_indices(conditioning), vae.encoded), ([], [0], []))


if __name__ == "__main__":
    unittest.main(verbosity=2)
