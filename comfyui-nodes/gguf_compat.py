"""Extend city96/ComfyUI-GGUF to support newer diffusion and text-encoder GGUFs.

Runtime patches applied to ComfyUI-GGUF (idempotent, no-op when absent):

1. Architecture allowlist (`IMG_ARCH_LIST`). Lets tagged GGUFs for newer
   ComfyUI-native models (MiniMax H3 video/image, Krea 2, Qwen-Image 2.1,
   MOSS-Image, Ming-Image, Flux 2/3, Chroma, Anima, PixArt, Stable Cascade,
   Ernie, etc.) pass the GGUF architecture check and reach ComfyUI core.

2. Untagged fallback detectors (`tools.convert.arch_list`). Many community
   conversions (including Unsloth MiniMax-H3 GGUFs) omit `general.architecture`
   or tag `"pig"`/`"cow"`. Registering tensor-key signatures (matching ComfyUI
   core's `comfy/model_detection.py`) lets `detect_arch` accept them cleanly.

3. Native-keyed text-encoder GGUFs. Some community GGUFs (such as Unsloth's
   `qwen3vl_32b_minimax_h3-*.gguf`) quantize ComfyUI/HuggingFace-layout text
   encoders (`model.*` / `visual.*`) with no `general.architecture` metadata
   instead of llama.cpp `blk.*` keys. When `gguf_sd_loader(..., is_text_model=True)`
   rejects an untagged file as incompatible with llama.cpp, recognize native
   text-encoder tensor layouts and pass them through untouched as `comfy_raw`.

4. Scaled int8 weights. Krea 2 "int8" GGUFs can store raw `I8` tensors plus
   `<name>_scale` tensors; per-tensor and per-row scales are repacked into
   native `Q8_0` blocks so ComfyUI-GGUF can dequantize them.

Custom node packs load in `os.listdir` order, which is not guaranteed to put
ComfyUI-GGUF first, so the patch runs at import and again from an `on_prompt`
handler before any loader node executes.
"""
import importlib
import logging
import sys
import warnings

import torch

KREA2_ARCH = "krea2"
# Keys ComfyUI core's model_detection uses to recognise a Krea 2 DiT.
KREA2_DETECT_KEYS = ("txtfusion.projector.weight", "first.weight")
SCALE_SUFFIX = "_scale"
Q8_0_BLOCK = 32

EXTRA_IMG_ARCHS = frozenset(
    {
        KREA2_ARCH,
        "minimax_h3",
        "minimax_h3_image",
        "minimax",
        "qwen_image21",
        "qwen_image_21",
        "moss_image",
        "moss",
        "ming_image",
        "ming",
        "flux2",
        "flux2_klein",
        "flux3",
        "flux3_klein",
        "anima",
        "chroma",
        "z_image",
        "zimage",
        "pixart",
        "pixart_sigma",
        "pixart_alpha",
        "cascade",
        "stable_cascade",
        "kolors",
        "nanosaur",
        "ernie",
    }
)

# Ordered specifically so more specific variants (e.g. minimax_h3 with audio)
# match before narrower ones (e.g. minimax_h3_image).
EXTRA_ARCH_DETECTORS = (
    (
        KREA2_ARCH,
        (KREA2_DETECT_KEYS,),
    ),
    (
        "minimax_h3",
        (("video_patch_proj.weight", "condition_proj.weight", "audio_patch_proj.weight"),),
    ),
    (
        "minimax_h3_image",
        (("video_patch_proj.weight", "condition_proj.weight"),),
    ),
    (
        "qwen_image21",
        (
            (
                "txt_in.text_norm.weight",
                "modulation.1.weight",
                "transformer_blocks.0.attn.norm_q.weight",
                "img_in.weight",
                "proj_out.weight",
            ),
        ),
    ),
    (
        "moss_image",
        (("shared_block_modulation.weight", "layers.0.proj.ffn_up_gate.weight"),),
    ),
    (
        "ming_image",
        (("cap_embedder.1.weight", "noise_refiner.0.attention.k_norm.weight"),),
    ),
    (
        "flux3",
        (("content_mode_blocks.image.0.attn_out.weight", "single_stream_modulations.txt.lin.weight"),),
    ),
    (
        "flux2",
        (("double_stream_modulation_img.lin.weight",),),
    ),
    (
        "chroma",
        (
            ("distilled_guidance_layer.0.norms.0.weight",),
            ("distilled_guidance_layer.0.norms.0.scale",),
            ("distilled_guidance_layer.norms.0.weight",),
            ("distilled_guidance_layer.norms.0.scale",),
        ),
    ),
    (
        "anima",
        (
            ("llm_adapter.blocks.0.cross_attn.q_proj.weight",),
            ("net.llm_adapter.blocks.0.cross_attn.q_proj.weight",),
            ("net.blocks.0.mlp.layer1.weight",),
        ),
    ),
    (
        "pixart",
        (
            ("t_block.1.weight",),
            ("adaln_single.emb.timestep_embedder.linear_1.bias", "pos_embed.proj.bias"),
        ),
    ),
    (
        "stable_cascade",
        (("clf.1.weight",),),
    ),
    (
        "ernie",
        (("layers.0.mlp.linear_fc2.weight",),),
    ),
)

NATIVE_TEXT_KEY_SETS = (
    ("model.embed_tokens.weight", "model.layers.0.self_attn.q_proj.weight"),
    (
        "model.language_model.embed_tokens.weight",
        "model.language_model.layers.0.self_attn.q_proj.weight",
    ),
    ("encoder.block.0.layer.0.SelfAttention.q.weight",),
    ("text_model.encoder.layers.0.self_attn.q_proj.weight",),
    ("transformer.encoder.layers.0.self_attention.query_key_value.weight",),
)

_patched_loaders = set()


def _gguf_loader_modules():
    for name, module in list(sys.modules.items()):
        if module is None or not name.endswith(".loader"):
            continue
        arch_list = getattr(module, "IMG_ARCH_LIST", None)
        if isinstance(arch_list, set) and hasattr(module, "gguf_sd_loader"):
            yield module


def _expand_prefixed_keys(key_sets):
    expanded = []
    for key_set in key_sets:
        expanded.append(key_set)
        if not any(k.startswith(("diffusion_model.", "net.")) for k in key_set):
            expanded.append(tuple(f"diffusion_model.{k}" for k in key_set))
    return expanded


def _register_extra_detectors(loader):
    package = getattr(loader, "__package__", None)
    if not package:
        return
    convert = importlib.import_module(f"{package}.tools.convert")
    arch_list = getattr(convert, "arch_list", None)
    template = getattr(convert, "ModelTemplate", None)
    if arch_list is None or template is None:
        return
    existing = {getattr(arch, "arch", None) for arch in arch_list}
    for arch_name, key_sets in EXTRA_ARCH_DETECTORS:
        if arch_name in existing:
            continue
        cls_name = "Model" + "".join(part.capitalize() for part in arch_name.split("_"))
        arch_list.append(
            type(
                cls_name,
                (template,),
                {"arch": arch_name, "keys_detect": _expand_prefixed_keys(key_sets)},
            )
        )
        existing.add(arch_name)


def int8_scaled_to_q8_0(weight, scale, shape):
    """Repack an int8 weight of logical `shape` (rows, cols) and its scale into
    Q8_0 block bytes, or return None when the scale is not per-tensor or
    per-row (those are the only layouts a Q8_0 block scale represents)."""
    if len(shape) != 2 or shape[1] % Q8_0_BLOCK:
        return None
    rows, cols = int(shape[0]), int(shape[1])
    scale = scale.as_subclass(torch.Tensor).to(torch.float32)
    if scale.numel() == 1:
        scale = scale.reshape(1, 1, 1)
    elif scale.numel() == rows:
        scale = scale.reshape(rows, 1, 1)
    else:
        return None
    n_blocks = cols // Q8_0_BLOCK
    d = scale.expand(rows, n_blocks, 1).to(torch.float16).contiguous().view(torch.uint8)
    qs = weight.as_subclass(torch.Tensor).reshape(rows, n_blocks, Q8_0_BLOCK).view(torch.uint8)
    return torch.cat((d, qs), dim=2).reshape(rows, n_blocks * (2 + Q8_0_BLOCK))


def _repack_scaled_int8(loader, state_dict):
    gguf = loader.gguf
    i8 = getattr(gguf.GGMLQuantizationType, "I8", None)
    q8_0 = getattr(gguf.GGMLQuantizationType, "Q8_0", None)
    if i8 is None or q8_0 is None:
        return
    repacked = 0
    for key in [k for k, v in state_dict.items() if getattr(v, "tensor_type", None) == i8]:
        scale = state_dict.get(key + SCALE_SUFFIX)
        if scale is None:
            continue
        weight = state_dict[key]
        shape = getattr(weight, "tensor_shape", weight.shape)
        blocks = int8_scaled_to_q8_0(weight, scale, shape)
        if blocks is None:
            continue
        state_dict[key] = loader.GGMLTensor(blocks, tensor_type=q8_0, tensor_shape=shape)
        del state_dict[key + SCALE_SUFFIX]
        repacked += 1
    if repacked:
        # The loader flags its largest quantized weight for VRAM estimation.
        quantized = {k: v for k, v in state_dict.items() if loader.is_quantized(v)}
        for v in quantized.values():
            v.is_largest_weight = False
        max_key = max(quantized, key=lambda k: quantized[k].numel())
        state_dict[max_key].is_largest_weight = True
        logging.info(f"[MooshieUI] ComfyUI-GGUF: repacked {repacked} scaled int8 weights as Q8_0")


def _is_native_text_keys(keys):
    if "blk.0.attn_q.weight" in keys:
        return False
    return any(all(k in keys for k in key_set) for key_set in NATIVE_TEXT_KEY_SETS)


def _load_native_text_gguf(loader, path, handle_prefix):
    reader = loader.gguf.GGUFReader(path)
    has_prefix = False
    prefix_len = 0
    if handle_prefix is not None:
        prefix_len = len(handle_prefix)
        tensor_names = {tensor.name for tensor in reader.tensors}
        has_prefix = any(s.startswith(handle_prefix) for s in tensor_names)

    tensors = []
    for tensor in reader.tensors:
        sd_key = tensor.name
        if has_prefix:
            if not sd_key.startswith(handle_prefix):
                continue
            sd_key = sd_key[prefix_len:]
        tensors.append((sd_key, tensor))

    if not _is_native_text_keys({sd_key for sd_key, _ in tensors}):
        return None

    ggml_qtype = loader.gguf.GGMLQuantizationType
    f32_type = getattr(ggml_qtype, "F32", None)
    f16_type = getattr(ggml_qtype, "F16", None)
    bf16_type = getattr(ggml_qtype, "BF16", None)
    get_orig_shape = getattr(loader, "get_orig_shape", None)
    get_meta = getattr(loader, "get_gguf_metadata", None)

    state_dict = {}
    for sd_key, tensor in tensors:
        with warnings.catch_warnings():
            warnings.filterwarnings("ignore", message="The given NumPy array is not writable")
            torch_tensor = torch.from_numpy(tensor.data)

        shape = get_orig_shape(reader, tensor.name) if get_orig_shape else None
        if shape is None:
            shape = torch.Size(tuple(int(v) for v in reversed(tensor.shape)))

        if tensor.tensor_type in {f32_type, f16_type}:
            torch_tensor = torch_tensor.view(*shape)

        state_dict[sd_key] = loader.GGMLTensor(
            torch_tensor, tensor_type=tensor.tensor_type, tensor_shape=shape
        )

        if len(shape) <= 1 and bf16_type is not None and tensor.tensor_type == bf16_type:
            state_dict[sd_key] = loader.dequantize_tensor(state_dict[sd_key], dtype=torch.float32)

    qsd = {k: v for k, v in state_dict.items() if loader.is_quantized(v)}
    if qsd:
        max_key = max(qsd.keys(), key=lambda k: qsd[k].numel())
        state_dict[max_key].is_largest_weight = True

    metadata = get_meta(reader) if get_meta else {}
    logging.info("[MooshieUI] ComfyUI-GGUF: loaded native-keyed text encoder GGUF [arch:comfy_raw]")
    return state_dict, {"arch_str": "comfy_raw", "metadata": metadata}


def _wrap_sd_loader(loader):
    original = loader.gguf_sd_loader
    if getattr(original, "__wrapped__", None) is not None:
        return

    def gguf_sd_loader(*args, **kwargs):
        try:
            state_dict, extra = original(*args, **kwargs)
        except ValueError as exc:
            is_text_model = (
                bool(args[2]) if len(args) >= 3 else bool(kwargs.get("is_text_model", False))
            )
            if not is_text_model or "incompatible with llama.cpp" not in str(exc):
                raise
            path = args[0] if args else kwargs.get("path")
            handle_prefix = (
                args[1] if len(args) >= 2 else kwargs.get("handle_prefix", "model.diffusion_model.")
            )
            loaded = _load_native_text_gguf(loader, path, handle_prefix)
            if loaded is None:
                raise
            state_dict, extra = loaded
        _repack_scaled_int8(loader, state_dict)
        return state_dict, extra

    gguf_sd_loader.__wrapped__ = original
    # nodes.py binds the function by name (`from .loader import gguf_sd_loader`),
    # so each ComfyUI-GGUF module holding the original reference is repointed.
    # Only that package's modules are touched: getattr on arbitrary modules can
    # trigger lazy imports (transformers).
    package = getattr(loader, "__package__", None)
    if not package:
        loader.gguf_sd_loader = gguf_sd_loader
        return
    prefix = package + "."
    for name, module in list(sys.modules.items()):
        if (
            module is not None
            and (name.startswith(prefix) or module is loader)
            and module.__dict__.get("gguf_sd_loader") is original
        ):
            module.gguf_sd_loader = gguf_sd_loader


def patch_gguf_compat():
    loaders = list(_gguf_loader_modules())
    if not loaders and not _patched_loaders:
        logging.info("[MooshieUI] ComfyUI-GGUF not loaded yet; GGUF compat patch deferred")
    for loader in loaders:
        if id(loader) in _patched_loaders:
            continue
        loader.IMG_ARCH_LIST.update(EXTRA_IMG_ARCHS)
        try:
            _register_extra_detectors(loader)
        except Exception as e:
            logging.warning(f"[MooshieUI] GGUF extra architecture detectors not registered: {e}")
        try:
            _wrap_sd_loader(loader)
        except Exception as e:
            logging.warning(f"[MooshieUI] GGUF compat loader wrapper not installed: {e}")
        _patched_loaders.add(id(loader))
        logging.info(
            "[MooshieUI] ComfyUI-GGUF: enabled MiniMax H3 and 2026 architecture GGUF compatibility"
        )


patch_gguf_krea2 = patch_gguf_compat


def _on_prompt(json_data):
    patch_gguf_compat()
    return json_data


def install():
    patch_gguf_compat()
    try:
        from server import PromptServer

        PromptServer.instance.add_on_prompt_handler(_on_prompt)
    except Exception as e:
        logging.warning(f"[MooshieUI] GGUF compat on_prompt hook not installed: {e}")
