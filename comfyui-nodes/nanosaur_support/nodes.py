import logging
import math

import comfy.conds
import comfy.latent_formats
import comfy.model_base
import comfy.model_detection
import comfy.model_management
import comfy.model_patcher
import comfy.samplers
import comfy.sd
import comfy.supported_models_base
import comfy.text_encoders.hunyuan_video
import comfy.utils
import folder_paths
import torch

from .model import NanoSaurTransformer2DModel
from .text_encoder import NanoSaurTokenizer, te as nanosaur_text_encoder
from .vae import NanoSaurVAE


def _preferred_first(options, preferred):
    ordered = []
    seen = set()

    for name in preferred:
        if name in options and name not in seen:
            ordered.append(name)
            seen.add(name)

    for name in options:
        if name not in seen:
            ordered.append(name)

    return ordered


def _count_blocks(state_dict_keys, prefix_string):
    count = 0
    while True:
        found = False
        for key in state_dict_keys:
            if key.startswith(prefix_string.format(count)):
                found = True
                break
        if not found:
            break
        count += 1
    return count


class NanoSaurLatentFormat(comfy.latent_formats.LatentFormat):
    latent_channels = 96
    spacial_downscale_ratio = 16

    def __init__(self):
        self.scale_factor = 2.3623
        self.shift_factor = -0.0179
        self.latent_rgb_factors = [
            #   R        G        B        (Ridge-regularised VAE encode regression)
            [-0.0055,  0.0430, -0.0243],
            [ 0.0023,  0.0072, -0.0022],
            [ 0.0014, -0.0096,  0.0256],
            [-0.0154,  0.0159,  0.0234],
            [ 0.0030, -0.0182,  0.0082],
            [-0.0027,  0.0277,  0.0054],
            [-0.0548,  0.0161,  0.0173],
            [ 0.0157,  0.0020,  0.0022],
            [-0.0297,  0.0012,  0.0006],
            [ 0.0115,  0.0036,  0.0167],
            [-0.0039, -0.0099,  0.0020],
            [-0.0040,  0.0100, -0.0262],
            [ 0.0057, -0.0473,  0.0416],
            [-0.0328, -0.0017,  0.0335],
            [-0.0083,  0.0249,  0.0247],
            [ 0.0215, -0.0505,  0.0251],
            [-0.0227,  0.0352,  0.0136],
            [-0.0035,  0.0004, -0.0287],
            [-0.0018, -0.0189, -0.0134],
            [ 0.0012,  0.0127, -0.0353],
            [ 0.0166,  0.0007, -0.0066],
            [-0.0022, -0.0204,  0.0174],
            [ 0.0082,  0.0065, -0.0046],
            [ 0.0212,  0.0084, -0.0049],
            [-0.0111, -0.0214,  0.0251],
            [-0.0078, -0.0246,  0.0271],
            [ 0.0369, -0.0081, -0.0219],
            [-0.0203,  0.0255, -0.0192],
            [ 0.0116, -0.0113,  0.0155],
            [ 0.0100, -0.0299, -0.0012],
            [-0.0194, -0.0280,  0.0065],
            [-0.0124, -0.0067, -0.0024],
            [-0.0296, -0.0015,  0.0247],
            [-0.0307,  0.0128, -0.0145],
            [-0.0049,  0.0084,  0.0021],
            [-0.0365,  0.0175,  0.0134],
            [-0.0223,  0.0316,  0.0153],
            [ 0.0066,  0.0052,  0.0045],
            [ 0.0100,  0.0485, -0.0378],
            [-0.0217,  0.0215,  0.0303],
            [ 0.0253, -0.0227, -0.0090],
            [ 0.0030,  0.0147, -0.0087],
            [ 0.0369, -0.0048, -0.0105],
            [ 0.0039,  0.0169,  0.0042],
            [ 0.0269, -0.0179, -0.0157],
            [ 0.0001, -0.0205,  0.0402],
            [-0.0121, -0.0192, -0.0129],
            [ 0.0151, -0.0035, -0.0182],
            [ 0.0304, -0.0273, -0.0204],
            [ 0.0178, -0.0086, -0.0037],
            [ 0.0247, -0.0037, -0.0117],
            [-0.0067,  0.0002, -0.0305],
            [ 0.0038,  0.0172, -0.0023],
            [-0.0558,  0.0575, -0.0128],
            [-0.0036, -0.0249, -0.0063],
            [-0.0045,  0.0136, -0.0517],
            [-0.0206,  0.0146, -0.0385],
            [ 0.0485, -0.0373,  0.0034],
            [-0.0055, -0.0057,  0.0029],
            [ 0.0044,  0.0248, -0.0442],
            [ 0.0071, -0.0156, -0.0059],
            [-0.0233,  0.0191, -0.0230],
            [-0.0026, -0.0177,  0.0033],
            [ 0.0238, -0.0040, -0.0260],
            [ 0.0198,  0.0025, -0.0114],
            [ 0.0080, -0.0135, -0.0047],
            [ 0.0069, -0.0090, -0.0299],
            [ 0.0155, -0.0258,  0.0008],
            [ 0.0549, -0.0427,  0.0032],
            [-0.0165,  0.0004, -0.0144],
            [-0.0164,  0.0346, -0.0383],
            [-0.0013, -0.0073, -0.0247],
            [-0.0131, -0.0157,  0.0189],
            [-0.0051, -0.0261,  0.0372],
            [-0.0094, -0.0248, -0.0207],
            [ 0.0039, -0.0215, -0.0073],
            [ 0.0065, -0.0233,  0.0302],
            [-0.0441,  0.0069,  0.0085],
            [-0.0094,  0.0204, -0.0199],
            [ 0.0052, -0.0028, -0.0203],
            [-0.0048, -0.0012,  0.0085],
            [-0.0259, -0.0115,  0.0114],
            [-0.0286, -0.0074,  0.0140],
            [ 0.0070,  0.0030, -0.0170],
            [ 0.0529, -0.0207,  0.0102],
            [-0.0077,  0.0044, -0.0183],
            [-0.0062, -0.0056,  0.0098],
            [-0.0236, -0.0243,  0.0061],
            [ 0.0264, -0.0127,  0.0314],
            [-0.0106,  0.0002,  0.0144],
            [ 0.0397, -0.0398,  0.0114],
            [-0.0059,  0.0149,  0.0145],
            [ 0.0191, -0.0207,  0.0620],
            [ 0.0227,  0.0181, -0.0192],
            [-0.0107,  0.0370,  0.0027],
            [ 0.0177, -0.0244,  0.0153],
        ]
        self.latent_rgb_factors_bias = [0.4882, 0.6410, 0.3384]

    def process_in(self, latent):
        return (latent - self.shift_factor) / self.scale_factor

    def process_out(self, latent):
        return latent * self.scale_factor + self.shift_factor


class NanoSaurModelConfig(comfy.supported_models_base.BASE):
    unet_extra_config = {}
    sampling_settings = {
        "multiplier": 1.0,
        "shift": 4.0,
    }
    latent_format = NanoSaurLatentFormat
    supported_inference_dtypes = [torch.bfloat16, torch.float32]
    preferred_inference_dtype = torch.bfloat16
    memory_usage_factor = 0.6

    def model_type(self, state_dict, prefix=""):
        return comfy.model_base.ModelType.FLOW

    def get_model(self, state_dict, prefix="", device=None):
        return NanoSaurModel(self, device=device)


class NanoSaurModel(comfy.model_base.BaseModel):
    def __init__(self, model_config, device=None):
        super().__init__(
            model_config,
            comfy.model_base.ModelType.FLOW,
            device=device,
            unet_model=NanoSaurTransformer2DModel,
        )

    def extra_conds(self, **kwargs):
        out = super().extra_conds(**kwargs)
        cross_attn = kwargs.get("cross_attn", None)
        if cross_attn is not None:
            out["c_crossattn"] = comfy.conds.CONDRegular(cross_attn)

        token_weights = kwargs.get("token_weights", None)
        if token_weights is not None:
            out["token_weights"] = comfy.conds.CONDRegular(token_weights)

        if "uncond" in kwargs:
            out["uncond"] = comfy.conds.CONDConstant(bool(kwargs["uncond"]))
        return out


class NanoSaurVAEWrapper(comfy.sd.VAE):
    def __init__(self, sd=None, device=None, dtype=None, metadata=None):
        if sd is None:
            raise RuntimeError("NanoSaur VAE weights are required.")

        if comfy.model_management.is_amd():
            vae_kl_mem_ratio = 2.73
        else:
            vae_kl_mem_ratio = 1.0

        self.memory_used_encode = lambda shape, target_dtype: (1767 * shape[2] * shape[3]) * comfy.model_management.dtype_size(target_dtype) * vae_kl_mem_ratio
        self.memory_used_decode = lambda shape, target_dtype: (2178 * shape[2] * shape[3] * 64) * comfy.model_management.dtype_size(target_dtype) * vae_kl_mem_ratio
        self.downscale_ratio = 8
        self.upscale_ratio = 8
        self.latent_channels = 4
        self.latent_dim = 2
        self.output_channels = 3
        self.pad_channel_value = None
        self.process_input = lambda image: image * 2.0 - 1.0
        self.process_output = lambda image: image.add_(1.0).div_(2.0).clamp_(0.0, 1.0)
        self.working_dtypes = [torch.bfloat16, torch.float32]
        self.disable_offload = False
        self.not_video = False
        self.size = None
        self.downscale_index_formula = None
        self.upscale_index_formula = None
        self.extra_1d_channel = None
        self.crop_input = True
        self.audio_sample_rate = 44100

        if "semantic_encoder.in_proj.weight" not in sd or "pixel_decoder.decoder.conv_in.weight" not in sd:
            self.first_stage_model = None
            raise RuntimeError("Provided VAE weights do not look like NanoSaur.")

        self.first_stage_model = NanoSaurVAE(latent_dim=sd["semantic_encoder.in_proj.weight"].shape[0])
        self.memory_used_encode = lambda shape, target_dtype: (200 * shape[2] * shape[3]) * comfy.model_management.dtype_size(target_dtype)
        self.memory_used_decode = lambda shape, target_dtype: (400 * shape[2] * shape[3] * 16 * 16) * comfy.model_management.dtype_size(target_dtype)
        self.downscale_ratio = 16
        self.upscale_ratio = 16
        self.latent_channels = sd["semantic_encoder.in_proj.weight"].shape[0]
        self.latent_dim = 2
        self.output_channels = 3
        self.working_dtypes = [torch.bfloat16, torch.float32]
        self.disable_offload = True
        self.first_stage_model = self.first_stage_model.eval()

        if device is None:
            device = comfy.model_management.vae_device()
        self.device = device
        offload_device = comfy.model_management.vae_offload_device()
        if dtype is None:
            dtype = comfy.model_management.vae_dtype(self.device, self.working_dtypes)
        self.vae_dtype = dtype
        self.first_stage_model.to(self.vae_dtype)
        comfy.model_management.archive_model_dtypes(self.first_stage_model)
        self.output_device = comfy.model_management.intermediate_device()

        model_patcher_cls = comfy.model_patcher.ModelPatcher if self.disable_offload else comfy.model_patcher.CoreModelPatcher
        self.patcher = model_patcher_cls(self.first_stage_model, load_device=self.device, offload_device=offload_device)

        missing, unexpected = self.first_stage_model.load_state_dict(sd, strict=False, assign=self.patcher.is_dynamic())
        if len(missing) > 0:
            logging.warning("Missing NanoSaur VAE keys %s", missing)
        if len(unexpected) > 0:
            logging.debug("Leftover NanoSaur VAE keys %s", unexpected)

        logging.info(
            "NanoSaur VAE load device: %s, offload device: %s, dtype: %s",
            self.device,
            offload_device,
            self.vae_dtype,
        )
        self.model_size()


def _infer_nanosaur_unet_config(state_dict):
    state_dict_keys = list(state_dict.keys())
    in_channels = state_dict["dec_net.final_layer.linear.weight"].shape[0]
    patch_tokens = state_dict["s_embedder.proj.weight"].shape[1]
    return {
        "image_model": "nanosaur",
        "in_channels": in_channels,
        "patch_size": round(math.sqrt(patch_tokens / in_channels)),
        "hidden_size": state_dict["s_embedder.proj.weight"].shape[0],
        "decoder_hidden_size": state_dict["dec_net.input_proj.weight"].shape[0],
        "num_encoder_blocks": _count_blocks(state_dict_keys, "blocks.{}."),
        "num_decoder_blocks": _count_blocks(state_dict_keys, "dec_net.res_blocks.{}."),
        "num_text_blocks": _count_blocks(state_dict_keys, "text_refine_blocks.{}."),
        "num_groups": state_dict["s_embedder.proj.weight"].shape[0] // state_dict["blocks.0.attn.q_norm.weight"].shape[0],
        "txt_embed_dim": state_dict["y_embedder.proj.weight"].shape[1],
    }


def _load_nanosaur_model(unet_name, model_options=None, disable_dynamic=False):
    if model_options is None:
        model_options = {}

    state_dict, metadata = comfy.utils.load_torch_file(
        folder_paths.get_full_path_or_raise("diffusion_models", unet_name),
        return_metadata=True,
    )

    diffusion_model_prefix = comfy.model_detection.unet_prefix_from_state_dict(state_dict)
    stripped_state_dict = comfy.utils.state_dict_prefix_replace(state_dict, {diffusion_model_prefix: ""}, filter_keys=True)
    if len(stripped_state_dict) > 0:
        state_dict = stripped_state_dict

    if model_options.get("custom_operations", None) is None:
        state_dict, metadata = comfy.utils.convert_old_quants(state_dict, "", metadata=metadata)

    parameters = comfy.utils.calculate_parameters(state_dict)
    weight_dtype = comfy.utils.weight_dtype(state_dict)
    load_device = comfy.model_management.get_torch_device()
    offload_device = comfy.model_management.unet_offload_device()

    model_config = NanoSaurModelConfig(_infer_nanosaur_unet_config(state_dict))
    if model_config.quant_config is not None:
        weight_dtype = None

    requested_dtype = model_options.get("dtype", None)
    if requested_dtype is None:
        preferred_dtype = getattr(model_config, "preferred_inference_dtype", None)
        if preferred_dtype in model_config.supported_inference_dtypes:
            unet_dtype = preferred_dtype
        else:
            unet_dtype = comfy.model_management.unet_dtype(
                model_params=parameters,
                supported_dtypes=list(model_config.supported_inference_dtypes),
                weight_dtype=weight_dtype,
            )
    else:
        unet_dtype = requested_dtype

    if model_config.quant_config is not None:
        manual_cast_dtype = comfy.model_management.unet_manual_cast(None, load_device, model_config.supported_inference_dtypes)
    else:
        manual_cast_dtype = comfy.model_management.unet_manual_cast(unet_dtype, load_device, model_config.supported_inference_dtypes)
    model_config.set_inference_dtype(unet_dtype, manual_cast_dtype)

    if model_options.get("custom_operations", None) is not None:
        model_config.custom_operations = model_options["custom_operations"]

    if model_options.get("fp8_optimizations", False):
        model_config.optimizations["fp8"] = True

    model = model_config.get_model(state_dict, "")
    model_patcher_cls = comfy.model_patcher.ModelPatcher if disable_dynamic else comfy.model_patcher.CoreModelPatcher
    model_patcher = model_patcher_cls(model, load_device=load_device, offload_device=offload_device)
    if not comfy.model_management.is_device_cpu(offload_device):
        model.to(offload_device)
    model.load_model_weights(state_dict, "", assign=model_patcher.is_dynamic())
    model_patcher.cached_patcher_init = (_load_nanosaur_model, (unet_name, model_options, disable_dynamic))
    return model_patcher


def _load_nanosaur_clip(text_encoder_name, clip_device="default", disable_dynamic=False):
    clip_state_dict, metadata = comfy.utils.load_torch_file(
        folder_paths.get_full_path_or_raise("text_encoders", text_encoder_name),
        safe_load=True,
        return_metadata=True,
    )
    del metadata

    if "lm_head.weight" in clip_state_dict:
        clip_state_dict["model.lm_head.weight"] = clip_state_dict.pop("lm_head.weight")

    clip_model_options = {
        "dtype": torch.bfloat16,
        "keep_loaded": True,
    }
    if clip_device == "cpu":
        clip_model_options["load_device"] = torch.device("cpu")
        clip_model_options["offload_device"] = torch.device("cpu")

    class ClipTarget:
        params = {}
        clip = nanosaur_text_encoder(
            **comfy.text_encoders.hunyuan_video.llama_detect(clip_state_dict)
        )
        tokenizer = NanoSaurTokenizer

    clip = comfy.sd.CLIP(
        ClipTarget,
        embedding_directory=folder_paths.get_folder_paths("embeddings"),
        parameters=comfy.utils.calculate_parameters(clip_state_dict),
        tokenizer_data={"spiece_model": clip_state_dict.get("spiece_model", None)},
        state_dict=[clip_state_dict],
        model_options=clip_model_options,
        disable_dynamic=disable_dynamic,
    )
    clip.patcher.cached_patcher_init = (_load_nanosaur_clip, (text_encoder_name, clip_device, disable_dynamic))
    return clip


def _load_nanosaur_vae(vae_name):
    vae_state_dict, metadata = comfy.utils.load_torch_file(
        folder_paths.get_full_path_or_raise("vae", vae_name),
        return_metadata=True,
    )
    return NanoSaurVAEWrapper(sd=vae_state_dict, metadata=metadata)


def _apply_nanosaur_sampler(model, uncond_crossover_percent):
    split_percent = max(0.0, min(1.0, float(uncond_crossover_percent)))
    split_sigma = float(model.get_model_object("model_sampling").percent_to_sigma(split_percent))
    state = {
        "last_sigma": None,
        "step_index": -1,
    }

    def calc_cond_batch_function(args):
        conds = list(args["conds"])
        x = args["input"]
        sigma = args["sigma"]
        model_options = args["model_options"]

        sigma_value = float(sigma[0].item())
        if state["last_sigma"] is None or not math.isclose(
            sigma_value,
            state["last_sigma"],
            rel_tol=0.0,
            abs_tol=1e-12,
        ):
            state["last_sigma"] = sigma_value
            state["step_index"] += 1

        use_sparse_uncond = sigma_value >= split_sigma and (state["step_index"] % 2 == 1)

        uncond = conds[1]
        if uncond is not None:
            updated_uncond = []
            for condition in uncond:
                updated_condition = {**condition, "uncond": use_sparse_uncond}
                model_conds = updated_condition.get("model_conds", {}).copy()
                model_conds["uncond"] = comfy.conds.CONDConstant(bool(use_sparse_uncond))
                updated_condition["model_conds"] = model_conds
                updated_uncond.append(updated_condition)
            conds[1] = updated_uncond

        return comfy.samplers.calc_cond_batch(args["model"], conds, x, sigma, model_options)

    patched_model = model.clone()
    patched_model.set_model_sampler_calc_cond_batch_function(calc_cond_batch_function)
    return patched_model


class NanoSaurLoader:
    @classmethod
    def INPUT_TYPES(cls):
        return {
            "required": {
                "unet_name": (
                    _preferred_first(
                        folder_paths.get_filename_list("diffusion_models"),
                        ["nanosaur_diffusion_model.safetensors"],
                    ),
                ),
                "text_encoder_name": (
                    _preferred_first(
                        folder_paths.get_filename_list("text_encoders"),
                        [
                            "nanosaur_text_encoder.safetensors",
                            "gemma3_270m_unsloth_nanosaur.safetensors",
                        ],
                    ),
                ),
                "vae_name": (
                    _preferred_first(
                        folder_paths.get_filename_list("vae"),
                        ["nanosaur_vae_decoder.safetensors"],
                    ),
                ),
                "uncond_crossover_percent": (
                    "FLOAT",
                    {
                        "default": 1.0,
                        "min": 0.0,
                        "max": 1.0,
                        "step": 0.001,
                        "tooltip": "NanoSaur sampler patch",
                    },
                ),
            },
            "optional": {
                "weight_dtype": (
                    ["default"],
                    {"advanced": True},
                ),
                "clip_device": (
                    ["default", "cpu"],
                    {"advanced": True},
                ),
            },
        }

    RETURN_TYPES = ("MODEL", "CLIP", "VAE")
    FUNCTION = "load_nanosaur"
    CATEGORY = "loaders"
    DESCRIPTION = "Loads NanoSaur from a custom-node package so core ComfyUI can be reset without losing model support."

    def load_nanosaur(
        self,
        unet_name,
        text_encoder_name,
        vae_name,
        uncond_crossover_percent,
        weight_dtype="default",
        clip_device="default",
    ):
        model_options = {}
        if weight_dtype == "fp8_e4m3fn":
            model_options["dtype"] = torch.float8_e4m3fn
        elif weight_dtype == "fp8_e4m3fn_fast":
            model_options["dtype"] = torch.float8_e4m3fn
            model_options["fp8_optimizations"] = True
        elif weight_dtype == "fp8_e5m2":
            model_options["dtype"] = torch.float8_e5m2

        model = _load_nanosaur_model(unet_name, model_options=model_options)
        model = _apply_nanosaur_sampler(model, uncond_crossover_percent)
        clip = _load_nanosaur_clip(text_encoder_name, clip_device=clip_device)
        vae = _load_nanosaur_vae(vae_name)
        return (model, clip, vae)


NODE_CLASS_MAPPINGS = {
    "NanoSaurLoader": NanoSaurLoader,
}


NODE_DISPLAY_NAME_MAPPINGS = {
    "NanoSaurLoader": "Load NanoSaur",
}
