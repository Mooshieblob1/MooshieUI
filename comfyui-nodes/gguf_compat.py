"""Let city96/ComfyUI-GGUF load Krea 2 diffusion models.

ComfyUI-GGUF gates every .gguf UNet on a hard-coded `general.architecture`
allowlist (`loader.IMG_ARCH_LIST`) before handing the state dict to ComfyUI,
and its fallback detector for untagged / "pig" / "cow" files
(`tools.convert.arch_list`) has no Krea 2 entry either. ComfyUI core detects
Krea 2 from the tensor names (`txtfusion.projector.weight`), so the arch tag is
only a gate: community Krea 2 GGUFs tagged "krea2" (or "pig") already carry
core's key layout and load fine once let through. Upstream (pinned commit
included) has no Krea 2 support, hence this shim.

Custom node packs load in os.listdir order, which is not guaranteed to put
ComfyUI-GGUF first, so the patch runs at import and again from an on_prompt
handler (before any loader node can execute). It is idempotent and a no-op
when ComfyUI-GGUF is absent.
"""
import importlib
import logging
import sys

KREA2_ARCH = "krea2"
# Keys ComfyUI core's model_detection uses to recognise a Krea 2 DiT.
KREA2_DETECT_KEYS = ("txtfusion.projector.weight", "first.weight")

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


def patch_gguf_krea2():
    for loader in _gguf_loader_modules():
        if id(loader) in _patched_loaders:
            continue
        loader.IMG_ARCH_LIST.add(KREA2_ARCH)
        try:
            _register_krea2_detector(loader)
        except Exception as e:  # untagged Krea 2 files only; tagged ones still load
            logging.warning(f"[MooshieUI] GGUF Krea 2 detector not registered: {e}")
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
