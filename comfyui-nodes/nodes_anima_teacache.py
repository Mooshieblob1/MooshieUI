"""
MooshieUI Anima TeaCache — step-caching acceleration for the Anima
(Cosmos-Predict2 DiT) model family.

Independent reimplementation of the TeaCache idea (Liu et al. 2024,
arxiv:2411.19108) for Anima: caches the model's noise prediction across
consecutive denoising steps and reuses it whenever the accumulated relative
L1 change in the model's input stays under a threshold, skipping the DiT
forward pass entirely for that step.

Hooks in via `set_model_unet_function_wrapper`, which wraps `model.apply_model`
one level above Anima's own internal `MiniTrainDIT` wrapper chain (Comfy's
`WrappersMP.DIFFUSION_MODEL`). That makes the caching decision agnostic to
Anima's architecture — it only ever looks at the shape-agnostic input/output
tensors of the wrapped call, the same level MooshieUI's existing MiniMax H3
TeaCache node (`ComfyUI-MiniMaxH3-TeaCache`) operates at for video models.
"""

from dataclasses import dataclass
from typing import Optional

import torch
from typing_extensions import override

from comfy_api.latest import ComfyExtension, io


@dataclass
class _TeaCacheState:
    step: int = -1
    accumulated_distance: float = 0.0
    previous_input_mean: Optional[torch.Tensor] = None
    previous_residual: Optional[torch.Tensor] = None


def _rel_l1(a: torch.Tensor, b: torch.Tensor) -> float:
    diff = (a - b).abs().mean()
    base = a.abs().mean().clamp(min=1e-8)
    return (diff / base).item()


def _make_wrapper(state, rel_l1_thresh, start_step, end_step, total_steps):
    def wrapper(apply_model, args):
        x = args["input"]
        c = args["c"]

        state.step += 1
        step = state.step

        effective_end = end_step if end_step >= 0 else max(total_steps + end_step, 0)
        in_window = start_step <= step < effective_end

        # Mean-pooled per-sample signature of the current step's input. Fully
        # shape-agnostic, so it works for Anima's 5D latents the same way it
        # would for a 4D image-model latent.
        input_mean = x.detach().mean(dim=tuple(range(1, x.ndim)))

        can_reuse = False
        if in_window and state.previous_residual is not None and state.previous_input_mean is not None:
            state.accumulated_distance += _rel_l1(input_mean, state.previous_input_mean)
            if state.accumulated_distance < rel_l1_thresh:
                can_reuse = True
            else:
                state.accumulated_distance = 0.0
        elif not in_window:
            state.accumulated_distance = 0.0

        state.previous_input_mean = input_mean

        if can_reuse:
            return x + state.previous_residual

        out = apply_model(x, args["timestep"], **c)
        state.previous_residual = (out - x).detach()
        return out

    return wrapper


class MooshieAnimaTeaCache(io.ComfyNode):
    """TeaCache-style step caching for Anima (Cosmos-Predict2 DiT).

    Reuses the previous step's noise prediction while the accumulated
    relative L1 change in the model's input stays under `rel_l1_thresh`,
    skipping the DiT forward pass for that step. `start_step`/`end_step`
    exclude the first/last steps (where output changes fastest) from
    caching, since reusing there tends to soften fine detail.
    """

    @classmethod
    def define_schema(cls):
        return io.Schema(
            node_id="MooshieAnimaTeaCache",
            display_name="Anima TeaCache",
            category="mooshie/caching",
            description=(
                "Skips the Anima DiT's forward pass on steps where the "
                "output barely changed from the last one, reusing the "
                "cached result instead. Faster with a small risk of softer "
                "detail; excludes the first/last steps by default."
            ),
            search_aliases=[
                "teacache", "anima cache", "cosmos cache",
                "speed up anima", "dit cache", "step cache",
            ],
            inputs=[
                io.Model.Input("model"),
                io.Float.Input(
                    "rel_l1_thresh",
                    default=0.15,
                    min=0.0,
                    max=1.0,
                    step=0.01,
                    tooltip=(
                        "Accumulated relative-L1 input change allowed before a "
                        "step is recomputed. Higher = more caching (faster, "
                        "softer); 0 disables caching entirely."
                    ),
                ),
                io.Int.Input(
                    "start_step",
                    default=2,
                    min=0,
                    max=1000,
                    tooltip="First step eligible for caching. Early steps are excluded by default.",
                ),
                io.Int.Input(
                    "end_step",
                    default=-2,
                    min=-1000,
                    max=1000,
                    tooltip="Last step eligible for caching. Negative counts back from total_steps.",
                ),
                io.Int.Input(
                    "total_steps",
                    default=20,
                    min=1,
                    max=1000,
                    tooltip="Total sampling steps for this generation (must match the sampler's step count).",
                ),
            ],
            outputs=[
                io.Model.Output(display_name="patched_model"),
            ],
        )

    @classmethod
    def execute(cls, model, rel_l1_thresh, start_step, end_step, total_steps) -> io.NodeOutput:
        m = model.clone()
        state = _TeaCacheState()
        wrapper = _make_wrapper(state, rel_l1_thresh, start_step, end_step, total_steps)
        m.set_model_unet_function_wrapper(wrapper)
        return io.NodeOutput(m)


class AnimaTeaCacheExtension(ComfyExtension):
    @override
    async def get_node_list(self) -> list[type[io.ComfyNode]]:
        return [MooshieAnimaTeaCache]


async def comfy_entrypoint() -> AnimaTeaCacheExtension:
    return AnimaTeaCacheExtension()
