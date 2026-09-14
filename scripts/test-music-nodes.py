"""Test real music node methods with synthetic CLIP/PyAV boundaries, no model downloads."""
import ast
import json
from pathlib import Path
import re
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

source = Path(__file__).resolve().parents[1] / "src-tauri/src/comfyui/mooshie_nodes.py"
tree = ast.parse(source.read_text(encoding="utf-8"))
selected = ast.Module(body=[node for node in tree.body if isinstance(node, ast.ClassDef) and node.name in {"MooshieYuE2Plan", "MooshieYuE2Music", "MooshieMusicLoadAudio"}], type_ignores=[])
scope = {"json": json, "re": re}
exec(compile(selected, str(source), "exec"), scope)

class Clip:
    def __init__(self, ids=None, flag=False):
        self.ids = ids if ids is not None else [1, 2]
        self.flag = flag
    def tokenize(self, style, **kwargs):
        self.options = kwargs
        return {"tokens": []}
    def generate(self, tokens, **kwargs):
        self.generation = kwargs
        return self.ids
    def decode(self, ids):
        return "X:1\nK:C\nCDEF"
    def encode_from_tokens_scheduled(self, tokens):
        return [[None, {"yue2_frames": 300, "yue2_truncated": self.flag}]]

class Buffer:
    def __init__(self, samples): self.samples = samples
    def unsqueeze(self, axis): return (1, 2, self.samples)

class MusicNodes(unittest.TestCase):
    def test_planner_end_token_contract(self):
        plan = scope["MooshieYuE2Plan"]()
        for count, budget, truncated in [(2, 4, False), (3, 4, False), (4, 4, True)]:
            clip = Clip(list(range(count)))
            abc, receipt = plan.plan(clip, "jazz", "lyrics", 42, "melody", budget)
            self.assertTrue(abc.startswith("X:1"))
            self.assertEqual(json.loads(receipt)["abc_truncated"], truncated)
            self.assertEqual(clip.options["cot"], "melody")
            self.assertEqual(clip.generation["max_length"], budget)
        with self.assertRaises(RuntimeError):
            plan.plan(Clip("unexpected"), "", "", 42, "full", 4)

    def test_semantic_guidance_and_unknown_receipts(self):
        node = scope["MooshieYuE2Music"]()
        with patch.dict(sys.modules, {"comfy.text_encoders.yue2": types.SimpleNamespace(FRAMES_PER_SECOND=25)}):
            for abc, cfg, expected, flag in [("score", -1, 1, True), ("", -1, 1.01, False), ("score", 1.4, 1.4, "not a flag")]:
                clip = Clip(flag=flag)
                _, seconds, receipt = node.generate(clip,"style","lyrics",abc,42,"full",30,1,.95,100,1.2,cfg)
                receipt = json.loads(receipt)
                self.assertEqual(seconds, 12)
                self.assertEqual(clip.options["max_tokens"], 750)
                self.assertEqual(clip.options["cfg_scale"], expected)
                self.assertEqual(receipt["semantic_truncated"], flag if isinstance(flag, bool) else None)
                self.assertEqual(clip.options["cot"], "full" if abc else "off")
            with self.assertRaises(ValueError):
                node.generate(Clip(),"","","",1,"full",30,1,.95,100,1.2,-.5)

    def test_source_scope_incremental_limit_and_cleanup(self):
        node = scope["MooshieMusicLoadAudio"]()
        sizes = [44100, 44100]
        class Container:
            streams = types.SimpleNamespace(audio=[types.SimpleNamespace(index=0)])
            def __enter__(self): return self
            def __exit__(self, *args): pass
            def decode(self, **kwargs):
                for size in sizes:
                    yield types.SimpleNamespace(samples=size, to_ndarray=lambda n=size: Buffer(n))
        av = types.SimpleNamespace(open=lambda _: Container(), AudioResampler=lambda **_: types.SimpleNamespace(resample=lambda frame: [] if frame is None else [frame]))
        modules = {"av":av,"comfy.model_management":types.SimpleNamespace(throw_exception_if_processing_interrupted=lambda:None)}
        with tempfile.TemporaryDirectory(prefix="mooshie-node-test-") as directory, patch.dict(sys.modules, modules):
            scope["folder_paths"] = types.SimpleNamespace(get_input_directory=lambda: directory)
            scope["torch"] = types.SimpleNamespace(from_numpy=lambda a:a, cat=lambda buffers,dim:Buffer(sum(b.samples for b in buffers)))
            root = Path(directory)
            name = "mooshie_cover_12345678-1234-1234-1234-123456789abc.wav"
            source = root/name
            other = root/"personal.wav"; other.write_bytes(b"keep")
            for invalid in ["../personal.wav", "personal.wav", "mooshie_cover_wrong.wav", "C:\\private.wav"]:
                with self.assertRaises(ValueError): node.load(invalid)
            source.write_bytes(b"audio")
            result = node.load(name)[0]
            self.assertEqual(result["waveform"], (1,2,88200))
            self.assertFalse(source.exists())
            sizes[:] = [360*44100, 1]
            source.write_bytes(b"audio")
            with self.assertRaisesRegex(ValueError,"360 seconds"): node.load(name)
            self.assertFalse(source.exists())
            self.assertEqual(other.read_bytes(),b"keep")

if __name__ == "__main__": unittest.main()
