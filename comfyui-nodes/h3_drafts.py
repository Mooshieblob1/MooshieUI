"""Retained H3 drafts: bounded JSON + safetensors, and a learned 2x continuation.

No pickle, uploaded package import, or client-supplied filesystem paths. Draft IDs
are random capabilities; MooshieUI separately enforces gallery ownership.
"""
import json
import math
import os
from pathlib import Path
import re
import shutil
import threading
import time
import uuid

import folder_paths

FORMAT = "mooshie-h3-draft-v1"
MODEL_NAME = "h3_clean_latent_upscaler_film_epoch200.safetensors"
MODEL_BYTES = 59022848
MODEL_SHA256 = "984afb58f11d01274b90d880596ce2f93bc9c512db66fdeecd4e2d99b371d3e4"
MAX_JSON = 4 * 1024 * 1024
MAX_TENSORS = 2 * 1024 * 1024 * 1024
_lock = threading.RLock()
_last_cleanup = 0.0


def draft_root():
    root = Path(folder_paths.get_output_directory()).resolve() / "mooshie_h3_drafts"
    root.mkdir(exist_ok=True)
    if root.is_symlink() or root.resolve().parent != Path(folder_paths.get_output_directory()).resolve():
        raise ValueError("Draft storage must be inside the ComfyUI output directory")
    return root


def draft_path(draft_id):
    if not isinstance(draft_id, str) or not re.fullmatch(r"[a-f0-9]{32}", draft_id):
        raise ValueError("Invalid draft identifier")
    root = draft_root()
    path = root / draft_id
    if path.is_symlink() or path.resolve().parent != root:
        raise ValueError("Invalid draft directory")
    return path


def model_path():
    return Path(folder_paths.models_dir) / "h3_latent_upscalers" / MODEL_NAME


def read_manifest(draft_id):
    path = draft_path(draft_id)
    meta_path, tensors_path = path / "manifest.json", path / "tensors.safetensors"
    if any(p.is_symlink() for p in (meta_path, tensors_path)):
        raise ValueError("Draft files cannot be symlinks")
    if meta_path.stat().st_size > MAX_JSON or tensors_path.stat().st_size > MAX_TENSORS:
        raise ValueError("Draft exceeds storage limits")
    meta = json.loads(meta_path.read_text(encoding="utf-8"))
    if meta.get("format") != FORMAT or meta.get("id") != draft_id:
        raise ValueError("Unsupported draft format")
    for key in ("width", "height", "frames"):
        if type(meta.get(key)) is not int or meta[key] <= 0:
            raise ValueError("Invalid draft dimensions")
    return meta


def mark_complete(draft_id):
    """Keep successful outputs, including clips waiting for a manual gallery save."""
    if not draft_id:
        return
    with _lock:
        meta = read_manifest(draft_id)
        meta["completed"] = True
        path = draft_path(draft_id)
        pending = path / "manifest.pending"
        pending.write_text(json.dumps(meta, ensure_ascii=False, allow_nan=False), encoding="utf-8")
        pending.replace(path / "manifest.json")


def cleanup_incomplete():
    """Prune only abandoned, incomplete writes older than a day, at most hourly."""
    global _last_cleanup
    now = time.time()
    with _lock:
        if now - _last_cleanup < 3600:
            return
        _last_cleanup = now
        for path in draft_root().iterdir():
            try:
                if path.is_symlink() or not path.is_dir() or now - path.stat().st_mtime < 86400:
                    continue
                if re.fullmatch(r"\.partial-[a-f0-9]{32}", path.name):
                    shutil.rmtree(path)
                elif re.fullmatch(r"[a-f0-9]{32}", path.name):
                    meta = read_manifest(path.name)
                    if meta.get("completed") is False and now - meta.get("created_at", now) > 86400:
                        shutil.rmtree(path)
            except (ValueError, KeyError, OSError, TypeError):
                # Corrupt or manually changed drafts require explicit deletion.
                continue


def encode_tree(value, tensors, depth=0):
    import torch
    from comfy.nested_tensor import NestedTensor
    if depth > 48 or len(tensors) > 20000:
        raise ValueError("Draft conditioning is too complex")
    if isinstance(value, NestedTensor):
        return {"kind": "nested", "value": [encode_tree(t, tensors, depth + 1) for t in value.unbind()]}
    if torch.is_tensor(value):
        key = f"t{len(tensors)}"
        tensors[key] = value.detach().cpu().contiguous().clone()
        return {"kind": "tensor", "value": key}
    if isinstance(value, (list, tuple)):
        return {"kind": "tuple" if isinstance(value, tuple) else "list", "value": [encode_tree(v, tensors, depth + 1) for v in value]}
    if isinstance(value, dict) and all(isinstance(k, str) for k in value):
        return {"kind": "dict", "value": {k: encode_tree(v, tensors, depth + 1) for k, v in value.items()}}
    if value is None or type(value) in (str, bool, int) or (type(value) is float and math.isfinite(value)):
        return {"kind": "scalar", "value": value}
    raise ValueError(f"Unsupported draft value: {type(value).__name__}")


def decode_tree(tree, tensors, depth=0):
    from comfy.nested_tensor import NestedTensor
    if depth > 48 or not isinstance(tree, dict) or set(tree) != {"kind", "value"}:
        raise ValueError("Invalid draft tensor schema")
    kind, value = tree["kind"], tree["value"]
    if kind == "tensor" and isinstance(value, str):
        return tensors[value]
    if kind == "scalar" and (value is None or type(value) in (str, bool, int) or (type(value) is float and math.isfinite(value))):
        return value
    if kind in ("list", "tuple", "nested") and isinstance(value, list):
        members = [decode_tree(v, tensors, depth + 1) for v in value]
        return NestedTensor(members) if kind == "nested" else tuple(members) if kind == "tuple" else members
    if kind == "dict" and isinstance(value, dict):
        return {k: decode_tree(v, tensors, depth + 1) for k, v in value.items()}
    raise ValueError("Unknown draft tensor schema")


def video_audio(latent):
    import torch
    from comfy.nested_tensor import NestedTensor
    samples = latent.get("samples")
    if not isinstance(samples, NestedTensor):
        raise ValueError("A joint H3 video/audio latent is required")
    members = list(samples.unbind())
    if len(members) != 2 or any(not torch.is_tensor(t) for t in members):
        raise ValueError("Invalid H3 streams")
    video, audio = members
    if video.ndim != 5 or video.shape[:2] != (1, 24) or audio.ndim != 4 or audio.shape[:3] != (1, 32, 2):
        raise ValueError("Unexpected H3 latent shape")
    return video, audio


class MooshieH3SaveDraft:
    @classmethod
    def INPUT_TYPES(cls):
        return {"required": {"samples": ("LATENT",), "positive": ("CONDITIONING",),
                "params_json": ("STRING",), "draft_id": ("STRING",)},
                "optional": {"audio": ("AUDIO",)}}
    RETURN_TYPES = ("STRING",)
    FUNCTION = "save"
    CATEGORY = "mooshie/video"

    def save(self, samples, positive, params_json, draft_id, audio=None):
        from safetensors.torch import save_file
        video, _ = video_audio(samples)
        if len(params_json.encode("utf-8")) > MAX_JSON:
            raise ValueError("Draft parameters are too large")
        params = json.loads(params_json)
        tensors = {}
        tree = encode_tree({"latent": samples, "positive": positive, "audio": audio}, tensors)
        if sum(t.numel() * t.element_size() for t in tensors.values()) > MAX_TENSORS - 1024 * 1024:
            raise ValueError("Retained draft exceeds 2 GiB")
        frames = int(params.get("video_frame_count", 0))
        meta = {"format": FORMAT, "id": draft_id, "width": int(video.shape[-1] * 16),
                "height": int(video.shape[-2] * 16), "frames": frames, "params": params, "tree": tree,
                "completed": False, "created_at": time.time()}
        encoded = json.dumps(meta, ensure_ascii=False, allow_nan=False)
        if len(encoded.encode("utf-8")) > MAX_JSON:
            raise ValueError("Draft manifest exceeds 4 MiB")
        with _lock:
            target = draft_path(draft_id)
            if target.exists():
                # A workflow re-execution must never replace an existing draft.
                raise ValueError("Draft identifier already exists; submit a new generation")
            staging = draft_root() / (".partial-" + uuid.uuid4().hex)
            try:
                staging.mkdir()
                save_file(tensors, str(staging / "tensors.safetensors"))
                (staging / "manifest.json").write_text(encoded, encoding="utf-8")
                staging.rename(target)
            finally:
                if staging.exists():
                    shutil.rmtree(staging)
        return (draft_id,)


class MooshieH3LoadDraft:
    @classmethod
    def INPUT_TYPES(cls):
        return {"required": {"draft_id": ("STRING",)}}
    RETURN_TYPES = ("LATENT", "CONDITIONING", "AUDIO")
    FUNCTION = "load"
    CATEGORY = "mooshie/video"

    def load(self, draft_id):
        from safetensors.torch import load_file
        with _lock:
            meta = read_manifest(draft_id)
            tensors = load_file(str(draft_path(draft_id) / "tensors.safetensors"), device="cpu")
            data = decode_tree(meta["tree"], tensors)
        video_audio(data["latent"])
        return data["latent"], data["positive"], data["audio"]

    @classmethod
    def IS_CHANGED(cls, draft_id):
        # Check disk again after deletion or external edits, even on repeat jobs.
        return float("nan")


def resize_conditioning(positive):
    import torch
    import torch.nn.functional as F
    def resize(value):
        if not torch.is_tensor(value) or value.ndim not in (4, 5):
            raise ValueError("Unsupported H3 reference latent layout")
        shape = value.shape
        flat = value.reshape(-1, 1, shape[-2], shape[-1]).float()
        out = F.interpolate(flat, scale_factor=2, mode="nearest-exact")
        return out.reshape(*shape[:-2], shape[-2] * 2, shape[-1] * 2).to(value.dtype)
    result = []
    for embedding, settings in positive:
        settings = dict(settings)
        for key in ("minimax_refs", "minimax_keyframes"):
            if key not in settings:
                continue
            entries = []
            for item in settings[key]:
                item = dict(item)
                if item.get("kind") == "audio":
                    entries.append(item)
                    continue
                if item.get("latent") is not None:
                    item["latent"] = resize(item["latent"])
                    if key == "minimax_refs":
                        item["latent_h"], item["latent_w"] = map(int, item["latent"].shape[-2:])
                entries.append(item)
            settings[key] = entries
        result.append([embedding, settings])
    return result


class MooshieH3UpscaleDraft:
    @classmethod
    def INPUT_TYPES(cls):
        return {"required": {"samples": ("LATENT",), "positive": ("CONDITIONING",),
                "steps": ("INT", {"default": 8, "min": 4, "max": 20}),
                "sigma": ("FLOAT", {"default": 0.35, "min": 0.15, "max": 0.6})}}
    RETURN_TYPES = ("LATENT", "CONDITIONING", "SIGMAS")
    FUNCTION = "upscale"
    CATEGORY = "mooshie/video"

    def upscale(self, samples, positive, steps, sigma):
        import hashlib
        import torch
        import comfy.model_management as mm
        import comfy.model_patcher
        import comfy.utils
        from comfy.nested_tensor import NestedTensor
        from .h3_upscaler import read_checkpoint_info, build_upscaler
        if not 4 <= steps <= 20 or not 0.15 <= sigma <= 0.6:
            raise ValueError("Invalid refinement settings")
        video, audio = video_audio(samples)
        if video.shape[-1] * video.shape[-2] * 16 * 16 * 4 > 2200000:
            raise ValueError("Choose a smaller draft; 2x output exceeds 2.2 megapixels")
        path = model_path()
        if not path.is_file() or path.stat().st_size != MODEL_BYTES:
            raise ValueError("Install the H3 2x latent upscaler in MooshieUI first")
        with path.open("rb") as handle:
            if hashlib.file_digest(handle, "sha256").hexdigest() != MODEL_SHA256:
                raise ValueError("H3 upscaler checksum mismatch; reinstall its model")
        state = comfy.utils.load_torch_file(str(path), safe_load=True)
        state = {k: v.clone() for k, v in state.items()}
        model = build_upscaler(state, read_checkpoint_info(path))
        device, offload = mm.get_torch_device(), mm.unet_offload_device()
        model.to(device=offload, dtype=torch.float32)
        patcher = comfy.model_patcher.ModelPatcher(model, load_device=device, offload_device=offload)
        try:
            mm.load_models_gpu([patcher], memory_required=patcher.model_size() + video.numel() * 4 * 32, force_full_load=True)
            with torch.inference_mode():
                upscaled = model(video.to(device=device, dtype=torch.float32)).to(device="cpu", dtype=video.dtype)
        finally:
            if hasattr(mm, "unload_model_and_clones"):
                mm.unload_model_and_clones(patcher, unload_additional_models=False)
            else:
                patcher.unpatch_model(device_to=offload)
        audio = audio.cpu()
        # This node does not add noise. SamplerCustomAdvanced + RandomNoise adds
        # it exactly once, using these raw video sigmas. Masked audio stays fixed.
        latent = {"samples": NestedTensor([upscaled, audio]),
                  "noise_mask": NestedTensor([torch.ones_like(upscaled[:, :1]), torch.zeros_like(audio[:, :1])])}
        return latent, resize_conditioning(positive), torch.linspace(float(sigma), 0.0, steps + 1)


class MooshieH3RestoreAudio:
    @classmethod
    def INPUT_TYPES(cls):
        return {"required": {"samples": ("LATENT",), "original": ("LATENT",)}}
    RETURN_TYPES = ("LATENT",)
    FUNCTION = "restore"
    CATEGORY = "mooshie/video"
    def restore(self, samples, original):
        from comfy.nested_tensor import NestedTensor
        video, _ = video_audio(samples)
        _, audio = video_audio(original)
        return ({"samples": NestedTensor([video, audio])},)


def register_routes():
    from aiohttp import web
    from server import PromptServer
    server = PromptServer.instance
    if server is None or getattr(server, "_mooshie_h3_drafts", False):
        return
    server._mooshie_h3_drafts = True

    async def capabilities(request):
        cleanup_incomplete()
        path = model_path()
        return web.json_response({"version": 1, "upscaler_ready": path.is_file() and path.stat().st_size == MODEL_BYTES})

    async def metadata(request):
        try:
            with _lock:
                meta = read_manifest(request.match_info["draft_id"])
                size = sum(p.stat().st_size for p in draft_path(meta["id"]).iterdir() if p.is_file())
            return web.json_response({k: meta[k] for k in ("format", "id", "width", "height", "frames", "params")} | {"bytes": size})
        except FileNotFoundError:
            raise web.HTTPNotFound(text="Retained draft data is missing")
        except (ValueError, KeyError, OSError) as exc:
            raise web.HTTPBadRequest(text=str(exc))

    async def delete(request):
        try:
            draft_id = request.match_info["draft_id"]
            with _lock:
                path = draft_path(draft_id)
                running, pending = server.prompt_queue.get_current_queue()
                for entry in [*running, *pending]:
                    if any(node.get("class_type") == "MooshieH3LoadDraft" and node.get("inputs", {}).get("draft_id") == draft_id for node in entry[2].values()):
                        raise web.HTTPConflict(text="This draft has a queued refinement")
                if path.exists():
                    shutil.rmtree(path)
            return web.json_response({"deleted": True})
        except (ValueError, KeyError, OSError) as exc:
            raise web.HTTPBadRequest(text=str(exc))

    server.routes.get("/mooshie/h3/capabilities")(capabilities)
    server.routes.get("/mooshie/h3/drafts/{draft_id}")(metadata)
    server.routes.delete("/mooshie/h3/drafts/{draft_id}")(delete)


NODE_CLASS_MAPPINGS = {cls.__name__: cls for cls in (MooshieH3SaveDraft, MooshieH3LoadDraft, MooshieH3UpscaleDraft, MooshieH3RestoreAudio)}
