"""MooshieSaveImage keeps a model-made alpha channel and stays opaque otherwise.

Ming-Image and Qwen-Image 2.1 decode RGBA; every other model decodes RGB. Only
numpy and Pillow are needed: the class is extracted from mooshie_nodes.py
without importing ComfyUI. Run with `python scripts/comfyui-compat/test_save_image_alpha.py`.
"""
import ast
import io
from pathlib import Path
import struct
import unittest

import numpy as np
from PIL import Image

NODES = Path(__file__).resolve().parents[2] / "src-tauri" / "src" / "comfyui" / "mooshie_nodes.py"


def load_save_image():
    source = ast.parse(NODES.read_text(encoding="utf-8"))
    cls = next(n for n in source.body if isinstance(n, ast.ClassDef) and n.name == "MooshieSaveImage")
    namespace = {"np": np, "io": io, "struct": struct}
    exec(compile(ast.Module(body=[cls], type_ignores=[]), str(NODES), "exec"), namespace)
    return namespace["MooshieSaveImage"]


class _Tensor:
    """The two calls `_encode_16bit` makes on a torch tensor."""

    def __init__(self, array):
        self.array = array

    def cpu(self):
        return self

    def numpy(self):
        return self.array


class SaveImageAlphaTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.node = load_save_image()
        rng = np.random.default_rng(0)
        cls.rgba = rng.random((5, 7, 4), dtype=np.float32)
        cls.rgba[0, 0, 3] = 0.0
        cls.rgb = np.ascontiguousarray(cls.rgba[:, :, :3])

    def test_png_8bit(self):
        alpha = np.array(Image.open(io.BytesIO(self.node._encode_png_8bit(self.rgba))))[:, :, 3]
        self.assertEqual(alpha[0, 0], 0)
        np.testing.assert_allclose(alpha, (255.0 * self.rgba[:, :, 3]).astype(np.uint8), atol=1)
        opaque = np.array(Image.open(io.BytesIO(self.node._encode_png_8bit(self.rgb))))
        self.assertTrue((opaque[:, :, 3] == 255).all())

    def test_raw_payloads(self):
        for depth, dtype, full in (("8bit", np.uint8, 255), ("16bit", np.uint16, 0xFFFF)):
            for image, see_through in ((self.rgba, True), (self.rgb, False)):
                _, payload = self.node._encode_raw(image, depth)
                width, height, channels, _, _ = struct.unpack(">HHBBH", payload[:8])
                self.assertEqual(channels, 4)
                alpha = np.frombuffer(payload[8:], dtype=dtype).reshape(height, width, 4)[:, :, 3]
                if see_through:
                    self.assertEqual(alpha[0, 0], 0, depth)
                else:
                    self.assertTrue((alpha == full).all(), depth)

    def test_png_16bit_fallback(self):
        # Without OpenCV the node writes the PNG itself; byte 25 is the IHDR colour type.
        for image, colour_type in ((self.rgba, 6), (self.rgb, 2)):
            png = self.node._encode_16bit(_Tensor(image))
            self.assertEqual(png[25], colour_type)
            decoded = Image.open(io.BytesIO(png))
            decoded.load()
            self.assertEqual(decoded.size, (7, 5))


if __name__ == "__main__":
    unittest.main()
