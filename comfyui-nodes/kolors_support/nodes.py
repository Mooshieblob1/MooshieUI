"""MooshieKolorsLoader: Kolors UNet + ChatGLM3 text encoder as MODEL and CLIP.

The VAE is a stock SDXL-style VAE and loads with core VAELoader, and the
outputs work with core CLIPTextEncode, KSampler and LoraLoader.
"""

from __future__ import annotations

import os
import re

import torch

import comfy.model_management
import comfy.sd
import comfy.utils
import folder_paths

from .model import load_kolors_unet
from .text_encoder import KolorsTEModel, ChatGLM3Tokenizer, ensure_tokenizer, prepare_state_dict

SHARD_RE = re.compile(r"^(?P<stem>.*)-(?P<index>\d{5})-of-(?P<total>\d{5})(?P<ext>\.safetensors)$")


def _preferred_first(options, preferred):
    hits = [name for name in options if any(p in name.lower() for p in preferred)]
    return hits + [name for name in options if name not in hits]


def text_encoder_files(path):
    """All shard paths of a sharded file (any of model-0000N-of-00003.safetensors), else just [path]."""
    folder, name = os.path.split(path)
    match = SHARD_RE.match(name)
    if match is None:
        return [path]
    total = int(match.group("total"))
    files = []
    for index in range(1, total + 1):
        shard = os.path.join(
            folder, "{}-{:05d}-of-{:05d}{}".format(match.group("stem"), index, total, match.group("ext"))
        )
        if not os.path.isfile(shard):
            raise RuntimeError(
                "ChatGLM3 shard {} of {} is missing: {}. Download all {} parts into the same folder.".format(
                    index, total, shard, total
                )
            )
        files.append(shard)
    return files


def load_chatglm3_clip(path, clip_device="default", disable_dynamic=False):
    sd = {}
    for shard in text_encoder_files(path):
        sd.update(comfy.utils.load_torch_file(shard, safe_load=True))
    sd = prepare_state_dict(sd)

    model_options = {}
    if clip_device == "cpu":
        model_options["load_device"] = torch.device("cpu")
        model_options["offload_device"] = torch.device("cpu")

    class ClipTarget:
        params = {}
        clip = KolorsTEModel
        tokenizer = ChatGLM3Tokenizer

    clip = comfy.sd.CLIP(
        ClipTarget,
        embedding_directory=folder_paths.get_folder_paths("embeddings"),
        tokenizer_data={"chatglm3_tokenizer_path": ensure_tokenizer(os.path.dirname(path))},
        parameters=comfy.utils.calculate_parameters(sd),
        state_dict=[sd],
        model_options=model_options,
        disable_dynamic=disable_dynamic,
    )
    clip.patcher.cached_patcher_init = (load_chatglm3_clip, (path, clip_device, disable_dynamic))
    return clip


class MooshieKolorsLoader:
    @classmethod
    def INPUT_TYPES(cls):
        unets = folder_paths.get_filename_list("diffusion_models")
        # Any shard of a sharded encoder may be picked: all of them load together.
        encoders = [n for n in folder_paths.get_filename_list("text_encoders") if n.endswith(".safetensors")]
        return {
            "required": {
                "unet_name": (_preferred_first(unets, ["kolors"]),),
                "text_encoder_name": (_preferred_first(encoders, ["chatglm", "kolors"]),),
            },
            "optional": {
                "weight_dtype": (["default", "fp8_e4m3fn", "fp8_e4m3fn_fast", "fp8_e5m2"], {"advanced": True}),
                "clip_device": (["default", "cpu"], {"advanced": True}),
                # Absolute path that overrides unet_name, for a Kolors UNet that
                # lives outside models/diffusion_models (e.g. in checkpoints/).
                "unet_path": ("STRING", {"default": "", "advanced": True}),
            },
        }

    RETURN_TYPES = ("MODEL", "CLIP")
    FUNCTION = "load"
    CATEGORY = "loaders"
    DESCRIPTION = "Loads Kolors (UNet + ChatGLM3-6B text encoder) without third-party node packs."

    @classmethod
    def VALIDATE_INPUTS(cls, unet_name, unet_path=""):
        # With unet_path set, unet_name is ignored, so it may name a file that
        # is not in the diffusion_models listing.
        return True

    def load(self, unet_name, text_encoder_name, weight_dtype="default", clip_device="default", unet_path=""):
        model_options = {}
        if weight_dtype.startswith("fp8") and comfy.model_management.get_torch_device().type == "mps":
            raise ValueError("FP8 precision is not supported on Apple Metal. Use default precision.")
        if weight_dtype == "fp8_e4m3fn":
            model_options["dtype"] = torch.float8_e4m3fn
        elif weight_dtype == "fp8_e4m3fn_fast":
            model_options["dtype"] = torch.float8_e4m3fn
            model_options["fp8_optimizations"] = True
        elif weight_dtype == "fp8_e5m2":
            model_options["dtype"] = torch.float8_e5m2

        unet_path = (unet_path or "").strip()
        if unet_path:
            if not os.path.isfile(unet_path):
                raise FileNotFoundError("Kolors UNet not found: {}".format(unet_path))
        else:
            unet_path = folder_paths.get_full_path_or_raise("diffusion_models", unet_name)
        model = load_kolors_unet(unet_path, model_options)
        clip = load_chatglm3_clip(
            folder_paths.get_full_path_or_raise("text_encoders", text_encoder_name), clip_device=clip_device
        )
        return (model, clip)


NODE_CLASS_MAPPINGS = {"MooshieKolorsLoader": MooshieKolorsLoader}
NODE_DISPLAY_NAME_MAPPINGS = {"MooshieKolorsLoader": "Load Kolors (Mooshie)"}
