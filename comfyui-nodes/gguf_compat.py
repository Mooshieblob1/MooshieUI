"""Let city96/ComfyUI-GGUF load Krea 2 diffusion models.

Two gaps in the pinned ComfyUI-GGUF (upstream latest included), both patched at
runtime here:

1. Architecture gate. Every .gguf UNet is checked against a hard-coded
   `general.architecture` allowlist (`loader.IMG_ARCH_LIST`), and the fallback
   detector for untagged / "pig" / "cow" files (`tools.convert.arch_list`) has
   no Krea 2 entry. ComfyUI core detects Krea 2 from the tensor names
   (`txtfusion.projector.weight`), so the tag is only a gate: Krea 2 GGUFs
   tagged "krea2" (or "pig") already carry core's key layout.

2. Scaled int8 weights. Some Krea 2 "int8" GGUFs (converted from int8
   safetensors) store each weight as a raw I8 tensor plus a float
   `<name>_scale` tensor (weight = int8 * scale). ComfyUI-GGUF cannot
   dequantize I8 and drops the scales as unexpected keys. A per-tensor or
   per-output-row scale is exactly a Q8_0 block scale, so those weights are
   repacked into Q8_0 blocks (32 int8 values + one fp16 scale), which the
   loader dequantizes natively at the same size. Any other scale layout is
   left untouched and fails as before rather than producing wrong weights.

Custom node packs load in os.listdir order, which is not guaranteed to put
ComfyUI-GGUF first, so the patch runs at import and again from an on_prompt
handler (before any loader node can execute). It is idempotent and a no-op
when ComfyUI-GGUF is absent.
"""
import importlib
import logging
import sys

import torch

KREA2_ARCH = "krea2"
# Keys ComfyUI core's model_detection uses to recognise a Krea 2 DiT.
KREA2_DETECT_KEYS = ("txtfusion.projector.weight", "first.weight")
SCALE_SUFFIX = "_scale"
Q8_0_BLOCK = 32

_patched_loaders = set()


def _gguf_loader_modules():
    for name, module in list(sys.modules.items()):
        if module is None or not name.endswith(".loader"):
            continue
        arch_list = getattr(module, "IMG_ARCH_LIST", None)
        if isinstance(arch_list, set) and hasattr(module, "gguf_sd_loader"):
            yield module


def _register_krea2_detector(loader):
    package = getattr(loader, "__package__", None)
    if not package:
        return
    convert = importlib.import_module(f"{package}.tools.convert")
    arch_list = getattr(convert, "arch_list", None)
    template = getattr(convert, "ModelTemplate", None)
    if arch_list is None or template is None:
        return
    if any(getattr(arch, "arch", None) == KREA2_ARCH for arch in arch_list):
        return
    arch_list.append(
        type("ModelKrea2", (template,), {"arch": KREA2_ARCH, "keys_detect": [KREA2_DETECT_KEYS]})
    )


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
    i8 = gguf.GGMLQuantizationType.I8
    q8_0 = gguf.GGMLQuantizationType.Q8_0
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


def _wrap_sd_loader(loader):
    original = loader.gguf_sd_loader

    def gguf_sd_loader(*args, **kwargs):
        state_dict, extra = original(*args, **kwargs)
        _repack_scaled_int8(loader, state_dict)
        return state_dict, extra

    gguf_sd_loader.__wrapped__ = original
    # nodes.py binds the function by name (`from .loader import gguf_sd_loader`),
    # so each ComfyUI-GGUF module holding the original reference is repointed.
    # Only that package's modules are touched: getattr on arbitrary modules can
    # trigger lazy imports (transformers).
    prefix = loader.__package__ + "."
    for name, module in list(sys.modules.items()):
        if (
            module is not None
            and name.startswith(prefix)
            and module.__dict__.get("gguf_sd_loader") is original
        ):
            module.gguf_sd_loader = gguf_sd_loader


def patch_gguf_krea2():
    loaders = list(_gguf_loader_modules())
    if not loaders and not _patched_loaders:
        logging.info("[MooshieUI] ComfyUI-GGUF not loaded yet; Krea 2 GGUF patch deferred")
    for loader in loaders:
        if id(loader) in _patched_loaders:
            continue
        loader.IMG_ARCH_LIST.add(KREA2_ARCH)
        try:
            _register_krea2_detector(loader)
        except Exception as e:  # untagged Krea 2 files only; tagged ones still load
            logging.warning(f"[MooshieUI] GGUF Krea 2 detector not registered: {e}")
        try:
            _wrap_sd_loader(loader)
        except Exception as e:  # scaled int8 files only; regular quants still load
            logging.warning(f"[MooshieUI] GGUF scaled int8 support not installed: {e}")
        _patched_loaders.add(id(loader))
        logging.info(f"[MooshieUI] ComfyUI-GGUF: enabled '{KREA2_ARCH}' architecture")


def _on_prompt(json_data):
    patch_gguf_krea2()
    return json_data


def install():
    patch_gguf_krea2()
    try:
        from server import PromptServer

        PromptServer.instance.add_on_prompt_handler(_on_prompt)
    except Exception as e:
        logging.warning(f"[MooshieUI] GGUF compat on_prompt hook not installed: {e}")
