"""
MooshieUI Guidance Nodes — in-house CFG refinement for upscaling and generation.

Two complementary model patches that can be used independently or together:

1. MooshieSoftGuidance ("Soft Guidance"):
   Rescales the CFG vector to prevent oversaturation and hallucination.
   Essential for tiled upscaling where high CFG + quality tags cause each tile
   to independently generate content (extra hands, objects, etc.).
   Recommended multiplier: 0.4 for upscaling, 0.7 for normal generation.

2. MooshieSmartGuidance ("Smart Guidance"):
   Biases guidance toward the *direction* of the positive prompt rather than
   the difference between positive and negative. Makes the model follow what
   you asked for more closely instead of just running away from the negative.
   Adaptive — no parameters needed.

These nodes are compatible and stack: SoftGuidance patches the CFG function,
SmartGuidance adds a post-CFG adjustment. Apply SoftGuidance first in the chain.

Based on the CFG Rescale (Common Diffusion Noise Schedules paper, Appendix I)
and Positive-Biased Guidance (similarity-adaptive) research.
"""

import torch
import torch.nn.functional as F

from typing_extensions import override

from comfy_api.latest import ComfyExtension, io


class MooshieSoftGuidance(io.ComfyNode):
    """Rescale CFG to prevent oversaturation and hallucination.

    At each denoising step, computes the standard CFG output then rescales it
    so the magnitude matches the positive-only prediction. The multiplier
    controls how much rescaling is applied:

      0.0 = no rescaling (standard CFG)
      0.4 = recommended for tiled upscaling (gentle guidance, no hallucination)
      0.7 = recommended for normal generation (improved quality)
      1.0 = full rescaling (may lose some contrast)
    """

    @classmethod
    def define_schema(cls):
        return io.Schema(
            node_id="MooshieSoftGuidance",
            display_name="Soft Guidance",
            category="mooshie/guidance",
            description=(
                "Prevents CFG from oversaturating by rescaling the guidance vector. "
                "Essential for tiled upscaling to stop quality tags from hallucinating "
                "extra content. Recommended: 0.4 for upscaling, 0.7 for generation."
            ),
            search_aliases=[
                "soft guidance", "cfg rescale", "rescale cfg",
                "anti burn", "anti hallucination", "upscale fix",
                "tile artifact fix", "gentle cfg",
            ],
            inputs=[
                io.Model.Input("model"),
                io.Float.Input(
                    "multiplier",
                    default=0.4,
                    min=0.0,
                    max=1.0,
                    step=0.05,
                    tooltip=(
                        "How much to rescale CFG. 0 = standard CFG (no change), "
                        "0.4 = recommended for upscaling, 0.7 = general use, "
                        "1.0 = full rescale."
                    ),
                ),
            ],
            outputs=[
                io.Model.Output(display_name="patched_model"),
            ],
        )

    @classmethod
    def execute(cls, model, multiplier) -> io.NodeOutput:
        m = model.clone()

        def soft_guidance_cfg(args):
            cond = args["cond"]
            uncond = args["uncond"]
            cond_scale = args["cond_scale"]
            sigma = args["sigma"]
            x_orig = args["input"]

            sigma = sigma.view(sigma.shape[:1] + (1,) * (cond.ndim - 1))

            # Convert to v-prediction space for correct rescaling
            x = x_orig / (sigma * sigma + 1.0)
            scale_factor = (sigma ** 2 + 1.0) ** 0.5 / sigma
            cond_v = (x - (x_orig - cond)) * scale_factor
            uncond_v = (x - (x_orig - uncond)) * scale_factor

            # Standard CFG in v-space
            x_cfg = uncond_v + cond_scale * (cond_v - uncond_v)

            # Rescale: match the magnitude of the positive-only prediction
            ro_pos = torch.std(cond_v, dim=tuple(range(1, cond_v.ndim)), keepdim=True)
            ro_cfg = torch.std(x_cfg, dim=tuple(range(1, x_cfg.ndim)), keepdim=True)

            x_rescaled = x_cfg * (ro_pos / ro_cfg.clamp(min=1e-8))

            # Blend between standard and rescaled based on multiplier
            x_final = multiplier * x_rescaled + (1.0 - multiplier) * x_cfg

            # Convert back from v-space
            return x_orig - (x - x_final * sigma / (sigma * sigma + 1.0) ** 0.5)

        m.set_model_sampler_cfg_function(soft_guidance_cfg)
        return io.NodeOutput(m)


class MooshieSmartGuidance(io.ComfyNode):
    """Positive-biased guidance that follows your prompt direction.

    Standard CFG works by amplifying the difference between the positive and
    negative predictions. This can cause artifacts when the negative prompt
    pulls too hard in the wrong direction.

    Smart Guidance instead scales more toward the *direction* of the positive
    prompt. It measures the cosine similarity between the unconditional and
    blended predictions, then adaptively favours the positive-only leap when
    the model is uncertain. The effect is cleaner output with fewer
    negative-prompt-induced distortions.

    Adaptive — no parameters needed. Works best combined with Soft Guidance.
    """

    @classmethod
    def define_schema(cls):
        return io.Schema(
            node_id="MooshieSmartGuidance",
            display_name="Smart Guidance",
            category="mooshie/guidance",
            description=(
                "Makes the model follow your positive prompt direction more closely "
                "instead of just pushing away from the negative. Adaptive, no "
                "parameters needed. Stack with Soft Guidance for best results."
            ),
            search_aliases=[
                "smart guidance", "positive biased", "positive bias cfg",
                "mahiro", "similarity guidance", "adaptive guidance",
                "prompt follower",
            ],
            inputs=[
                io.Model.Input("model"),
            ],
            outputs=[
                io.Model.Output(display_name="patched_model"),
            ],
        )

    @classmethod
    def execute(cls, model) -> io.NodeOutput:
        m = model.clone()

        def smart_guidance_post_cfg(args):
            scale: float = args["cond_scale"]
            cond_p: torch.Tensor = args["cond_denoised"]
            uncond_p: torch.Tensor = args["uncond_denoised"]
            cfg: torch.Tensor = args["denoised"]

            # Naive positive-only leap (where the positive prompt wants to go)
            leap = cond_p * scale

            # Uncond-scaled leap (where the model goes without prompt)
            u_leap = uncond_p * scale

            # Blend between leap and standard CFG
            merge = (leap + cfg) / 2

            # Sqrt-normalised vectors for stable cosine similarity
            normu = torch.sqrt(u_leap.abs()) * u_leap.sign()
            normm = torch.sqrt(merge.abs()) * merge.sign()

            # Cosine similarity determines how aligned uncond is with the blend.
            # When highly aligned (model is confident), use more standard CFG.
            # When divergent (model is uncertain), lean toward positive leap.
            sim = F.cosine_similarity(normu, normm).mean()
            simsc = 2.0 * (sim + 1.0)

            # Weighted merge: higher simsc = more standard CFG, lower = more leap
            result = (simsc * cfg + (4.0 - simsc) * leap) / 4.0
            return result

        m.set_model_sampler_post_cfg_function(smart_guidance_post_cfg)
        return io.NodeOutput(m)


class GuidanceExtension(ComfyExtension):
    @override
    async def get_node_list(self) -> list[type[io.ComfyNode]]:
        return [
            MooshieSoftGuidance,
            MooshieSmartGuidance,
        ]


async def comfy_entrypoint() -> GuidanceExtension:
    return GuidanceExtension()
