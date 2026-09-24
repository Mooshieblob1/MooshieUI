"""CPU contract tests with real ComfyUI NestedTensor and safetensors.

Run with ComfyUI's Python: test-h3-drafts.py --comfy PATH [--checkpoint FILE].
All output uses a temporary directory. No GPU generation or server mutation.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

parser = argparse.ArgumentParser()
parser.add_argument("--comfy", required=True)
parser.add_argument("--checkpoint")
options = parser.parse_args()
sys.argv = [sys.argv[0]]
sys.path.insert(0, options.comfy)
import comfy.cli_args
comfy.cli_args.args.cpu = True
import torch
from comfy.nested_tensor import NestedTensor
import folder_paths

package = types.ModuleType("mooshie_draft_test")
package.__path__ = [str(Path(__file__).resolve().parents[1] / "comfyui-nodes")]
sys.modules[package.__name__] = package
spec = importlib.util.spec_from_file_location(package.__name__ + ".h3_drafts", Path(package.__path__[0]) / "h3_drafts.py")
drafts = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = drafts
spec.loader.exec_module(drafts)


class DraftTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.folder_patch = patch.object(folder_paths, "get_output_directory", return_value=str(self.root))
        self.folder_patch.start()
        self.id = "a" * 32
        self.video = torch.randn(1, 24, 2, 4, 4)
        self.audio = torch.randn(1, 32, 2, 8)
        self.latent = {"samples": NestedTensor([self.video, self.audio])}
        self.embedding = torch.randn(1, 4, 8)
        self.positive = [[self.embedding, {"minimax_refs": [
            {"kind": "video", "latent": self.video, "latent_h": 4, "latent_w": 4, "latent_t": 2},
            {"kind": "audio", "latent": self.audio, "audio_latent": self.audio}],
            "minimax_keyframes": [{"latent": self.video}], "pooled_output": self.embedding}]]

    def tearDown(self):
        self.folder_patch.stop()
        self.temp.cleanup()

    def save(self):
        return drafts.MooshieH3SaveDraft().save(self.latent, self.positive,
            json.dumps({"video_frame_count": 22, "positive_prompt": "synthetic 雪"}), self.id,
            {"waveform": torch.randn(1, 2, 128), "sample_rate": 48000})

    def test_safe_round_trip_and_storage_dimensions(self):
        self.save()
        meta = drafts.read_manifest(self.id)
        self.assertEqual((meta["width"], meta["height"], meta["frames"]), (64, 64, 22))
        latent, positive, audio = drafts.MooshieH3LoadDraft().load(self.id)
        v, a = drafts.video_audio(latent)
        self.assertTrue(torch.equal(v, self.video))
        self.assertTrue(torch.equal(a, self.audio))
        self.assertTrue(torch.equal(positive[0][0], self.embedding))
        self.assertEqual(audio["waveform"].shape, (1, 2, 128))
        self.assertEqual(audio["sample_rate"], 48000)

    def test_references_resize_but_audio_and_text_stay_exact(self):
        out = drafts.resize_conditioning(self.positive)
        self.assertIs(out[0][0], self.embedding)
        refs = out[0][1]["minimax_refs"]
        self.assertEqual(refs[0]["latent"].shape, (1, 24, 2, 8, 8))
        self.assertEqual((refs[0]["latent_h"], refs[0]["latent_w"], refs[0]["latent_t"]), (8, 8, 2))
        self.assertIs(refs[1]["latent"], self.audio)
        self.assertIs(refs[1]["audio_latent"], self.audio)
        self.assertEqual(self.positive[0][1]["minimax_refs"][0]["latent_h"], 4)
        self.assertEqual(out[0][1]["minimax_keyframes"][0]["latent"].shape[-2:], (8, 8))

    def test_traversal_invalid_ids_and_unknown_objects_rejected(self):
        for value in ["../draft", "", "a" * 31, "A" * 32, "/" + self.id, "C:" + self.id]:
            with self.assertRaises(ValueError): drafts.draft_path(value)
        for value in [object(), float("nan"), {1: "bad"}]:
            with self.assertRaises(ValueError): drafts.encode_tree(value, {})
        with self.assertRaises(ValueError): drafts.decode_tree({"kind": "pickle", "value": "x"}, {})

    def test_atomic_failure_and_duplicate_preserve_original(self):
        with patch("safetensors.torch.save_file", side_effect=OSError("synthetic disk failure")):
            with self.assertRaises(OSError): self.save()
        self.assertEqual(list(drafts.draft_root().iterdir()), [])
        self.save()
        original = (drafts.draft_path(self.id) / "manifest.json").read_bytes()
        with self.assertRaises(ValueError): self.save()
        self.assertEqual((drafts.draft_path(self.id) / "manifest.json").read_bytes(), original)

    def test_invalid_or_missing_manifest_rejected(self):
        with self.assertRaises(FileNotFoundError): drafts.read_manifest(self.id)
        self.save()
        path = drafts.draft_path(self.id) / "manifest.json"
        meta = json.loads(path.read_text("utf-8"))
        meta["width"] = -1
        path.write_text(json.dumps(meta), "utf-8")
        with self.assertRaises(ValueError): drafts.read_manifest(self.id)

    def test_audio_restored_even_if_sampler_changed_it(self):
        refined = {"samples": NestedTensor([self.video * 2, self.audio * 3])}
        out, = drafts.MooshieH3RestoreAudio().restore(refined, self.latent)
        v, a = drafts.video_audio(out)
        self.assertTrue(torch.equal(v, self.video * 2))
        self.assertTrue(torch.equal(a, self.audio))

    def test_cleanup_only_removes_old_incomplete_drafts(self):
        self.save()
        first = self.id
        self.id = "b" * 32
        self.save()
        drafts.mark_complete(self.id)
        old = drafts.time.time() - 90000
        for draft_id in (first, self.id):
            path = drafts.draft_path(draft_id)
            manifest = path / "manifest.json"
            meta = json.loads(manifest.read_text("utf-8"))
            meta["created_at"] = old
            manifest.write_text(json.dumps(meta), "utf-8")
            os.utime(path, (old, old))
        with patch.object(drafts, "_last_cleanup", 0): drafts.cleanup_incomplete()
        self.assertFalse(drafts.draft_path(first).exists())
        self.assertTrue(drafts.draft_path(self.id).exists())

    @unittest.skipUnless(options.checkpoint, "Pass --checkpoint for real learned upscaler inference")
    def test_pinned_upscaler_runs_on_cpu_and_masks_audio(self):
        with patch.object(drafts, "model_path", return_value=Path(options.checkpoint)):
            latent, conditioning, sigmas = drafts.MooshieH3UpscaleDraft().upscale(self.latent, self.positive, 8, 0.35)
        v, a = drafts.video_audio(latent)
        self.assertEqual(v.shape, (1, 24, 2, 8, 8))
        self.assertTrue(torch.isfinite(v).all())
        self.assertTrue(torch.equal(a, self.audio))
        masks = latent["noise_mask"].unbind()
        self.assertTrue(torch.all(masks[0] == 1))
        self.assertTrue(torch.all(masks[1] == 0))
        # Exercise the native sampler's mask preparation, including channels.
        from comfy.sampler_helpers import prepare_mask
        video_mask = prepare_mask(masks[0], v.shape, torch.device("cpu"))
        audio_mask = prepare_mask(masks[1], a.shape, torch.device("cpu"))
        self.assertEqual(video_mask.shape, v.shape)
        self.assertEqual(audio_mask.shape, a.shape)
        self.assertTrue(torch.all(audio_mask == 0))
        self.assertEqual(len(sigmas), 9)
        self.assertAlmostEqual(sigmas[0].item(), 0.35, places=6)
        self.assertEqual(sigmas[-1].item(), 0)
        self.assertEqual(conditioning[0][1]["minimax_refs"][0]["latent_h"], 8)


class RouteTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        from aiohttp import web
        from aiohttp.test_utils import TestClient, TestServer
        DraftTests.setUp(self)
        self.pending = []
        routes = web.RouteTableDef()
        server = types.SimpleNamespace(routes=routes, prompt_queue=types.SimpleNamespace(get_current_queue=lambda: ([], self.pending)))
        fake = types.ModuleType("server")
        fake.PromptServer = types.SimpleNamespace(instance=server)
        with patch.dict(sys.modules, {"server": fake}): drafts.register_routes()
        app = web.Application()
        app.add_routes(routes)
        self.client = TestClient(TestServer(app))
        await self.client.start_server()

    async def asyncTearDown(self):
        await self.client.close()
        DraftTests.tearDown(self)

    async def test_status_busy_delete_missing_and_corrupt_data(self):
        DraftTests.save(self)
        url = "/mooshie/h3/drafts/" + self.id
        response = await self.client.get(url)
        self.assertEqual(response.status, 200)
        meta = await response.json()
        self.assertNotIn("tree", meta)
        self.assertGreater(meta["bytes"], 0)
        self.pending = [(0, "synthetic-job", {"1": {"class_type": "MooshieH3LoadDraft", "inputs": {"draft_id": self.id}}})]
        self.assertEqual((await self.client.delete(url)).status, 409)
        self.assertTrue(drafts.draft_path(self.id).exists())
        self.pending = []
        (drafts.draft_path(self.id) / "manifest.json").write_text("bad", "utf-8")
        self.assertEqual((await self.client.get(url)).status, 400)
        self.assertEqual((await self.client.delete(url)).status, 200)
        self.assertEqual((await self.client.get(url)).status, 404)
        self.assertEqual((await self.client.delete(url)).status, 200)
        self.assertEqual((await self.client.get("/mooshie/h3/drafts/not-an-id")).status, 400)


if __name__ == "__main__": unittest.main(verbosity=2)
