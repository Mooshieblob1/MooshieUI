import logging
import torch
import torch.nn.functional as F
import comfy.latent_formats
import comfy.model_management
import comfy.sd
import comfy.supported_models
import comfy.model_detection
import comfy.ldm.modules.diffusionmodules.openaimodel
import nodes
import latent_preview


_PATCHED = False

# --- PATCH SDXL MODEL FORWARD PASS ---
# This handles the interface between Packed Latents (128ch) and the SDXL Model (32ch).
# It wraps the execution to ensure shapes match on both ends.

def _wrap_latent_preview():
    if getattr(latent_preview, "_sdxl_flux2_preview_wrapped", False):
        return

    # Find the previewer class that uses latent_rgb_factors
    Latent2RGBPreviewer = getattr(latent_preview, "Latent2RGBPreviewer", None)
    if Latent2RGBPreviewer is None:
        return

    original_decode = Latent2RGBPreviewer.decode_latent_to_preview

    def decode_latent_to_preview(self, x0):
        # If latents are packed (128ch) but the format is 32ch, unpack for preview
        try:
            latent_channels = getattr(self.latent_format, "latent_channels", None)
            if latent_channels == 32 and x0.shape[1] == 128:
                x0 = F.pixel_shuffle(x0, 2)  # (N,128,H,W)->(N,32,H*2,W*2)
        except Exception:
            pass
        return original_decode(self, x0)

    Latent2RGBPreviewer.decode_latent_to_preview = decode_latent_to_preview
    latent_preview._sdxl_flux2_preview_wrapped = True

def _patch_unetmodel_forward():
    if hasattr(comfy.ldm.modules.diffusionmodules.openaimodel.UNetModel, "_sdxl_flux2_patched"):
        return

    original_forward = comfy.ldm.modules.diffusionmodules.openaimodel.UNetModel.forward

    def patched_forward(self, x, timesteps=None, context=None, y=None, control=None, transformer_options={}, **kwargs):
        # Check if we have a packed input (128ch) but the model expects unpacked (32ch)
        is_packed = False
        if x.shape[1] == 128:
            try:
                # Check the weight shape of the first layer (input_blocks[0][0])
                first_layer = self.input_blocks[0][0]
                if hasattr(first_layer, "weight") and first_layer.weight.shape[1] == 32:
                    is_packed = True
            except Exception:
                pass

        # 1. PRE-PROCESS: Unpack Input (128ch -> 32ch)
        if is_packed:
            # (N, 128, H, W) -> (N, 32, H*2, W*2)
            x = F.pixel_shuffle(x, 2)

        # 2. RUN ORIGINAL MODEL
        h = original_forward(self, x, timesteps, context, y, control, transformer_options, **kwargs)

        # 3. POST-PROCESS: Repack Output (32ch -> 128ch)
        if is_packed:
            # (N, 32, H*2, W*2) -> (N, 128, H, W)
            h = F.pixel_unshuffle(h, 2)

        return h

    comfy.ldm.modules.diffusionmodules.openaimodel.UNetModel.forward = patched_forward
    comfy.ldm.modules.diffusionmodules.openaimodel.UNetModel._sdxl_flux2_patched = True

# -----------------------------

class PackedLatentVAE:
    def __init__(self, inner, packed_channels, spatial_factor=2):
        self._inner = inner
        self.packed_latent_channels = packed_channels
        self.packed_latent_spatial_factor = spatial_factor
        self.unpack_on_encode = False

    def set_packed_latents(self, packed_channels, spatial_factor=2, unpack_on_encode=None):
        self.packed_latent_channels = packed_channels
        self.packed_latent_spatial_factor = spatial_factor
        if unpack_on_encode is not None:
            self.unpack_on_encode = unpack_on_encode

    def _to_vae_latent(self, latent):
        packed_channels = self.packed_latent_channels
        sf = self.packed_latent_spatial_factor
        target_channels = getattr(self._inner, "latent_channels", latent.shape[1])
        if packed_channels is None and latent.shape[1] * (sf ** 2) == target_channels:
            packed_channels = latent.shape[1]
        if packed_channels is not None and latent.shape[1] == packed_channels:
            if packed_channels * (sf ** 2) == target_channels and latent.ndim >= 4:
                h = latent.shape[-2]
                w = latent.shape[-1]
                if h % sf != 0 or w % sf != 0:
                    pad_h = (sf - (h % sf)) % sf
                    pad_w = (sf - (w % sf)) % sf
                    latent = torch.nn.functional.pad(latent, (0, pad_w, 0, pad_h))
                    h = latent.shape[-2]
                    w = latent.shape[-1]
                latent = latent.reshape(latent.shape[0], packed_channels, h // sf, sf, w // sf, sf)
                latent = latent.permute(0, 1, 3, 5, 2, 4).reshape(latent.shape[0], target_channels, h // sf, w // sf)
        return latent

    def _from_vae_latent(self, latent):
        if not self.unpack_on_encode:
            return latent

        packed_channels = self.packed_latent_channels
        sf = self.packed_latent_spatial_factor
        target_channels = getattr(self._inner, "latent_channels", latent.shape[1])
        if packed_channels is None and target_channels == 128 and latent.shape[1] == target_channels:
            packed_channels = 32
        if packed_channels is not None and latent.shape[1] == target_channels:
            if packed_channels * (sf ** 2) == target_channels and latent.ndim >= 4:
                h = latent.shape[-2]
                w = latent.shape[-1]
                latent = latent.reshape(latent.shape[0], packed_channels, sf, sf, h, w)
                latent = latent.permute(0, 1, 4, 2, 5, 3).reshape(latent.shape[0], packed_channels, h * sf, w * sf)
        return latent

    def decode(self, samples_in, vae_options=None):
        if vae_options is None:
            vae_options = {}
        samples_in = self._to_vae_latent(samples_in)
        return self._inner.decode(samples_in, vae_options=vae_options)

    def decode_tiled(self, samples, **kwargs):
        samples = self._to_vae_latent(samples)
        return self._inner.decode_tiled(samples, **kwargs)

    def decode_tiled_(self, samples, *args, **kwargs):
        samples = self._to_vae_latent(samples)
        return self._inner.decode_tiled_(samples, *args, **kwargs)

    def decode_tiled_1d(self, samples, *args, **kwargs):
        samples = self._to_vae_latent(samples)
        return self._inner.decode_tiled_1d(samples, *args, **kwargs)

    def decode_tiled_3d(self, samples, *args, **kwargs):
        samples = self._to_vae_latent(samples)
        return self._inner.decode_tiled_3d(samples, *args, **kwargs)

    def encode(self, pixel_samples):
        return self._from_vae_latent(self._inner.encode(pixel_samples))

    def encode_tiled(self, pixel_samples, **kwargs):
        return self._from_vae_latent(self._inner.encode_tiled(pixel_samples, **kwargs))

    def encode_tiled_(self, pixel_samples, *args, **kwargs):
        return self._from_vae_latent(self._inner.encode_tiled_(pixel_samples, *args, **kwargs))

    def encode_tiled_1d(self, pixel_samples, *args, **kwargs):
        return self._from_vae_latent(self._inner.encode_tiled_1d(pixel_samples, *args, **kwargs))

    def encode_tiled_3d(self, pixel_samples, *args, **kwargs):
        return self._from_vae_latent(self._inner.encode_tiled_3d(pixel_samples, *args, **kwargs))

    def __getattr__(self, name):
        return getattr(self._inner, name)


def _packed_latent_settings(vae):
    target_channels = getattr(vae, "latent_channels", None)
    if target_channels is None:
        return None

    first_stage = getattr(vae, "first_stage_model", None)
    bn = getattr(first_stage, "bn", None)
    ps = getattr(first_stage, "ps", None)
    if bn is not None and ps is not None and len(ps) == 2 and ps[0] == ps[1]:
        sf = ps[0]
        if target_channels % (sf ** 2) == 0:
            packed_channels = target_channels // (sf ** 2)
            return packed_channels, sf

    latent_dim = getattr(vae, "latent_dim", 2)
    downscale = getattr(vae, "downscale_ratio", None)
    upscale = getattr(vae, "upscale_ratio", None)
    if (
        target_channels == 128
        and latent_dim == 2
        and downscale in (8, 16)
        and upscale in (8, 16)
    ):
        return 32, 2

    return None


def _ensure_latent_format():
    if hasattr(comfy.latent_formats, "SDXL_Flux2"):
        return comfy.latent_formats.SDXL_Flux2
    if not hasattr(comfy.latent_formats, "Flux2"):
        raise RuntimeError("Flux2 latent format is not available in this ComfyUI build.")

    class SDXL_Flux2(comfy.latent_formats.Flux2):
        latent_channels = 32

        def __init__(self):
            super().__init__()
            self.latent_rgb_factors_reshape = None

            # Unpack packed latents (128ch) to 32ch *only for preview*
            def _maybe_unpack_preview(x0, sf=2, packed_channels=128):
                if not torch.is_tensor(x0):
                    return x0
                if x0.ndim == 5:
                    # (B, C, T, H, W) -> (B*T, C, H, W) -> pixel_shuffle -> restore
                    b, c, t, h, w = x0.shape
                    if c == packed_channels:
                        x0_bt = x0.permute(0, 2, 1, 3, 4).reshape(b * t, c, h, w)
                        x0_bt = torch.nn.functional.pixel_shuffle(x0_bt, sf)
                        _, c2, h2, w2 = x0_bt.shape
                        return x0_bt.reshape(b, t, c2, h2, w2).permute(0, 2, 1, 3, 4)
                    return x0
                if x0.ndim == 4 and x0.shape[1] == packed_channels:
                    return torch.nn.functional.pixel_shuffle(x0, sf)
                return x0

            self.latent_rgb_factors_reshape = _maybe_unpack_preview


    comfy.latent_formats.SDXL_Flux2 = SDXL_Flux2
    return SDXL_Flux2


def _ensure_model_class():
    if hasattr(comfy.supported_models, "SDXL_flux2"):
        return comfy.supported_models.SDXL_flux2

    latent_format_cls = _ensure_latent_format()

    class SDXL_flux2(comfy.supported_models.SDXL):
        unet_config = dict(comfy.supported_models.SDXL.unet_config, in_channels=32, out_channels=32)
        latent_format = latent_format_cls
        vae_key_prefix = ["vae.", "first_stage_model."]
        packed_vae_latent_channels = 32
        packed_vae_spatial_factor = 2

        def inpaint_model(self):
            return False

    comfy.supported_models.SDXL_flux2 = SDXL_flux2
    return SDXL_flux2


def _ensure_model_order(model_class):
    models = getattr(comfy.supported_models, "models", None)
    if models is None:
        return

    sdxl_class = getattr(comfy.supported_models, "SDXL", None)
    if model_class not in models:
        if sdxl_class in models:
            models.insert(models.index(sdxl_class), model_class)
        else:
            models.append(model_class)
        return

    if sdxl_class in models and models.index(model_class) > models.index(sdxl_class):
        models.remove(model_class)
        models.insert(models.index(sdxl_class), model_class)


def _wrap_load_state_dict_guess_config():
    if getattr(comfy.sd.load_state_dict_guess_config, "_sdxl_flux2_wrapped", False):
        return

    original = comfy.sd.load_state_dict_guess_config

    def wrapped(*args, **kwargs):
        out = original(*args, **kwargs)
        if out is None:
            return out

        model, clip, vae, clipvision = out
        if model is None or vae is None:
            return out

        model_config = getattr(getattr(model, "model", None), "model_config", None)
        packed_channels = getattr(model_config, "packed_vae_latent_channels", None)
        if packed_channels:
            spatial_factor = getattr(model_config, "packed_vae_spatial_factor", 2)
            if hasattr(vae, "_to_vae_latent") and hasattr(vae, "set_packed_latents"):
                vae.set_packed_latents(packed_channels, spatial_factor)
                if hasattr(vae, "unpack_on_encode"):
                    # NOTE: Keep unpack_on_encode = False for Flux Klein Edit compatibility
                    vae.unpack_on_encode = False
            else:
                vae = PackedLatentVAE(vae, packed_channels, spatial_factor)
                vae.unpack_on_encode = False
        return (model, clip, vae, clipvision)

    wrapped._sdxl_flux2_wrapped = True
    comfy.sd.load_state_dict_guess_config = wrapped


def _wrap_vae_loader():
    loader_class = getattr(nodes, "VAELoader", None)
    if loader_class is None:
        return
    if getattr(loader_class.load_vae, "_sdxl_flux2_wrapped", False):
        return

    original = loader_class.load_vae

    def wrapped(self, *args, **kwargs):
        out = original(self, *args, **kwargs)
        if not out:
            return out

        (vae,) = out
        settings = _packed_latent_settings(vae)
        if settings is not None:
            packed_channels, spatial_factor = settings
            if hasattr(vae, "_to_vae_latent") and hasattr(vae, "set_packed_latents"):
                vae.set_packed_latents(packed_channels, spatial_factor)
            else:
                vae = PackedLatentVAE(vae, packed_channels, spatial_factor)
        return (vae,)

    wrapped._sdxl_flux2_wrapped = True
    loader_class.load_vae = wrapped


def _wrap_unet_config_from_diffusers_unet():
    target = comfy.model_detection.unet_config_from_diffusers_unet
    if getattr(target, "_sdxl_flux2_wrapped", False):
        return

    original = target

    def wrapped(state_dict, dtype=None):
        unet_config = original(state_dict, dtype)
        if unet_config is not None:
            return unet_config

        conv_in = state_dict.get("conv_in.weight", None)
        if conv_in is None or conv_in.ndim != 4:
            return None
        if conv_in.shape[0] != 320 or conv_in.shape[1] != 32:
            return None

        adm_key = None
        if "add_embedding.linear_1.weight" in state_dict:
            adm_key = "add_embedding.linear_1.weight"
        elif "class_embedding.linear_1.weight" in state_dict:
            adm_key = "class_embedding.linear_1.weight"
        if adm_key is None or state_dict[adm_key].shape[1] != 2816:
            return None

        context_key = "down_blocks.2.attentions.1.transformer_blocks.0.attn2.to_k.weight"
        if context_key in state_dict and state_dict[context_key].shape[1] != 2048:
            return None

        sdxl_flux2 = {
            "use_checkpoint": False,
            "image_size": 32,
            "out_channels": 32,
            "use_spatial_transformer": True,
            "legacy": False,
            "num_classes": "sequential",
            "adm_in_channels": 2816,
            "dtype": dtype,
            "in_channels": 32,
            "model_channels": 320,
            "num_res_blocks": [2, 2, 2],
            "transformer_depth": [0, 0, 2, 2, 10, 10],
            "channel_mult": [1, 2, 4],
            "transformer_depth_middle": 10,
            "use_linear_in_transformer": True,
            "context_dim": 2048,
            "num_head_channels": 64,
            "transformer_depth_output": [0, 0, 0, 2, 2, 2, 10, 10, 10],
            "use_temporal_attention": False,
            "use_temporal_resblock": False,
        }
        cond_proj_key = "time_embedding.cond_proj.weight"
        if cond_proj_key in state_dict:
            sdxl_flux2["timestep_cond_dim"] = state_dict[cond_proj_key].shape[1]
        return comfy.model_detection.convert_config(sdxl_flux2)

    wrapped._sdxl_flux2_wrapped = True
    comfy.model_detection.unet_config_from_diffusers_unet = wrapped


def _pack_latents_for_model(latent, model):
    if latent is None or "samples" not in latent:
        return latent

    try:
        model_config = model.get_model_object("model_config")
    except Exception:
        model_config = getattr(getattr(model, "model", None), "model_config", None)

    packed_channels = getattr(model_config, "packed_vae_latent_channels", None)
    if not packed_channels:
        return latent

    sf = getattr(model_config, "packed_vae_spatial_factor", 2)
    if not isinstance(sf, int) or sf <= 0:
        return latent

    samples = latent["samples"]
    if not torch.is_tensor(samples) or samples.is_nested or samples.ndim < 4:
        return latent

    target_channels = packed_channels * (sf ** 2)
    if samples.shape[1] != target_channels:
        return latent

    h = samples.shape[-2]
    w = samples.shape[-1]
    samples = samples.reshape(samples.shape[0], packed_channels, sf, sf, h, w)
    samples = samples.permute(0, 1, 4, 2, 5, 3).reshape(samples.shape[0], packed_channels, h * sf, w * sf)

    out = latent.copy()
    out["samples"] = samples

    noise_mask = latent.get("noise_mask", None)
    if torch.is_tensor(noise_mask) and noise_mask.ndim >= 4:
        out["noise_mask"] = torch.nn.functional.interpolate(noise_mask, scale_factor=sf, mode="nearest")

    return out

def _get_model_config(model):
    try:
        return model.get_model_object("model_config")
    except Exception:
        return getattr(getattr(model, "model", None), "model_config", None)

def _is_sdxl_flux2_model(model):
    model_config = _get_model_config(model)
    return model_config is not None and model_config.__class__.__name__ == "SDXL_flux2"


def _wrap_common_ksampler():
    target = nodes.common_ksampler
    if getattr(target, "_sdxl_flux2_wrapped", False):
        return

    original = target

    def wrapped(model, seed, steps, cfg, sampler_name, scheduler, positive, negative, latent, denoise=1.0, disable_noise=False, start_step=None, last_step=None, force_full_denoise=False):
        # Explicit route: SDXL Flux2 uses the unpacked latent path.
        # Skip latent packing and strip legacy spatial ratio hints that can
        # cause an unintended 0.5 resize in fix_empty_latent_channels.
        if _is_sdxl_flux2_model(model):
            latent = latent.copy()
            latent.pop("downscale_ratio_spacial", None)
        else:
            latent = _pack_latents_for_model(latent, model)
        return original(
            model,
            seed,
            steps,
            cfg,
            sampler_name,
            scheduler,
            positive,
            negative,
            latent,
            denoise=denoise,
            disable_noise=disable_noise,
            start_step=start_step,
            last_step=last_step,
            force_full_denoise=force_full_denoise,
        )

    wrapped._sdxl_flux2_wrapped = True
    nodes.common_ksampler = wrapped


def _apply_patches():
    global _PATCHED
    if _PATCHED:
        return

    _PATCHED = True
    try:
        model_class = _ensure_model_class()
        _ensure_model_order(model_class)
        _patch_unetmodel_forward() # Updated Fix for SDXL Input/Output matching
        _wrap_load_state_dict_guess_config()
        _wrap_vae_loader()
        _wrap_unet_config_from_diffusers_unet()
        _wrap_common_ksampler()
        _wrap_latent_preview()
    except Exception:
        logging.exception("SDXL Flux2 support patch failed")


_apply_patches()


class EmptySDXLFlux2LatentImage:
    @classmethod
    def INPUT_TYPES(cls):
        return {
            "required": {
                "width": ("INT", {"default": 1024, "min": 16, "max": nodes.MAX_RESOLUTION, "step": 16}),
                "height": ("INT", {"default": 1024, "min": 16, "max": nodes.MAX_RESOLUTION, "step": 16}),
                "batch_size": ("INT", {"default": 1, "min": 1, "max": 4096}),
            }
        }

    RETURN_TYPES = ("LATENT",)
    FUNCTION = "generate"
    CATEGORY = "latent"

    def generate(self, width, height, batch_size=1):
        latent = torch.zeros(
            [batch_size, 32, height // 8, width // 8],
            device=comfy.model_management.intermediate_device(),
        )
        return ({"samples": latent},)


NODE_CLASS_MAPPINGS = {
    "EmptySDXLFlux2LatentImage": EmptySDXLFlux2LatentImage,
}

NODE_DISPLAY_NAME_MAPPINGS = {
    "EmptySDXLFlux2LatentImage": "Empty SDXL Flux2 Latent",
}
