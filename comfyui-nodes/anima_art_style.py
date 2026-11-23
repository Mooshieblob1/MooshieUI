"""Anima art style reference: a training-free art style from a picture, for Anima (Cosmos Predict2 MiniTrainDIT).

Vendored from BetaDoggo/SwarmUI-BrokeAnimaStyleReference
(https://github.com/BetaDoggo/SwarmUI-BrokeAnimaStyleReference), file
comfy_node/broke_anima_style_reference/__init__.py at revision
b33ada652c6bbaf7c190c7d06f9825d1b257b279. Apache License 2.0, see
anima_art_style.LICENSE.

Changes from the original (Apache-2.0 section 4(b)): the node class is renamed from
BrokeAnimaStyleReference to MooshieAnimaArtStyle and registered through the
mooshie-nodes package, its log prefix changes to match, and this header replaces
the first line of the original docstring. The method itself is unchanged.

Each sampling step, the picture is noised to that step's sigma (fixed noise) and run through the unmodified model
with a neutral caption. After every block, each channel of the generated image's tokens is moved a share
`stats_match` of the way to the picture's per-channel mean and standard deviation at the same block (AdaIN inside
the DiT, on the residual stream). Only 2 x 2048 numbers per block cross over, with no positions, so the picture's
palette, tonality and texture carry over but its subject doesn't.

reference_megapixels (default 1) is the size the picture is shrunk to first, aspect kept; it is never enlarged.
About the generated image's own size works best: smaller loses coarse texture such as pixel-art blocks, larger
drifts off the style, and at 4 with a 1024x1024 image the result falls apart. Bigger is also slower.

restyle (on by default) leaves the first quarter of the steps alone (steps 0 and 1 of 8), so the image keeps the
composition it would have had without the reference.

Only the positive prompt's evaluations are matched (cond_or_uncond 0); the negative branch runs as usual. Costs one
extra model pass per matched evaluation.
"""
import logging

import torch

import comfy.patcher_extension
import comfy.utils

WRAPPER_KEY = "broke_anima_style_reference"
# the reference pass's prompt, the picture's default size and the noise it's noised with
CAPTION = "no humans, scenery, detailed background"
MEGAPIXELS = 1.0
NOISE_SEED = 0x5EED
# restyle: the first quarter of the steps run without the reference (2 of 8)
RESTYLE_SKIP = 0.25
EPS = 1e-5


def _encode_image(model, vae, image, megapixels: float = MEGAPIXELS):
    """[B, H, W, C] -> model-space latent [1, 16, 1, h, w] of the first image, shrunk (never enlarged) to
    `megapixels`, aspect kept, sides multiples of 16."""
    pixels = image[:1].movedim(-1, 1)
    h, w = pixels.shape[-2:]
    s = min(1.0, (megapixels * 1024 * 1024 / (w * h)) ** 0.5)
    width, height = max(16, int(w * s) // 16 * 16), max(16, int(h * s) // 16 * 16)
    pixels = comfy.utils.common_upscale(pixels, width, height, "lanczos", "disabled").movedim(1, -1)
    latent = vae.encode(pixels[..., :3])
    if latent.ndim == 4:
        latent = latent.unsqueeze(2)
    return model.get_model_object("process_latent_in")(latent), (width, height)


def _step_index(sigma: float, sample_sigmas) -> int | None:
    """The step this sigma belongs to (a solver's intermediate sigma takes the step it is in)."""
    s = [float(v) for v in sample_sigmas]
    for i in range(len(s) - 1):
        if sigma <= s[i] * (1 + 1e-5) and sigma > s[i + 1]:
            return i
    return None


def _stats(x: torch.Tensor):
    """Per-channel mean and (population) variance over every token: x [B, T, H, W, D] -> ([B, D], [B, D])."""
    xf = x.float().flatten(1, -2)
    mean = xf.mean(1)
    var = (xf * xf).mean(1) - mean * mean
    return mean, var.clamp_min(0)


def _make_wrapper(ref_latent: torch.Tensor, ref_cond, strength: float, restyle: bool):
    noise_cache = {}
    ctx_cache = {}

    def wrapper(executor, x, timesteps, context, fps=None, padding_mask=None, **kwargs):
        options = kwargs.get("transformer_options", {})
        flags = options.get("cond_or_uncond") or [0]
        sigmas = options.get("sigmas")
        if strength <= 0 or sigmas is None or 0 not in flags:
            return executor(x, timesteps, context, fps, padding_mask, **kwargs)
        sigma = float(sigmas.flatten()[0])
        sample_sigmas = options.get("sample_sigmas")
        if restyle and sample_sigmas is not None:
            step = _step_index(sigma, sample_sigmas)
            if step is not None and step < round(RESTYLE_SKIP * (len(sample_sigmas) - 1)):
                return executor(x, timesteps, context, fps, padding_mask, **kwargs)
        dm = executor.class_obj
        blocks = getattr(dm, "blocks", None)
        if blocks is None or not hasattr(dm, "preprocess_text_embeds"):
            raise RuntimeError("Broke Anima Style Reference only works with Anima models")

        # the picture noised to this sigma with fixed noise (flow: x = (1 - sigma) x0 + sigma e)
        ref = ref_latent.to(x.device, x.dtype)
        if x.ndim == 4 and ref.ndim == 5:
            ref = ref.squeeze(2)
        key = (tuple(ref.shape), x.device, x.dtype)
        if key not in noise_cache:
            noise_cache.clear()
            g = torch.Generator("cpu").manual_seed(NOISE_SEED)
            noise_cache[key] = torch.randn(ref.shape, generator=g, dtype=torch.float32).to(x.device, x.dtype)
        x_ref = (1.0 - sigma) * ref + sigma * noise_cache[key]
        ckey = (x.device, x.dtype)
        if ckey not in ctx_cache:
            ctx_cache.clear()
            cross, meta = ref_cond
            ids, weights = meta.get("t5xxl_ids"), meta.get("t5xxl_weights")
            ctx_cache[ckey] = dm.preprocess_text_embeds(
                cross.to(x.device, x.dtype), None if ids is None else ids.unsqueeze(0).to(x.device),
                None if weights is None else weights.unsqueeze(0).unsqueeze(-1).to(x.device, x.dtype))
        ctx = ctx_cache[ckey]

        n = x.shape[0] // len(flags)
        rows = [r for r in range(x.shape[0]) if flags[r // n] == 0]
        ref_stats = [None] * len(blocks)
        state = {"mode": "capture"}

        def hook_for(b):
            def hook(module, args, out):
                if state["mode"] == "capture":
                    ref_stats[b] = _stats(out[:1])
                    return None
                mr, vr = ref_stats[b]
                y = out.clone()
                part = out[rows].float()
                mt, vt = _stats(part)
                shape = (part.shape[0],) + (1,) * (part.ndim - 2) + (part.shape[-1],)
                scale = ((vr + EPS) / (vt + EPS)).sqrt().reshape(shape)
                target = (part - mt.reshape(shape)) * scale + mr.reshape(shape)
                y[rows] = (part + strength * (target - part)).to(out.dtype)
                return y
            return hook

        handles = [blk.register_forward_hook(hook_for(b)) for b, blk in enumerate(blocks)]
        try:
            ref_options = dict(options)
            ref_options["cond_or_uncond"] = [0]
            ref_kwargs = dict(kwargs)
            ref_kwargs["transformer_options"] = ref_options
            executor(x_ref, timesteps[:1], ctx.expand(1, -1, -1), fps, padding_mask, **ref_kwargs)
            state["mode"] = "apply"
            return executor(x, timesteps, context, fps, padding_mask, **kwargs)
        finally:
            for h in handles:
                h.remove()

    return wrapper


class MooshieAnimaArtStyle:
    @classmethod
    def INPUT_TYPES(cls):
        return {
            "required": {
                "model": ("MODEL",),
                "clip": ("CLIP", {"tooltip": "Anima's text encoder: encodes the reference pass's neutral caption"}),
                "vae": ("VAE",),
                "image": ("IMAGE", {"tooltip": "the style picture (first image of the batch)"}),
                "stats_match": ("FLOAT", {"default": 0.6, "min": 0.0, "max": 1.0, "step": 0.05,
                                          "tooltip": "how far each channel moves towards the picture's statistics after every block. "
                                                     "0 = off; 0.6 is a good default; near 1 the image falls apart"}),
                "restyle": ("BOOLEAN", {"default": True,
                                        "tooltip": "leave the first quarter of the steps alone, keeping the composition the "
                                                   "prompt would have had without the picture"}),
                "reference_megapixels": ("FLOAT", {"default": MEGAPIXELS, "min": 0.5, "max": 4.0, "step": 0.05,
                                                   "tooltip": "the picture is shrunk (never enlarged) to this area, aspect kept. "
                                                              "About the generated image's size works best (1 for 1024x1024): smaller "
                                                              "loses coarse texture such as pixel-art blocks, larger drifts off the "
                                                              "style and falls apart near 4. Bigger is slower"}),
            }
        }

    RETURN_TYPES = ("MODEL",)
    FUNCTION = "apply"
    CATEGORY = "advanced/model"
    DESCRIPTION = ("Training-free style from a picture for Anima: matches each block's per-channel statistics to the "
                   "picture's at the same noise level. Takes palette, tonality and texture, not the subject.")

    def apply(self, model, clip, vae, image, stats_match, restyle, reference_megapixels=MEGAPIXELS):
        if stats_match <= 0:
            return (model,)
        latent, (width, height) = _encode_image(model, vae, image, float(reference_megapixels))
        cond = clip.encode_from_tokens_scheduled(clip.tokenize(CAPTION))[0]
        m = model.clone()
        m.add_wrapper_with_key(comfy.patcher_extension.WrappersMP.DIFFUSION_MODEL, WRAPPER_KEY,
                               _make_wrapper(latent, cond, float(stats_match), bool(restyle)))
        logging.info(f"[MooshieAnimaArtStyle] picture {width}x{height}, stats_match {stats_match}, restyle {restyle}")
        return (m,)


NODE_CLASS_MAPPINGS = {"MooshieAnimaArtStyle": MooshieAnimaArtStyle}
NODE_DISPLAY_NAME_MAPPINGS = {"MooshieAnimaArtStyle": "Mooshie Anima Art Style (Broke Anima Style Reference)"}
