"""Verify the real bridge with a synthetic Transformers model; no weights/GPU."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("music_cover", Path(__file__).resolve().parents[1] / "src-tauri/src/commands/music_cover.py")
bridge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bridge)


class BridgeTest(unittest.TestCase):
    def run_bridge(self, result):
        calls = []

        class Model:
            @staticmethod
            def from_pretrained(path, **kwargs):
                calls.append((path, kwargs))
                return Model()

            def eval(self):
                return self

            def to(self, device):
                calls.append(device)
                return self

            def transcribe(self, source, **kwargs):
                calls.append((source, kwargs))
                return result

        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "result.json"
            with patch.dict(sys.modules, {"transformers": types.SimpleNamespace(AutoModel=Model)}), patch.object(sys, "argv", ["bridge", "model dir", "source song.wav", str(output), "cpu"]):
                bridge.main()
            return json.loads(output.read_text(encoding="utf-8")), calls

    def test_melody_only_and_unicode_warning_transfer(self):
        abc = "X:1\nT:春\nK:C\nCDEF"
        result, calls = self.run_bridge({"abc": abc, "warnings": ["Review meter"]})
        self.assertEqual(result, {"abc": abc, "warnings": ["Review meter"]})
        self.assertEqual(calls, [("model dir", {"trust_remote_code": True}), "cpu", ("source song.wav", {"melody_only": True})])

    def test_failure_and_size_limits(self):
        for result in [{}, {"abc": "score", "abc_error": "partial"}, {"abc": "é" * 70000}]:
            with self.assertRaises(RuntimeError):
                self.run_bridge(result)


if __name__ == "__main__":
    unittest.main()
