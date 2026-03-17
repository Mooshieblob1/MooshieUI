"""
    This file is part of ComfyUI.
    Copyright (C) 2024 Comfy

    This program is free software: you can redistribute it and/or modify
    it under the terms of the GNU General Public License as published by
    the Free Software Foundation, either version 3 of the License, or
    (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU General Public License for more details.

    You should have received a copy of the GNU General Public License
    along with this program.  If not, see <https://www.gnu.org/licenses/>.
"""

"""
Tiled Diffusion nodes for ComfyUI.

Implements MultiDiffusion and SpotDiffusion tiling strategies for
high-resolution image generation and upscaling across all generation modes
(txt2img, img2img, inpainting).

MultiDiffusion: Bar-Tal et al., "MultiDiffusion: Fusing Diffusion Paths
for Controlled Image Generation", ICML 2023. arxiv.org/abs/2302.08113

SpotDiffusion: Ding et al., arxiv.org/abs/2407.15507

Unlike tile-by-tile approaches (e.g. Ultimate Upscale), these methods process
ALL tiles at each denoising step and blend them together, producing seamless
results much faster. Compatible with Anima (COSMOS-based) models via tiled mode.
"""

import logging
import math
import random
from typing import Callable

import torch
from typing_extensions import override

from comfy_api.latest import ComfyExtension, io

logger = logging.getLogger(__name__)


def _get_tile_positions(h: int, w: int, tile_h: int, tile_w: int,
                        overlap_h: int, overlap_w: int) -> list[tuple[int, int, int, int]]:
    """Generate tile positions covering the full [h, w] area with overlap.

    Returns list of (y, x, height, width) tuples.
    """
    tile_h = min(tile_h, h)
    tile_w = min(tile_w, w)

    stride_h = max(1, tile_h - overlap_h)
    stride_w = max(1, tile_w - overlap_w)

    y_positions = list(range(0, max(1, h - tile_h + 1), stride_h))
    x_positions = list(range(0, max(1, w - tile_w + 1), stride_w))

    # Ensure full coverage
    if not y_positions or y_positions[-1] + tile_h < h:
        y_positions.append(max(0, h - tile_h))
    if not x_positions or x_positions[-1] + tile_w < w:
        x_positions.append(max(0, w - tile_w))

    # Deduplicate preserving order
    y_positions = list(dict.fromkeys(y_positions))
    x_positions = list(dict.fromkeys(x_positions))

    tiles = []
    for y in y_positions:
        for x in x_positions:
            th = min(tile_h, h - y)
            tw = min(tile_w, w - x)
            tiles.append((y, x, th, tw))
    return tiles


def _create_blend_mask(h: int, w: int, overlap_h: int, overlap_w: int,
                       device: torch.device, dtype: torch.dtype) -> torch.Tensor:
    """Create a cosine-feathered blending mask for seamless tile merging.

    Uses a raised-cosine (Hann) window in the overlap region for smooth
    transitions between adjacent tiles.

    Returns: [1, 1, h, w] mask tensor.
    """
    if overlap_h == 0 and overlap_w == 0:
        return torch.ones(1, 1, h, w, device=device, dtype=dtype)

    mask_h = torch.ones(h, device=device, dtype=dtype)
    mask_w = torch.ones(w, device=device, dtype=dtype)

    feather_h = min(overlap_h, h // 2)
    feather_w = min(overlap_w, w // 2)

    if feather_h > 0:
        ramp = torch.linspace(0, math.pi / 2, feather_h + 1, device=device, dtype=dtype)[1:]
        ramp = torch.sin(ramp) ** 2
        mask_h[:feather_h] = ramp
        mask_h[-feather_h:] = ramp.flip(0)

    if feather_w > 0:
        ramp = torch.linspace(0, math.pi / 2, feather_w + 1, device=device, dtype=dtype)[1:]
        ramp = torch.sin(ramp) ** 2
        mask_w[:feather_w] = ramp
        mask_w[-feather_w:] = ramp.flip(0)

    return (mask_h.unsqueeze(1) * mask_w.unsqueeze(0)).unsqueeze(0).unsqueeze(0)


def _crop_cond_spatial(c: dict, y: int, x: int, h: int, w: int,
                       full_h: int, full_w: int) -> dict:
    """Crop spatial conditioning tensors to match a tile region.

    Handles c_concat (inpainting), ControlNet outputs, and passes through
    non-spatial tensors (c_crossattn, transformer_options) unchanged.
    """
    cropped = {}
    for key, val in c.items():
        if val is None:
            cropped[key] = None
        elif key == 'c_concat' and isinstance(val, torch.Tensor) and val.ndim >= 4:
            rh = val.shape[2] / full_h
            rw = val.shape[3] / full_w
            cy, cx = round(y * rh), round(x * rw)
            ch, cw = round(h * rh), round(w * rw)
            ch = min(ch, val.shape[2] - cy)
            cw = min(cw, val.shape[3] - cx)
            cropped[key] = val[:, :, cy:cy + ch, cx:cx + cw]
        elif key == 'control' and val is not None:
            cropped[key] = _crop_control(val, y, x, h, w, full_h, full_w)
        else:
            cropped[key] = val
    return cropped


def _crop_control(control, y: int, x: int, h: int, w: int,
                  full_h: int, full_w: int):
    """Crop ControlNet output tensors proportionally to tile region."""
    if isinstance(control, dict):
        out = {}
        for key, tensors in control.items():
            if isinstance(tensors, list):
                cropped_list = []
                for t in tensors:
                    if isinstance(t, torch.Tensor) and t.ndim >= 4:
                        rh = t.shape[2] / full_h
                        rw = t.shape[3] / full_w
                        cy, cx = round(y * rh), round(x * rw)
                        ch = min(round(h * rh), t.shape[2] - cy)
                        cw = min(round(w * rw), t.shape[3] - cx)
                        cropped_list.append(t[:, :, cy:cy + ch, cx:cx + cw])
                    else:
                        cropped_list.append(t)
                out[key] = cropped_list
            elif isinstance(tensors, torch.Tensor) and tensors.ndim >= 4:
                rh = tensors.shape[2] / full_h
                rw = tensors.shape[3] / full_w
                cy, cx = round(y * rh), round(x * rw)
                ch = min(round(h * rh), tensors.shape[2] - cy)
                cw = min(round(w * rw), tensors.shape[3] - cx)
                out[key] = tensors[:, :, cy:cy + ch, cx:cx + cw]
            else:
                out[key] = tensors
        return out
    return control


def _multidiffusion_step(apply_model: Callable, args: dict,
                         tile_h: int, tile_w: int,
                         overlap_h: int, overlap_w: int) -> torch.Tensor:
    """MultiDiffusion: denoise all tiles and blend with feathered overlap.

    At each denoising step, splits the latent into overlapping tiles, denoises
    each independently, then blends them together using cosine-feathered masks.

    Supports both 4D (B,C,H,W) and 5D (B,C,T,H,W) latents (e.g. Anima/COSMOS).
    """
    input_x = args["input"]
    timestep = args["timestep"]
    c = args["c"]

    # Support both 4D (B,C,H,W) and 5D (B,C,T,H,W) latents
    is_5d = input_x.ndim == 5
    H, W = input_x.shape[-2], input_x.shape[-1]

    if H <= tile_h and W <= tile_w:
        return apply_model(input_x, timestep, **c)

    tiles = _get_tile_positions(H, W, tile_h, tile_w, overlap_h, overlap_w)

    result = torch.zeros_like(input_x)
    # Weight shape must broadcast over all leading dims
    weight_shape = [1] * (input_x.ndim - 2) + [H, W]
    weight = torch.zeros(weight_shape, device=input_x.device, dtype=input_x.dtype)

    for (ty, tx, th, tw) in tiles:
        tile_input = input_x[..., ty:ty + th, tx:tx + tw]
        tile_c = _crop_cond_spatial(c, ty, tx, th, tw, H, W)
        tile_out = apply_model(tile_input, timestep, **tile_c)

        mask = _create_blend_mask(th, tw, overlap_h, overlap_w,
                                  input_x.device, input_x.dtype)
        if is_5d:
            # Expand mask from [1,1,h,w] to [1,1,1,h,w] for 5D tensors
            mask = mask.unsqueeze(2)
        result[..., ty:ty + th, tx:tx + tw] += tile_out * mask
        weight[..., ty:ty + th, tx:tx + tw] += mask.squeeze()

    # Expand weight to broadcast with result
    while weight.ndim < result.ndim:
        weight = weight.unsqueeze(0)

    return result / weight.clamp(min=1e-8)


def _spotdiffusion_step(apply_model: Callable, args: dict,
                        tile_h: int, tile_w: int,
                        step_counter: list) -> torch.Tensor:
    """SpotDiffusion: random tile shift per timestep for fast seamless tiling.

    Eliminates seams by randomly offsetting the tile grid at each denoising
    step. Faster than MultiDiffusion since no overlap is needed.

    Supports both 4D (B,C,H,W) and 5D (B,C,T,H,W) latents (e.g. Anima/COSMOS).
    """
    input_x = args["input"]
    timestep = args["timestep"]
    c = args["c"]

    # Support both 4D and 5D latents — spatial dims are always the last two
    H, W = input_x.shape[-2], input_x.shape[-1]
    spatial_dims = (-2, -1)

    if H <= tile_h and W <= tile_w:
        return apply_model(input_x, timestep, **c)

    step_counter[0] += 1
    rng = random.Random(step_counter[0] * 31337)
    shift_h = rng.randint(0, max(0, tile_h - 1)) if H > tile_h else 0
    shift_w = rng.randint(0, max(0, tile_w - 1)) if W > tile_w else 0

    # Circular shift input over spatial dims
    shifted = torch.roll(input_x, shifts=(shift_h, shift_w), dims=spatial_dims)

    # Also shift spatial conditioning
    shifted_c = {}
    for key, val in c.items():
        if key == 'c_concat' and isinstance(val, torch.Tensor) and val.ndim >= 4:
            shifted_c[key] = torch.roll(val, shifts=(shift_h, shift_w), dims=(-2, -1))
        elif key == 'control' and val is not None:
            shifted_c[key] = _shift_control(val, shift_h, shift_w, H, W)
        else:
            shifted_c[key] = val

    tiles = _get_tile_positions(H, W, tile_h, tile_w, 0, 0)
    result = torch.zeros_like(shifted)

    for (ty, tx, th, tw) in tiles:
        tile_input = shifted[..., ty:ty + th, tx:tx + tw]
        tile_c = _crop_cond_spatial(shifted_c, ty, tx, th, tw, H, W)
        tile_out = apply_model(tile_input, timestep, **tile_c)
        result[..., ty:ty + th, tx:tx + tw] = tile_out

    # Shift back
    return torch.roll(result, shifts=(-shift_h, -shift_w), dims=spatial_dims)


def _shift_control(control, shift_h: int, shift_w: int,
                   latent_h: int, latent_w: int):
    """Circular-shift ControlNet output tensors, scaling shift proportionally.

    ControlNet outputs have feature maps at various downsampled resolutions.
    The shift must be scaled relative to each tensor's spatial dimensions
    to maintain alignment with the shifted latent.
    """
    def _shift_tensor(t: torch.Tensor) -> torch.Tensor:
        if t.ndim < 4:
            return t
        sh = round(shift_h * t.shape[2] / latent_h)
        sw = round(shift_w * t.shape[3] / latent_w)
        return torch.roll(t, shifts=(sh, sw), dims=(2, 3))

    if isinstance(control, dict):
        out = {}
        for key, tensors in control.items():
            if isinstance(tensors, list):
                out[key] = [
                    _shift_tensor(t) if isinstance(t, torch.Tensor) else t
                    for t in tensors
                ]
            elif isinstance(tensors, torch.Tensor):
                out[key] = _shift_tensor(tensors)
            else:
                out[key] = tensors
        return out
    return control


class ApplyTiledDiffusion(io.ComfyNode):
    """Patches a model for MultiDiffusion or SpotDiffusion tiled denoising.

    Enables high-resolution generation/upscaling by splitting the latent into
    tiles at each denoising step and blending results. Works with all generation
    modes: txt2img, img2img, and inpainting.

    Connect this node's MODEL output to any KSampler to enable tiled processing.
    """

    @classmethod
    def define_schema(cls):
        return io.Schema(
            node_id="ApplyTiledDiffusion",
            display_name="Apply Tiled Diffusion",
            category="model_patches/unet",
            description=(
                "Enables tiled diffusion for high-resolution generation and upscaling. "
                "MultiDiffusion blends overlapping tiles for best quality. "
                "SpotDiffusion randomly shifts tiles per step for maximum speed. "
                "Compatible with Anima (COSMOS) and all diffusion models."
            ),
            search_aliases=[
                "multidiffusion", "tiled sampling", "tiled upscale",
                "tile diffusion", "hires fix", "spotdiffusion",
                "tiled denoise", "upscale diffusion",
            ],
            inputs=[
                io.Model.Input("model"),
                io.Combo.Input("method", options=["MultiDiffusion", "SpotDiffusion"],
                               default="MultiDiffusion", tooltip=(
                                   "MultiDiffusion: overlapping tiles with cosine blending "
                                   "(best quality). SpotDiffusion: random shift per step, "
                                   "no overlap (fastest, experimental).")),
                io.Int.Input("tile_width", default=1024, min=256, max=4096, step=64,
                             tooltip="Tile width in pixels. Should match model's native resolution (e.g. 512 for SD1.5, 1024 for SDXL/Flux)."),
                io.Int.Input("tile_height", default=1024, min=256, max=4096, step=64,
                             tooltip="Tile height in pixels. Should match model's native resolution."),
                io.Int.Input("tile_overlap", default=128, min=0, max=512, step=16,
                             tooltip="Overlap between tiles in pixels. Higher = smoother seams but slower. Ignored for SpotDiffusion."),
            ],
            outputs=[
                io.Model.Output(),
            ],
        )

    @classmethod
    def execute(cls, model, method, tile_width, tile_height, tile_overlap) -> io.NodeOutput:
        m = model.clone()

        # Get latent downscale ratio from the model
        try:
            latent_format = m.get_model_object("latent_format")
            downscale = latent_format.spacial_downscale_ratio
        except Exception:
            downscale = 8  # Fallback for standard SD models

        # Convert pixel dimensions to latent space
        tile_h = max(1, tile_height // downscale)
        tile_w = max(1, tile_width // downscale)
        overlap_h = max(0, tile_overlap // downscale)
        overlap_w = max(0, tile_overlap // downscale)

        # Preserve any existing wrapper
        old_wrapper = m.model_options.get("model_function_wrapper", None)

        if method == "SpotDiffusion":
            step_counter = [0]

            def wrapper(apply_model: Callable, args: dict) -> torch.Tensor:
                if old_wrapper is not None:
                    inner = lambda inp, ts, **kw: old_wrapper(
                        apply_model, {"input": inp, "timestep": ts, "c": kw,
                                      "cond_or_uncond": args.get("cond_or_uncond", [])})
                else:
                    inner = apply_model
                return _spotdiffusion_step(
                    inner, args, tile_h, tile_w, step_counter)
        else:
            def wrapper(apply_model: Callable, args: dict) -> torch.Tensor:
                if old_wrapper is not None:
                    inner = lambda inp, ts, **kw: old_wrapper(
                        apply_model, {"input": inp, "timestep": ts, "c": kw,
                                      "cond_or_uncond": args.get("cond_or_uncond", [])})
                else:
                    inner = apply_model
                return _multidiffusion_step(
                    inner, args, tile_h, tile_w, overlap_h, overlap_w)

        m.set_model_unet_function_wrapper(wrapper)

        logger.info("Tiled Diffusion enabled: method=%s, tile=%dx%d (latent %dx%d), overlap=%d",
                     method, tile_width, tile_height, tile_w, tile_h, tile_overlap)
        return io.NodeOutput(m)


class TiledDiffusionExtension(ComfyExtension):
    @override
    async def get_node_list(self) -> list[type[io.ComfyNode]]:
        return [
            ApplyTiledDiffusion,
        ]


async def comfy_entrypoint() -> TiledDiffusionExtension:
    return TiledDiffusionExtension()
