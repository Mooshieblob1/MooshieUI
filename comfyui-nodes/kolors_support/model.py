"""Kolors UNet support for ComfyUI without patching ComfyUI's model detection.

Kolors is an SDXL-shaped UNet with two differences:
  * ``encoder_hid_proj``: a Linear(4096 -> 2048) that projects ChatGLM3's
    hidden states into the cross-attention width before the UNet runs;
  * a 5632-wide ADM vector (4096 pooled text + the usual 6x256 size/crop
    embeddings) and a 1100-step scaled-linear noise schedule
    (beta 0.00085 to 0.014), per Kwai-Kolors' scheduler_config.json.

Core ComfyUI does not recognise it, so this module builds the model config
directly instead of going through ``model_detection``. Everything else
(sampling, LoRA key mapping, ControlNet-free inpaint concat, memory
management) is stock ComfyUI because the model is a ``model_base.SDXL``.
"""

from __future__ import annotations

import logging

import torch

import comfy.latent_formats
import comfy.model_base
import comfy.model_detection
import comfy.model_management
import comfy.model_patcher
import comfy.supported_models
import comfy.utils
from comfy.ldm.modules.diffusionmodules.openaimodel import UNetModel

KOLORS_CONTEXT_IN = 4096
KOLORS_CONTEXT_DIM = 2048
KOLORS_ADM = 5632

KOLORS_SAMPLING = {
    "beta_schedule": "linear",  # ComfyUI's "linear" is diffusers' scaled_linear
    "linear_start": 0.00085,
    "linear_end": 0.014,
    "timesteps": 1100,
}


def kolors_unet_config(in_channels=4):
    return comfy.model_detection.convert_config({
        "use_checkpoint": False,
        "image_size": 32,
        "out_channels": 4,
        "use_spatial_transformer": True,
        "legacy": False,
        "num_classes": "sequential",
        "adm_in_channels": KOLORS_ADM,
        "in_channels": in_channels,
        "model_channels": 320,
        "num_res_blocks": [2, 2, 2],
        "transformer_depth": [0, 0, 2, 2, 10, 10],
        "channel_mult": [1, 2, 4],
        "transformer_depth_middle": 10,
        "use_linear_in_transformer": True,
        "context_dim": KOLORS_CONTEXT_DIM,
        "num_head_channels": 64,
        "transformer_depth_output": [0, 0, 0, 2, 2, 2, 10, 10, 10],
        "use_temporal_attention": False,
        "use_temporal_resblock": False,
    })


class KolorsUNetModel(UNetModel):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        operations = kwargs.get("operations")
        self.encoder_hid_proj = operations.Linear(
            KOLORS_CONTEXT_IN,
            KOLORS_CONTEXT_DIM,
            bias=True,
            dtype=kwargs.get("dtype"),
            device=kwargs.get("device"),
        )

    def forward(self, x, timesteps=None, context=None, y=None, control=None, transformer_options={}, **kwargs):
        if context is not None and context.shape[-1] == KOLORS_CONTEXT_IN:
            context = self.encoder_hid_proj(context)
        return super().forward(x, timesteps, context, y, control, transformer_options, **kwargs)


class KolorsModel(comfy.model_base.SDXL):
    def __init__(self, model_config, model_type=comfy.model_base.ModelType.EPS, device=None):
        comfy.model_base.BaseModel.__init__(self, model_config, model_type, device=device, unet_model=KolorsUNetModel)
        self.embedder = comfy.model_base.Timestep(256)
        self.noise_augmentor = comfy.model_base.CLIPEmbeddingNoiseAugmentation(
            **{"noise_schedule_config": {"timesteps": 1000, "beta_schedule": "squaredcos_cap_v2"}, "timestep_dim": 1280}
        )


class KolorsConfig(comfy.supported_models.SDXL):
    unet_config = {
        "model_channels": 320,
        "use_linear_in_transformer": True,
        "transformer_depth": [0, 0, 2, 2, 10, 10],
        "context_dim": KOLORS_CONTEXT_DIM,
        "adm_in_channels": KOLORS_ADM,
        "use_temporal_attention": False,
    }
    sampling_settings = dict(KOLORS_SAMPLING)
    latent_format = comfy.latent_formats.SDXL
    memory_usage_factor = 0.8

    def model_type(self, state_dict, prefix=""):
        return comfy.model_base.ModelType.EPS

    def get_model(self, state_dict, prefix="", device=None):
        out = KolorsModel(self, model_type=self.model_type(state_dict, prefix), device=device)
        if self.inpaint_model():
            out.set_inpaint()
        return out


def _strip_checkpoint_prefix(sd):
    """Single-file checkpoints keep the UNet under model.diffusion_model."""
    prefix = comfy.model_detection.unet_prefix_from_state_dict(sd)
    stripped = comfy.utils.state_dict_prefix_replace(sd, {prefix: ""}, filter_keys=True)
    return stripped if len(stripped) > 0 else sd


def to_comfy_unet_state_dict(sd):
    """Returns (state_dict, in_channels) in ComfyUI's UNet layout.

    Accepts Kwai-Kolors' diffusers ``unet/`` weights as well as ldm-layout
    files (ComfyUI saves, single-file checkpoints)."""
    if "conv_in.weight" in sd:
        in_channels = sd["conv_in.weight"].shape[1]
        config = kolors_unet_config(in_channels)
        diffusers_keys = comfy.utils.unet_to_diffusers(config)
        out = {}
        for diffusers_key, comfy_key in diffusers_keys.items():
            if diffusers_key in sd:
                out[comfy_key] = sd.pop(diffusers_key)
        for key in ("weight", "bias"):
            name = "encoder_hid_proj.{}".format(key)
            if name in sd:
                out[name] = sd.pop(name)
        if len(sd) > 0:
            logging.info("Kolors: left over keys in diffusers UNet: %s", list(sd.keys())[:20])
        return out, in_channels

    sd = _strip_checkpoint_prefix(sd)
    if "input_blocks.0.0.weight" not in sd:
        raise RuntimeError("This file does not look like a Kolors UNet.")
    return sd, sd["input_blocks.0.0.weight"].shape[1]


def is_kolors_unet(sd):
    if "encoder_hid_proj.weight" in sd:
        return tuple(sd["encoder_hid_proj.weight"].shape) == (KOLORS_CONTEXT_DIM, KOLORS_CONTEXT_IN)
    for key, value in sd.items():
        if key.endswith("diffusion_model.encoder_hid_proj.weight"):
            return tuple(value.shape) == (KOLORS_CONTEXT_DIM, KOLORS_CONTEXT_IN)
    return False


def load_kolors_unet(path, model_options=None, disable_dynamic=False):
    model_options = model_options or {}
    sd, metadata = comfy.utils.load_torch_file(path, return_metadata=True)
    if not is_kolors_unet(sd):
        raise RuntimeError(
            "'{}' has no Kolors text projection (encoder_hid_proj 4096 -> 2048), so it is not a Kolors "
            "model. Plain SDXL checkpoints load with the normal checkpoint loader.".format(path)
        )
    sd, in_channels = to_comfy_unet_state_dict(sd)

    parameters = comfy.utils.calculate_parameters(sd)
    weight_dtype = comfy.utils.weight_dtype(sd)
    load_device = model_options.get("load_device", comfy.model_management.get_torch_device())
    offload_device = model_options.get("offload_device", comfy.model_management.unet_offload_device())

    model_config = KolorsConfig(kolors_unet_config(in_channels))
    requested = model_options.get("dtype", None)
    if requested is None:
        unet_dtype = comfy.model_management.unet_dtype(
            model_params=parameters,
            supported_dtypes=list(model_config.supported_inference_dtypes),
            weight_dtype=weight_dtype,
        )
    else:
        unet_dtype = requested
    manual_cast_dtype = comfy.model_management.unet_manual_cast(unet_dtype, load_device, model_config.supported_inference_dtypes)
    model_config.set_inference_dtype(unet_dtype, manual_cast_dtype)
    if model_options.get("fp8_optimizations", False):
        model_config.optimizations["fp8"] = True

    model = model_config.get_model(sd, "")
    patcher_cls = comfy.model_patcher.ModelPatcher if disable_dynamic else getattr(
        comfy.model_patcher, "CoreModelPatcher", comfy.model_patcher.ModelPatcher
    )
    patcher = patcher_cls(model, load_device=load_device, offload_device=offload_device)
    if not comfy.model_management.is_device_cpu(offload_device):
        model.to(offload_device)
    model.load_model_weights(sd, "", assign=patcher.is_dynamic() if hasattr(patcher, "is_dynamic") else False)
    patcher.cached_patcher_init = (load_kolors_unet, (path, model_options, disable_dynamic))
    return patcher
