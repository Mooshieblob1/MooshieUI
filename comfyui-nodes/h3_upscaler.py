"""Model and checkpoint contract for the MiniMax H3 clean-latent 2x upscaler."""

# Vendored from mamad8c/ComfyUI-H3-Latent-Upscaler-Mamad8,
# revision e98237773011523528353a8beb4863e65b099a38. See h3_upscaler.LICENSE.

from __future__ import annotations

import json
from dataclasses import dataclass, fields
from pathlib import Path
from typing import Any, Mapping

import torch
import torch.nn as nn
import torch.nn.functional as F
from safetensors import safe_open


CHECKPOINT_FORMAT = "minimax_h3_clean_latent_upscaler_v3_factorized_attention"


@dataclass(frozen=True)
class UpscalerConfigV2:
    in_channels: int
    hidden_channels: int
    num_blocks: int
    refine_channels: int
    refine_blocks: int
    temporal_kernel: int


@dataclass(frozen=True)
class UpscalerConfigV3:
    width: int
    blocks: int
    heads: int
    window: int
    mlp_ratio: int


@dataclass(frozen=True)
class CheckpointInfo:
    base_config: UpscalerConfigV2
    config: UpscalerConfigV3
    step: int | None


def _config_from_mapping(cls, value: Any, label: str):
    if not isinstance(value, Mapping):
        raise ValueError(f"Checkpoint metadata '{label}' must be an object")
    expected = {field.name for field in fields(cls)}
    missing = sorted(expected - set(value))
    unknown = sorted(set(value) - expected)
    if missing or unknown:
        raise ValueError(
            f"Checkpoint metadata '{label}' has missing={missing} and unknown={unknown} fields"
        )
    parsed: dict[str, int] = {}
    for name in expected:
        raw = value[name]
        if isinstance(raw, bool):
            raise ValueError(f"Checkpoint metadata '{label}.{name}' must be an integer")
        try:
            parsed[name] = int(raw)
        except (TypeError, ValueError) as exc:
            raise ValueError(
                f"Checkpoint metadata '{label}.{name}' must be an integer, got {raw!r}"
            ) from exc
        if parsed[name] < 1:
            raise ValueError(f"Checkpoint metadata '{label}.{name}' must be positive")
    return cls(**parsed)


def _validate_configs(base: UpscalerConfigV2, config: UpscalerConfigV3) -> None:
    if base.in_channels != 24:
        raise ValueError(f"Expected 24 H3 video-latent channels, got {base.in_channels}")
    if base.temporal_kernel % 2 != 1:
        raise ValueError("The temporal kernel must be odd")
    if config.width % config.heads:
        raise ValueError("Attention width must be divisible by attention heads")

    # Fail before allocating a pathological model from untrusted metadata.
    limits = {
        "hidden_channels": (base.hidden_channels, 1024),
        "num_blocks": (base.num_blocks, 64),
        "refine_channels": (base.refine_channels, 1024),
        "refine_blocks": (base.refine_blocks, 64),
        "temporal_kernel": (base.temporal_kernel, 9),
        "width": (config.width, 1024),
        "blocks": (config.blocks, 64),
        "heads": (config.heads, 64),
        "window": (config.window, 64),
        "mlp_ratio": (config.mlp_ratio, 8),
    }
    for name, (value, maximum) in limits.items():
        if value > maximum:
            raise ValueError(f"Checkpoint metadata '{name}' exceeds the supported limit {maximum}")


def read_checkpoint_info(path: str | Path) -> CheckpointInfo:
    """Read and strictly validate the small JSON contract embedded in the file."""

    path = Path(path)
    if path.suffix.lower() != ".safetensors":
        raise ValueError("H3 latent upscalers must be safetensors files")
    with safe_open(str(path), framework="pt", device="cpu") as handle:
        outer = handle.metadata() or {}
    raw = outer.get("metadata")
    if not isinstance(raw, str):
        raise ValueError("Checkpoint is missing its H3 latent-upscaler metadata")
    try:
        metadata = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ValueError("Checkpoint contains invalid H3 latent-upscaler metadata JSON") from exc
    if metadata.get("format") != CHECKPOINT_FORMAT:
        raise ValueError(
            f"Unsupported checkpoint format {metadata.get('format')!r}; expected {CHECKPOINT_FORMAT!r}"
        )
    if metadata.get("strict_latent_only") is not True:
        raise ValueError("Checkpoint must declare strict_latent_only=true")

    base = _config_from_mapping(UpscalerConfigV2, metadata.get("base_config"), "base_config")
    config = _config_from_mapping(UpscalerConfigV3, metadata.get("config"), "config")
    _validate_configs(base, config)

    step = None
    if "step" in outer:
        try:
            step = int(outer["step"])
        except (TypeError, ValueError) as exc:
            raise ValueError(f"Checkpoint step must be an integer, got {outer['step']!r}") from exc
        if step < 0:
            raise ValueError("Checkpoint step cannot be negative")
    return CheckpointInfo(base_config=base, config=config, step=step)


def spatial_bilinear_2x(x: torch.Tensor) -> torch.Tensor:
    """Resize B,C,T,H,W spatially without interpolating between time tokens."""

    batch, channels, frames, height, width = x.shape
    image_batch = x.permute(0, 2, 1, 3, 4).reshape(
        batch * frames, channels, height, width
    )
    image_batch = F.interpolate(
        image_batch,
        size=(height * 2, width * 2),
        mode="bilinear",
        align_corners=False,
    )
    return image_batch.view(
        batch, frames, channels, height * 2, width * 2
    ).permute(0, 2, 1, 3, 4)


def spatial_pixel_shuffle_2x(x: torch.Tensor, out_channels: int) -> torch.Tensor:
    """Channel-to-space shuffle for B,(C*4),T,H,W tensors."""

    batch, channels, frames, height, width = x.shape
    if channels != out_channels * 4:
        raise ValueError(f"Expected {out_channels * 4} channels, got {channels}")
    x = x.view(batch, out_channels, 2, 2, frames, height, width)
    return x.permute(0, 1, 4, 5, 2, 6, 3).reshape(
        batch, out_channels, frames, height * 2, width * 2
    )


def _groups_for(channels: int) -> int:
    groups = min(16, channels)
    while channels % groups:
        groups -= 1
    return groups


class ResidualBlock3d(nn.Module):
    def __init__(
        self,
        channels: int,
        temporal_kernel: int = 3,
        spatial_dilation: int = 1,
    ):
        super().__init__()
        temporal_padding = temporal_kernel // 2
        padding = (temporal_padding, spatial_dilation, spatial_dilation)
        dilation = (1, spatial_dilation, spatial_dilation)
        self.norm1 = nn.GroupNorm(_groups_for(channels), channels, eps=1e-6)
        self.conv1 = nn.Conv3d(
            channels,
            channels,
            (temporal_kernel, 3, 3),
            padding=padding,
            dilation=dilation,
        )
        self.norm2 = nn.GroupNorm(_groups_for(channels), channels, eps=1e-6)
        self.conv2 = nn.Conv3d(
            channels,
            channels,
            (temporal_kernel, 3, 3),
            padding=padding,
            dilation=dilation,
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        residual = self.conv1(F.silu(self.norm1(x)))
        residual = self.conv2(F.silu(self.norm2(residual)))
        return x + residual


class H3LatentUpscalerV2(nn.Module):
    def __init__(self, config: UpscalerConfigV2):
        super().__init__()
        self.config = config
        temporal_padding = config.temporal_kernel // 2
        self.low_stem = nn.Conv3d(
            config.in_channels,
            config.hidden_channels,
            (config.temporal_kernel, 3, 3),
            padding=(temporal_padding, 1, 1),
        )
        dilation_pattern = (1, 2, 1, 3)
        self.low_blocks = nn.ModuleList(
            ResidualBlock3d(
                config.hidden_channels,
                config.temporal_kernel,
                dilation_pattern[index % len(dilation_pattern)],
            )
            for index in range(config.num_blocks)
        )
        self.low_norm = nn.GroupNorm(
            _groups_for(config.hidden_channels), config.hidden_channels, eps=1e-6
        )
        self.to_high = nn.Conv3d(
            config.hidden_channels,
            config.refine_channels * 4,
            (config.temporal_kernel, 3, 3),
            padding=(temporal_padding, 1, 1),
        )
        self.refine_stem = nn.Conv3d(
            config.refine_channels + config.in_channels,
            config.refine_channels,
            (config.temporal_kernel, 3, 3),
            padding=(temporal_padding, 1, 1),
        )
        self.refine_blocks = nn.ModuleList(
            ResidualBlock3d(config.refine_channels, config.temporal_kernel)
            for _ in range(config.refine_blocks)
        )
        self.out_norm = nn.GroupNorm(
            _groups_for(config.refine_channels), config.refine_channels, eps=1e-6
        )
        self.out = nn.Conv3d(
            config.refine_channels,
            config.in_channels,
            (config.temporal_kernel, 3, 3),
            padding=(temporal_padding, 1, 1),
        )

    def correction(self, low: torch.Tensor) -> torch.Tensor:
        base = spatial_bilinear_2x(low)
        features = self.low_stem(low)
        for block in self.low_blocks:
            features = block(features)
        features = self.to_high(F.silu(self.low_norm(features)))
        features = spatial_pixel_shuffle_2x(features, self.config.refine_channels)
        features = self.refine_stem(torch.cat([features, base], dim=1))
        for block in self.refine_blocks:
            features = block(features)
        return self.out(F.silu(self.out_norm(features)))

    def forward(self, low: torch.Tensor) -> torch.Tensor:
        return spatial_bilinear_2x(low) + self.correction(low)


class FactorizedBlock(nn.Module):
    def __init__(
        self,
        width: int,
        heads: int,
        window: int,
        shifted: bool,
        mlp_ratio: int,
    ):
        super().__init__()
        self.window = window
        self.shift = window // 2 if shifted else 0
        self.spatial_norm = nn.LayerNorm(width, eps=1e-6)
        self.spatial = nn.MultiheadAttention(width, heads, batch_first=True)
        self.temporal_norm = nn.LayerNorm(width, eps=1e-6)
        self.temporal = nn.MultiheadAttention(width, heads, batch_first=True)
        self.local_norm = nn.GroupNorm(1, width, eps=1e-6)
        self.local = nn.Conv3d(width, width, 3, padding=1, groups=width)
        hidden = width * mlp_ratio
        self.mlp_norm = nn.LayerNorm(width, eps=1e-6)
        self.mlp = nn.Sequential(
            nn.Linear(width, hidden), nn.GELU(), nn.Linear(hidden, width)
        )

    def _spatial(self, x: torch.Tensor) -> torch.Tensor:
        batch, channels, frames, height, width = x.shape
        shift, window = self.shift, self.window
        if shift:
            x = F.pad(x, (shift, 0, shift, 0))
        padded_height, padded_width = x.shape[-2:]
        pad_height = (-padded_height) % window
        pad_width = (-padded_width) % window
        x = F.pad(x, (0, pad_width, 0, pad_height))
        full_height, full_width = x.shape[-2:]
        tokens = (
            x.permute(0, 2, 3, 4, 1)
            .reshape(
                batch * frames,
                full_height // window,
                window,
                full_width // window,
                window,
                channels,
            )
            .permute(0, 1, 3, 2, 4, 5)
            .reshape(-1, window * window, channels)
        )
        query = self.spatial_norm(tokens)
        tokens = tokens + self.spatial(query, query, query, need_weights=False)[0]
        x = (
            tokens.reshape(
                batch * frames,
                full_height // window,
                full_width // window,
                window,
                window,
                channels,
            )
            .permute(0, 1, 3, 2, 4, 5)
            .reshape(batch, frames, full_height, full_width, channels)
            .permute(0, 4, 1, 2, 3)
        )
        x = x[..., :padded_height, :padded_width]
        if shift:
            return x[..., shift : shift + height, shift : shift + width]
        return x[..., :height, :width]

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        x = self._spatial(x)
        batch, channels, frames, height, width = x.shape
        tokens = x.permute(0, 3, 4, 2, 1).reshape(
            batch * height * width, frames, channels
        )
        query = self.temporal_norm(tokens)
        tokens = tokens + self.temporal(query, query, query, need_weights=False)[0]
        x = tokens.reshape(batch, height, width, frames, channels).permute(0, 4, 3, 1, 2)
        x = x + self.local(F.silu(self.local_norm(x)))
        tokens = x.permute(0, 2, 3, 4, 1)
        return x + self.mlp(self.mlp_norm(tokens)).permute(0, 4, 1, 2, 3)


class H3LatentUpscalerV3(nn.Module):
    def __init__(self, base_config: UpscalerConfigV2, config: UpscalerConfigV3):
        super().__init__()
        self.base_config = base_config
        self.config = config
        self.base = H3LatentUpscalerV2(base_config)
        self.stem = nn.Conv3d(base_config.in_channels, config.width, 3, padding=1)
        self.blocks = nn.ModuleList(
            FactorizedBlock(
                config.width,
                config.heads,
                config.window,
                bool(index % 2),
                config.mlp_ratio,
            )
            for index in range(config.blocks)
        )
        self.norm = nn.GroupNorm(1, config.width, eps=1e-6)
        self.to_delta = nn.Conv3d(
            config.width, base_config.in_channels * 4, 3, padding=1
        )

    def forward(self, low: torch.Tensor) -> torch.Tensor:
        base = self.base(low)
        x = self.stem(low)
        for block in self.blocks:
            x = block(x)
        delta = spatial_pixel_shuffle_2x(
            self.to_delta(F.silu(self.norm(x))), self.base_config.in_channels
        )
        return base + delta


def build_upscaler(
    state_dict: Mapping[str, torch.Tensor], info: CheckpointInfo
) -> H3LatentUpscalerV3:
    """Instantiate the metadata-declared architecture and load every tensor strictly."""

    model = H3LatentUpscalerV3(info.base_config, info.config)
    try:
        model.load_state_dict(state_dict, strict=True)
    except RuntimeError as exc:
        raise ValueError(f"Checkpoint tensors do not match their declared architecture: {exc}") from exc
    return model.eval().requires_grad_(False)
