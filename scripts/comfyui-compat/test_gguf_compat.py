"""Unit tests for comfyui-nodes/gguf_compat.py.

Simulates city96/ComfyUI-GGUF's loader, nodes, and tools.convert modules in
memory without needing a full ComfyUI checkout. Uses real PyTorch when
available, or a tiny NumPy-backed shim when run with plain Python + NumPy.
Run with `python scripts/comfyui-compat/test_gguf_compat.py`.
"""
import importlib.util
from pathlib import Path
import sys
import types
import unittest

import numpy as np

GGUF_COMPAT_PATH = (
    Path(__file__).resolve().parents[2] / "comfyui-nodes" / "gguf_compat.py"
)


def _ensure_torch():
    try:
        import torch  # noqa: F401
        return
    except ImportError:
        pass

    torch_mod = types.ModuleType("torch")
    torch_mod.float32 = np.float32
    torch_mod.float16 = np.float16
    torch_mod.uint8 = np.uint8
    torch_mod.int8 = np.int8

    class FakeSize(tuple):
        pass

    class FakeTensor:
        def __new__(cls, arr, *_args, **_kwargs):
            return super().__new__(cls)

        def __init__(self, arr):
            raw = arr._arr if isinstance(arr, FakeTensor) else arr
            self._arr = np.asarray(raw)

        @property
        def shape(self):
            return FakeSize(self._arr.shape)

        @property
        def dtype(self):
            return self._arr.dtype

        def size(self):
            return self.shape

        def numel(self):
            return int(self._arr.size)

        def as_subclass(self, cls):
            if cls is FakeTensor:
                return FakeTensor(self._arr)
            obj = object.__new__(cls)
            obj._arr = self._arr
            return obj

        def to(self, dtype):
            return FakeTensor(self._arr.astype(dtype))

        def reshape(self, *shape):
            if len(shape) == 1 and isinstance(shape[0], (tuple, list)):
                shape = tuple(shape[0])
            return FakeTensor(self._arr.reshape(shape))

        def expand(self, *shape):
            if len(shape) == 1 and isinstance(shape[0], (tuple, list)):
                shape = tuple(shape[0])
            return FakeTensor(np.broadcast_to(self._arr, shape).copy())

        def contiguous(self):
            return FakeTensor(np.ascontiguousarray(self._arr))

        def view(self, *args):
            if len(args) == 1 and args[0] in (np.uint8, np.int8, np.float16, np.float32):
                return FakeTensor(self._arr.view(args[0]))
            return self.reshape(*args)

        def numpy(self):
            return self._arr

        def __eq__(self, other):
            other_arr = other._arr if isinstance(other, FakeTensor) else other
            return np.array_equal(self._arr, other_arr)

    def from_numpy(arr):
        return FakeTensor(arr)

    def cat(tensors, dim=0):
        return FakeTensor(np.concatenate([t._arr for t in tensors], axis=dim))

    torch_mod.Tensor = FakeTensor
    torch_mod.Size = FakeSize
    torch_mod.from_numpy = from_numpy
    torch_mod.cat = cat
    sys.modules["torch"] = torch_mod


_ensure_torch()
import torch  # noqa: E402


def _make_fake_gguf_package(pkg_name="_fake_comfyui_gguf"):
    """Build an in-memory ComfyUI-GGUF package matching city96's layout."""
    for name in list(sys.modules):
        if name == pkg_name or name.startswith(pkg_name + "."):
            del sys.modules[name]

    class FakeQType:
        def __init__(self, name, val):
            self.name = name
            self.val = val

        def __repr__(self):
            return self.name

    class FakeGGMLQuantizationType:
        F32 = FakeQType("F32", 0)
        F16 = FakeQType("F16", 1)
        Q8_0 = FakeQType("Q8_0", 8)
        Q2_K = FakeQType("Q2_K", 10)
        I8 = FakeQType("I8", 24)
        BF16 = FakeQType("BF16", 30)

    class FakeTensorEntry:
        def __init__(self, name, data, tensor_type, shape=None):
            self.name = name
            self.data = data
            self.tensor_type = tensor_type
            # GGUF stores shape in reversed order relative to PyTorch dims.
            self.shape = shape if shape is not None else tuple(reversed(data.shape))

    class FakeReaderRegistry:
        files = {}

    class FakeGGUFReader:
        def __init__(self, path):
            spec = FakeReaderRegistry.files[path]
            self.fields = spec.get("fields", {})
            self.tensors = spec["tensors"]

    gguf_mod = types.ModuleType("gguf")
    gguf_mod.GGMLQuantizationType = FakeGGMLQuantizationType
    gguf_mod.GGUFReader = FakeGGUFReader

    pkg = types.ModuleType(pkg_name)
    pkg.__path__ = []
    tools_pkg = types.ModuleType(f"{pkg_name}.tools")
    tools_pkg.__path__ = []
    convert_mod = types.ModuleType(f"{pkg_name}.tools.convert")
    convert_mod.__package__ = f"{pkg_name}.tools"

    class ModelTemplate:
        arch = "invalid"
        keys_detect = []
        keys_banned = []

    class ModelFlux(ModelTemplate):
        arch = "flux"
        keys_detect = [
            ("transformer_blocks.0.attn.norm_added_k.weight",),
            ("double_blocks.0.img_attn.proj.weight",),
        ]
        keys_banned = ["transformer_blocks.0.attn.norm_added_k.weight"]

    class ModelSDXL(ModelTemplate):
        arch = "sdxl"
        keys_detect = [("label_emb.0.0.weight",)]

    def is_model_arch(model, state_dict_keys):
        matched = False
        invalid = False
        for match_list in model.keys_detect:
            if all(key in state_dict_keys for key in match_list):
                matched = True
                invalid = any(key in state_dict_keys for key in model.keys_banned)
                break
        assert not invalid, "Model architecture not allowed"
        return matched

    def detect_arch(state_dict_keys):
        for arch in convert_mod.arch_list:
            if is_model_arch(arch, state_dict_keys):
                return arch()
        raise AssertionError("Unknown model architecture!")

    convert_mod.ModelTemplate = ModelTemplate
    convert_mod.arch_list = [ModelFlux, ModelSDXL]
    convert_mod.is_model_arch = is_model_arch
    convert_mod.detect_arch = detect_arch

    loader_mod = types.ModuleType(f"{pkg_name}.loader")
    loader_mod.__package__ = pkg_name
    loader_mod.gguf = gguf_mod
    loader_mod.IMG_ARCH_LIST = {
        "flux",
        "sd1",
        "sdxl",
        "sd3",
        "aura",
        "hidream",
        "cosmos",
        "ltxv",
        "hyvid",
        "wan",
        "lumina2",
        "qwen_image",
    }
    loader_mod.TXT_ARCH_LIST = {
        "t5",
        "t5encoder",
        "llama",
        "qwen2vl",
        "qwen3",
        "qwen3vl",
        "gemma3",
    }
    loader_mod.VIS_TYPE_LIST = {"clip-vision", "mmproj"}
    loader_mod.remap_calls = 0

    class GGMLTensor(torch.Tensor):
        def __new__(cls, data, tensor_type=None, tensor_shape=None, **kwargs):
            return super().__new__(cls, data)

        def __init__(self, data, tensor_type=None, tensor_shape=None, **kwargs):
            if hasattr(torch.Tensor, "__init__") and hasattr(data, "_arr"):
                super().__init__(data)
            self.tensor_type = tensor_type
            self.tensor_shape = tensor_shape
            self.is_largest_weight = False

        @property
        def shape(self):
            if getattr(self, "tensor_shape", None) is not None:
                return self.tensor_shape
            return super().shape

    def is_quantized(tensor):
        return getattr(tensor, "tensor_type", None) not in {
            None,
            FakeGGMLQuantizationType.F32,
            FakeGGMLQuantizationType.F16,
            FakeGGMLQuantizationType.BF16,
        }

    def dequantize_tensor(tensor, dtype=None):
        if hasattr(tensor, "_arr"):
            return torch.from_numpy(tensor._arr.astype(dtype or torch.float32))
        return torch.tensor(tensor.detach().cpu().numpy(), dtype=dtype or torch.float32)

    def get_orig_shape(_reader, _tensor_name):
        return None

    def get_gguf_metadata(reader):
        return dict(reader.fields)

    def gguf_sd_loader(path, handle_prefix="model.diffusion_model.", is_text_model=False):
        reader = gguf_mod.GGUFReader(path)
        has_prefix = False
        if handle_prefix is not None:
            prefix_len = len(handle_prefix)
            tensor_names = {t.name for t in reader.tensors}
            has_prefix = any(s.startswith(handle_prefix) for s in tensor_names)

        tensors = []
        for tensor in reader.tensors:
            sd_key = tensor.name
            if has_prefix:
                if not sd_key.startswith(handle_prefix):
                    continue
                sd_key = sd_key[prefix_len:]
            tensors.append((sd_key, tensor))

        arch_str = reader.fields.get("general.architecture")
        type_str = reader.fields.get("general.type")
        if arch_str in [None, "pig", "cow"]:
            if is_text_model:
                raise ValueError(
                    f"This gguf file is incompatible with llama.cpp!\n({path})"
                )
            try:
                arch_str = convert_mod.detect_arch({val[0] for val in tensors}).arch
            except Exception as e:
                raise ValueError(f"This model is not currently supported - ({e})")
        elif arch_str not in loader_mod.TXT_ARCH_LIST and is_text_model:
            if type_str not in loader_mod.VIS_TYPE_LIST:
                raise ValueError(f"Unexpected text model architecture type in GGUF file: {arch_str!r}")
        elif arch_str not in loader_mod.IMG_ARCH_LIST and not is_text_model:
            raise ValueError(f"Unexpected architecture type in GGUF file: {arch_str!r}")

        state_dict = {}
        for sd_key, tensor in tensors:
            shape = torch.Size(tuple(int(v) for v in reversed(tensor.shape)))
            t = torch.from_numpy(tensor.data)
            if tensor.tensor_type in {FakeGGMLQuantizationType.F32, FakeGGMLQuantizationType.F16}:
                t = t.view(*shape)
            state_dict[sd_key] = GGMLTensor(t, tensor_type=tensor.tensor_type, tensor_shape=shape)
        return state_dict, {"arch_str": arch_str, "metadata": get_gguf_metadata(reader)}

    def gguf_clip_loader(path):
        sd, extra = loader_mod.gguf_sd_loader(path, is_text_model=True)
        arch = extra.get("arch_str")
        if arch in {"t5", "t5encoder", "llama", "qwen2vl", "qwen3", "qwen3vl", "gemma3"}:
            loader_mod.remap_calls += 1
        return sd

    loader_mod.GGMLTensor = GGMLTensor
    loader_mod.is_quantized = is_quantized
    loader_mod.dequantize_tensor = dequantize_tensor
    loader_mod.get_orig_shape = get_orig_shape
    loader_mod.get_gguf_metadata = get_gguf_metadata
    loader_mod.gguf_sd_loader = gguf_sd_loader
    loader_mod.gguf_clip_loader = gguf_clip_loader

    nodes_mod = types.ModuleType(f"{pkg_name}.nodes")
    nodes_mod.__package__ = pkg_name
    nodes_mod.gguf_sd_loader = gguf_sd_loader
    nodes_mod.gguf_clip_loader = gguf_clip_loader

    sys.modules[pkg_name] = pkg
    sys.modules[f"{pkg_name}.tools"] = tools_pkg
    sys.modules[f"{pkg_name}.tools.convert"] = convert_mod
    sys.modules[f"{pkg_name}.loader"] = loader_mod
    sys.modules[f"{pkg_name}.nodes"] = nodes_mod

    return pkg_name, loader_mod, nodes_mod, convert_mod, FakeReaderRegistry, FakeTensorEntry, FakeGGMLQuantizationType


def _load_gguf_compat():
    spec = importlib.util.spec_from_file_location("test_gguf_compat_mod", GGUF_COMPAT_PATH)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


class GgufCompatTests(unittest.TestCase):
    def setUp(self):
        (
            self.pkg_name,
            self.loader,
            self.nodes,
            self.convert,
            self.registry,
            self.TensorEntry,
            self.QType,
        ) = _make_fake_gguf_package()
        self.compat = _load_gguf_compat()
        self.compat.patch_gguf_compat()

    def _register_file(self, path, keys, fields=None, entries=None):
        if entries is None:
            entries = [
                self.TensorEntry(
                    k,
                    np.zeros((2, 4), dtype=np.float32),
                    self.QType.F32,
                )
                for k in keys
            ]
        self.registry.files[path] = {
            "fields": fields or {},
            "tensors": entries,
        }

    def test_untagged_minimax_h3_video_and_image(self):
        for arch_tag in (None, "pig", "cow"):
            fields = {"general.architecture": arch_tag} if arch_tag is not None else {}
            video_path = f"minimax_h3_{arch_tag}.gguf"
            self._register_file(
                video_path,
                [
                    "video_patch_proj.weight",
                    "condition_proj.weight",
                    "audio_patch_proj.weight",
                ],
                fields=fields,
            )
            sd, extra = self.nodes.gguf_sd_loader(video_path)
            self.assertEqual(extra["arch_str"], "minimax_h3")
            self.assertIn("video_patch_proj.weight", sd)

            img_path = f"minimax_h3_image_{arch_tag}.gguf"
            self._register_file(
                img_path,
                ["video_patch_proj.weight", "condition_proj.weight"],
                fields=fields,
            )
            _, extra_img = self.nodes.gguf_sd_loader(img_path)
            self.assertEqual(extra_img["arch_str"], "minimax_h3_image")

    def test_explicit_img_arch_allowlist(self):
        expected = [
            "minimax_h3",
            "minimax_h3_image",
            "krea2",
            "qwen_image21",
            "moss_image",
            "ming_image",
            "flux2",
            "flux3",
            "anima",
            "chroma",
            "pixart",
            "stable_cascade",
            "ernie",
        ]
        for idx, arch in enumerate(expected):
            path = f"tagged_{idx}.gguf"
            self._register_file(
                path,
                ["dummy.weight"],
                fields={"general.architecture": arch},
            )
            _, extra = self.nodes.gguf_sd_loader(path)
            self.assertEqual(extra["arch_str"], arch)

    def test_untagged_2026_architectures_and_prefixed_keys(self):
        cases = [
            ("krea2", ["txtfusion.projector.weight", "first.weight"]),
            (
                "qwen_image21",
                [
                    "txt_in.text_norm.weight",
                    "modulation.1.weight",
                    "transformer_blocks.0.attn.norm_q.weight",
                    "img_in.weight",
                    "proj_out.weight",
                ],
            ),
            (
                "moss_image",
                ["shared_block_modulation.weight", "layers.0.proj.ffn_up_gate.weight"],
            ),
            (
                "ming_image",
                ["cap_embedder.1.weight", "noise_refiner.0.attention.k_norm.weight"],
            ),
            (
                "flux3",
                [
                    "content_mode_blocks.image.0.attn_out.weight",
                    "single_stream_modulations.txt.lin.weight",
                ],
            ),
            ("flux2", ["double_stream_modulation_img.lin.weight"]),
            ("chroma", ["distilled_guidance_layer.0.norms.0.weight"]),
            ("anima", ["llm_adapter.blocks.0.cross_attn.q_proj.weight"]),
            ("pixart", ["t_block.1.weight"]),
            ("stable_cascade", ["clf.1.weight"]),
            ("ernie", ["layers.0.mlp.linear_fc2.weight"]),
            (
                "minimax_h3",
                [
                    "diffusion_model.video_patch_proj.weight",
                    "diffusion_model.condition_proj.weight",
                    "diffusion_model.audio_patch_proj.weight",
                ],
            ),
        ]
        for expected_arch, keys in cases:
            path = f"untagged_{expected_arch}_{len(keys)}.gguf"
            self._register_file(path, keys)
            _, extra = self.nodes.gguf_sd_loader(path)
            self.assertEqual(extra["arch_str"], expected_arch, msg=f"failed on {keys}")

    def test_untagged_native_text_encoder_gguf_loads_as_comfy_raw(self):
        path = "qwen3vl_32b_minimax_h3-Q2_K_M.gguf"
        # GGUF shape (cols, rows) -> reversed to PyTorch (rows, cols)
        emb = np.zeros((151936, 16), dtype=np.uint8)
        qproj = np.zeros((8192, 16), dtype=np.uint8)
        norm1d = np.ones((16,), dtype=np.float32)
        entries = [
            self.TensorEntry("model.embed_tokens.weight", emb, self.QType.Q2_K, shape=(16, 151936)),
            self.TensorEntry(
                "model.layers.0.self_attn.q_proj.weight",
                qproj,
                self.QType.Q2_K,
                shape=(16, 8192),
            ),
            self.TensorEntry("model.layers.0.input_layernorm.weight", norm1d, self.QType.BF16, shape=(16,)),
            self.TensorEntry("visual.blocks.0.attn.qkv.weight", qproj, self.QType.Q2_K, shape=(16, 8192)),
        ]
        self._register_file(path, [], entries=entries)

        sd, extra = self.nodes.gguf_sd_loader(path, is_text_model=True)
        self.assertEqual(extra["arch_str"], "comfy_raw")
        self.assertEqual(tuple(sd["model.embed_tokens.weight"].shape), (151936, 16))
        self.assertTrue(sd["model.embed_tokens.weight"].is_largest_weight)
        self.assertIn("visual.blocks.0.attn.qkv.weight", sd)
        self.assertEqual(sd["model.layers.0.input_layernorm.weight"].dtype, torch.float32)

        sd_clip = self.nodes.gguf_clip_loader(path)
        self.assertIn("model.embed_tokens.weight", sd_clip)
        self.assertEqual(self.loader.remap_calls, 0)

    def test_untagged_invalid_text_encoder_still_raises(self):
        path = "bad_text.gguf"
        self._register_file(path, ["totally.unknown.weight"])
        with self.assertRaises(ValueError) as ctx:
            self.nodes.gguf_sd_loader(path, is_text_model=True)
        self.assertIn("incompatible with llama.cpp", str(ctx.exception))

    def test_int8_scaled_to_q8_0(self):
        rows, cols = 2, 64
        weights = np.arange(rows * cols, dtype=np.int8).reshape(rows, cols)
        per_tensor_scale = np.array([0.25], dtype=np.float32)
        out_tensor = self.compat.int8_scaled_to_q8_0(
            torch.from_numpy(weights),
            torch.from_numpy(per_tensor_scale),
            (rows, cols),
        )
        self.assertEqual(tuple(out_tensor.shape), (rows, (cols // 32) * 34))

        per_row_scale = np.array([0.25, 0.5], dtype=np.float32)
        out_row = self.compat.int8_scaled_to_q8_0(
            torch.from_numpy(weights),
            torch.from_numpy(per_row_scale),
            (rows, cols),
        )
        self.assertEqual(tuple(out_row.shape), (rows, (cols // 32) * 34))

    def test_patch_gguf_compat_is_idempotent(self):
        arch_count = len(self.convert.arch_list)
        wrapped_fn = self.loader.gguf_sd_loader
        self.compat.patch_gguf_compat()
        self.compat.patch_gguf_compat()
        self.assertEqual(len(self.convert.arch_list), arch_count)
        self.assertIs(self.loader.gguf_sd_loader, wrapped_fn)
        self.assertIs(self.nodes.gguf_sd_loader, wrapped_fn)


if __name__ == "__main__":
    unittest.main()
