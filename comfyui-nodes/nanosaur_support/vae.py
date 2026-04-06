import math

import torch
import torch.nn.functional as F
from torch import nn


def nonlinearity(x):
    return x * torch.sigmoid(x)


def Normalize(in_channels, num_groups=32):
    return nn.GroupNorm(num_groups=num_groups, num_channels=in_channels, eps=1e-6, affine=True)


def _feature_take_indices(num_features: int, indices=None):
    if indices is None:
        return list(range(num_features)), num_features - 1
    if isinstance(indices, int):
        if not 0 < indices <= num_features:
            raise AssertionError(f"last-n ({indices}) is out of range (1 to {num_features})")
        take_indices = [num_features - indices + i for i in range(indices)]
    else:
        take_indices = []
        for i in indices:
            idx = num_features + i if i < 0 else i
            if not 0 <= idx < num_features:
                raise AssertionError(f"feature index {idx} is out of range (0 to {num_features - 1})")
            take_indices.append(idx)
    return take_indices, max(take_indices)


def _rope_rotate_half(x: torch.Tensor) -> torch.Tensor:
    x1, x2 = x.chunk(2, dim=-1)
    return torch.cat([-x2, x1], dim=-1)


def _apply_rot_embed_cat(x: torch.Tensor, emb: torch.Tensor, half: bool = False) -> torch.Tensor:
    sin_emb, cos_emb = emb.chunk(2, dim=-1)
    if half:
        return x * cos_emb + _rope_rotate_half(x) * sin_emb
    return x * cos_emb + torch.stack([-x[..., 1::2], x[..., ::2]], dim=-1).reshape(x.shape) * sin_emb


def _make_coords_dinov3(
    height: int,
    width: int,
    normalize_coords: str = "separate",
    grid_indexing: str = "ij",
    grid_offset: float = 0.0,
    device: torch.device = torch.device("cpu"),
    dtype: torch.dtype = torch.float32,
) -> torch.Tensor:
    coords_h = torch.arange(0.5, height, device=device, dtype=torch.float32) + grid_offset
    coords_w = torch.arange(0.5, width, device=device, dtype=torch.float32) + grid_offset
    if normalize_coords == "max":
        h_denom = w_denom = float(max(height, width))
    elif normalize_coords == "min":
        h_denom = w_denom = float(min(height, width))
    elif normalize_coords == "separate":
        h_denom = float(height)
        w_denom = float(width)
    else:
        raise ValueError(f"Unknown normalize_coords: {normalize_coords}")
    coords_h = (coords_h / h_denom).to(dtype)
    coords_w = (coords_w / w_denom).to(dtype)
    if grid_indexing == "xy":
        grid_w, grid_h = torch.meshgrid(coords_w, coords_h, indexing="xy")
        coords = torch.stack([grid_h, grid_w], dim=-1)
    else:
        coords = torch.stack(torch.meshgrid(coords_h, coords_w, indexing="ij"), dim=-1)
    return 2.0 * coords.flatten(0, 1) - 1.0


class _RotaryEmbeddingDinoV3(nn.Module):
    def __init__(
        self,
        dim: int,
        temperature: float = 100.0,
        feat_shape=None,
        normalize_coords: str = "separate",
        grid_offset: float = 0.0,
        grid_indexing: str = "ij",
        rotate_half: bool = True,
        device=None,
        dtype=None,
    ):
        super().__init__()
        self.dim = dim
        self.temperature = float(temperature)
        self.feat_shape = feat_shape
        self.normalize_coords = normalize_coords
        self.grid_offset = grid_offset
        self.grid_indexing = grid_indexing
        self.rotate_half = rotate_half
        self.register_buffer("periods", torch.empty(dim // 4, device=device, dtype=dtype), persistent=False)
        self.register_buffer("pos_embed_cached", None, persistent=False)
        self._init_buffers()

    def _init_buffers(self) -> None:
        exponents = 2.0 * torch.arange(self.dim // 4, device="cpu", dtype=torch.float32) / (self.dim // 2)
        self.periods.copy_(self.temperature ** exponents)
        if self.feat_shape is not None:
            rope_embed = self._create_embed(self.feat_shape)
            self.pos_embed_cached = rope_embed

    def _create_embed(self, feat_shape) -> torch.Tensor:
        coords = _make_coords_dinov3(
            feat_shape[0],
            feat_shape[1],
            normalize_coords=self.normalize_coords,
            grid_indexing=self.grid_indexing,
            grid_offset=self.grid_offset,
        )
        coords = coords[:, :, None].to(device=self.periods.device, dtype=self.periods.dtype)
        angles = 2 * math.pi * coords / self.periods[None, None, :]
        angles = angles.flatten(1).tile(2)
        return torch.cat([torch.sin(angles), torch.cos(angles)], dim=-1)

    def get_embed(self, shape=None) -> torch.Tensor:
        if shape is not None:
            return self._create_embed(shape)
        if self.pos_embed_cached is None:
            raise AssertionError("feature shape must be cached on create")
        return self.pos_embed_cached


class _PatchEmbed(nn.Module):
    def __init__(self, img_size=256, patch_size=16, in_chans=3, embed_dim=768):
        super().__init__()
        self.patch_size = (patch_size, patch_size)
        self.img_size = (img_size, img_size)
        self.grid_size = (img_size // patch_size, img_size // patch_size)
        self.num_patches = self.grid_size[0] * self.grid_size[1]
        self.proj = nn.Conv2d(in_chans, embed_dim, kernel_size=patch_size, stride=patch_size)

    def feat_ratio(self, as_scalar=True):
        return max(self.patch_size) if as_scalar else self.patch_size

    def dynamic_feat_size(self, img_size: tuple[int, int]) -> tuple[int, int]:
        return img_size[0] // self.patch_size[0], img_size[1] // self.patch_size[1]

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        x = self.proj(x)
        return x.permute(0, 2, 3, 1).contiguous()


class _Mlp(nn.Module):
    def __init__(self, dim: int, hidden_features: int):
        super().__init__()
        self.fc1 = nn.Linear(dim, hidden_features)
        self.act = nn.GELU()
        self.drop1 = nn.Dropout(0.0)
        self.norm = nn.Identity()
        self.fc2 = nn.Linear(hidden_features, dim)
        self.drop2 = nn.Dropout(0.0)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        x = self.fc1(x)
        x = self.act(x)
        x = self.drop1(x)
        x = self.norm(x)
        x = self.fc2(x)
        return self.drop2(x)


class _EvaAttention(nn.Module):
    def __init__(self, dim: int, num_heads: int, num_prefix_tokens: int):
        super().__init__()
        self.num_heads = num_heads
        self.head_dim = dim // num_heads
        self.scale = self.head_dim ** -0.5
        self.num_prefix_tokens = num_prefix_tokens
        self.qkv = nn.Linear(dim, dim * 3, bias=False)
        self.q_norm = nn.Identity()
        self.k_norm = nn.Identity()
        self.attn_drop = nn.Dropout(0.0)
        self.norm = nn.Identity()
        self.proj = nn.Linear(dim, dim)
        self.proj_drop = nn.Dropout(0.0)

    def forward(self, x: torch.Tensor, rope: torch.Tensor | None = None) -> torch.Tensor:
        b, n, c = x.shape
        qkv = self.qkv(x).reshape(b, n, 3, self.num_heads, self.head_dim).permute(2, 0, 3, 1, 4)
        q, k, v = qkv.unbind(0)
        q = self.q_norm(q)
        k = self.k_norm(k)
        if rope is not None:
            npt = self.num_prefix_tokens
            q = torch.cat([q[:, :, :npt, :], _apply_rot_embed_cat(q[:, :, npt:, :], rope, half=True)], dim=2).type_as(v)
            k = torch.cat([k[:, :, :npt, :], _apply_rot_embed_cat(k[:, :, npt:, :], rope, half=True)], dim=2).type_as(v)
        if hasattr(F, "scaled_dot_product_attention"):
            x = F.scaled_dot_product_attention(q, k, v, dropout_p=0.0, is_causal=False)
        else:
            attn = (q * self.scale) @ k.transpose(-2, -1)
            attn = self.attn_drop(attn.softmax(dim=-1))
            x = attn @ v
        x = x.transpose(1, 2).reshape(b, n, c)
        x = self.norm(x)
        x = self.proj(x)
        return self.proj_drop(x)


class _EvaBlock(nn.Module):
    def __init__(self, dim: int, num_heads: int, mlp_ratio: float, num_prefix_tokens: int, init_values: float):
        super().__init__()
        self.norm1 = nn.LayerNorm(dim, eps=1e-5)
        self.attn = _EvaAttention(dim, num_heads=num_heads, num_prefix_tokens=num_prefix_tokens)
        self.gamma_1 = nn.Parameter(torch.full((dim,), init_values))
        self.drop_path1 = nn.Identity()
        self.norm2 = nn.LayerNorm(dim, eps=1e-5)
        self.mlp = _Mlp(dim, int(dim * mlp_ratio))
        self.gamma_2 = nn.Parameter(torch.full((dim,), init_values))
        self.drop_path2 = nn.Identity()

    def forward(self, x: torch.Tensor, rope: torch.Tensor | None = None) -> torch.Tensor:
        x = x + self.drop_path1(self.gamma_1 * self.attn(self.norm1(x), rope=rope))
        x = x + self.drop_path2(self.gamma_2 * self.mlp(self.norm2(x)))
        return x


class _EvaDinoV3(nn.Module):
    def __init__(self):
        super().__init__()
        img_size = 256
        patch_size = 16
        embed_dim = 768
        depth = 12
        num_heads = 12
        self.embed_dim = embed_dim
        self.num_prefix_tokens = 5
        self.patch_embed = _PatchEmbed(img_size=img_size, patch_size=patch_size, in_chans=3, embed_dim=embed_dim)
        self.cls_token = nn.Parameter(torch.empty(1, 1, embed_dim))
        self.reg_token = nn.Parameter(torch.empty(1, 4, embed_dim))
        self.pos_drop = nn.Dropout(0.0)
        self.rope = _RotaryEmbeddingDinoV3(
            dim=embed_dim // num_heads,
            temperature=100.0,
            feat_shape=self.patch_embed.grid_size,
            rotate_half=True,
        )
        self.norm_pre = nn.Identity()
        self.blocks = nn.ModuleList(
            [_EvaBlock(embed_dim, num_heads=num_heads, mlp_ratio=4.0, num_prefix_tokens=self.num_prefix_tokens, init_values=1.0e-5) for _ in range(depth)]
        )
        self.norm = nn.Identity()
        self.fc_norm = nn.Identity()
        self.head = nn.Identity()

    def prune_intermediate_layers(self, indices=1, prune_norm: bool = False, prune_head: bool = True):
        take_indices, max_index = _feature_take_indices(len(self.blocks), indices)
        self.blocks = self.blocks[: max_index + 1]
        if prune_norm:
            self.norm = nn.Identity()
        if prune_head:
            self.fc_norm = nn.Identity()
            self.head = nn.Identity()
        return take_indices

    def _pos_embed(self, x: torch.Tensor) -> tuple[torch.Tensor, torch.Tensor]:
        b, h, w, c = x.shape
        x = x.view(b, -1, c)
        x = torch.cat(
            [
                self.cls_token.expand(b, -1, -1),
                self.reg_token.expand(b, -1, -1),
                x,
            ],
            dim=1,
        )
        return self.pos_drop(x), self.rope.get_embed(shape=(h, w))

    def forward_intermediates(
        self,
        x: torch.Tensor,
        indices=None,
        norm: bool = False,
        output_fmt: str = "NCHW",
        intermediates_only: bool = False,
    ):
        if output_fmt != "NCHW":
            raise ValueError("Only NCHW output is supported.")
        take_indices, _ = _feature_take_indices(len(self.blocks), indices)
        b, _, height, width = x.shape
        x = self.patch_embed(x)
        x, rot_pos_embed = self._pos_embed(x)
        x = self.norm_pre(x)
        intermediates = []
        for i, blk in enumerate(self.blocks):
            x = blk(x, rope=rot_pos_embed)
            if i in take_indices:
                intermediates.append(self.norm(x) if norm else x)
        if self.num_prefix_tokens:
            intermediates = [y[:, self.num_prefix_tokens :] for y in intermediates]
        h, w = self.patch_embed.dynamic_feat_size((height, width))
        intermediates = [y.reshape(b, h, w, -1).permute(0, 3, 1, 2).contiguous() for y in intermediates]
        if intermediates_only:
            return intermediates
        return self.norm(x), intermediates


class _FeatureGetterNet(nn.Module):
    def __init__(self, model: nn.Module, out_indices=(11,)):
        super().__init__()
        self.model = model
        if hasattr(model, "prune_intermediate_layers"):
            out_indices = model.prune_intermediate_layers(out_indices, prune_norm=True)
        self.out_indices = out_indices

    def forward(self, x: torch.Tensor):
        return self.model.forward_intermediates(
            x,
            indices=self.out_indices,
            norm=False,
            output_fmt="NCHW",
            intermediates_only=True,
        )


def _create_dinov3_model(model_name: str, features_only: bool = False, out_indices=(11,)) -> nn.Module:
    model_base = model_name.split(".", 1)[0]
    if model_base != "vit_base_patch16_dinov3":
        raise ValueError(f"Unsupported NanoSaur DINOv3 model {model_name!r}")
    model = _EvaDinoV3()
    if features_only:
        return _FeatureGetterNet(model, out_indices=out_indices)
    return model


class Upsample(nn.Module):
    def __init__(self, in_channels, with_conv):
        super().__init__()
        self.with_conv = with_conv
        if with_conv:
            self.conv = nn.Conv2d(in_channels, in_channels, kernel_size=3, stride=1, padding=1)

    def forward(self, x):
        x = torch.nn.functional.interpolate(x, scale_factor=2.0, mode="nearest")
        if self.with_conv:
            x = self.conv(x)
        return x


class ResnetBlock(nn.Module):
    def __init__(self, *, in_channels, out_channels=None, conv_shortcut=False, dropout=0.0, temb_channels=512):
        super().__init__()
        self.in_channels = in_channels
        self.out_channels = in_channels if out_channels is None else out_channels
        self.use_conv_shortcut = conv_shortcut
        self.norm1 = Normalize(in_channels)
        self.conv1 = nn.Conv2d(in_channels, self.out_channels, kernel_size=3, stride=1, padding=1)
        if temb_channels > 0:
            self.temb_proj = nn.Linear(temb_channels, self.out_channels)
        self.norm2 = Normalize(self.out_channels)
        self.dropout = nn.Dropout(dropout)
        self.conv2 = nn.Conv2d(self.out_channels, self.out_channels, kernel_size=3, stride=1, padding=1)
        if self.in_channels != self.out_channels:
            if self.use_conv_shortcut:
                self.conv_shortcut = nn.Conv2d(in_channels, self.out_channels, kernel_size=3, stride=1, padding=1)
            else:
                self.nin_shortcut = nn.Conv2d(in_channels, self.out_channels, kernel_size=1, stride=1, padding=0)

    def forward(self, x, temb):
        h = self.conv1(nonlinearity(self.norm1(x)))
        if temb is not None:
            h = h + self.temb_proj(nonlinearity(temb))[:, :, None, None]
        h = self.conv2(self.dropout(nonlinearity(self.norm2(h))))
        if self.in_channels != self.out_channels:
            x = self.conv_shortcut(x) if self.use_conv_shortcut else self.nin_shortcut(x)
        return x + h


class AttnBlock(nn.Module):
    def __init__(self, in_channels):
        super().__init__()
        self.norm = Normalize(in_channels)
        self.q = nn.Conv2d(in_channels, in_channels, kernel_size=1, stride=1, padding=0)
        self.k = nn.Conv2d(in_channels, in_channels, kernel_size=1, stride=1, padding=0)
        self.v = nn.Conv2d(in_channels, in_channels, kernel_size=1, stride=1, padding=0)
        self.proj_out = nn.Conv2d(in_channels, in_channels, kernel_size=1, stride=1, padding=0)

    def forward(self, x):
        h_ = self.norm(x)
        q = self.q(h_)
        k = self.k(h_)
        v = self.v(h_)
        b, c, h, w = q.shape
        q = q.reshape(b, c, h * w).permute(0, 2, 1)
        k = k.reshape(b, c, h * w)
        w_ = torch.nn.functional.softmax(torch.bmm(q, k) * (int(c) ** -0.5), dim=2)
        v = v.reshape(b, c, h * w)
        h_ = torch.bmm(v, w_.permute(0, 2, 1)).reshape(b, c, h, w)
        return x + self.proj_out(h_)


def make_attn(in_channels, attn_type="vanilla"):
    if attn_type == "vanilla":
        return AttnBlock(in_channels)
    if attn_type == "none":
        return nn.Identity()
    raise ValueError(f"Unknown attn_type {attn_type}")


class Decoder(nn.Module):
    def __init__(
        self,
        *,
        ch,
        out_ch,
        ch_mult=(1, 2, 4, 8),
        num_res_blocks,
        attn_resolutions,
        dropout=0.0,
        resamp_with_conv=True,
        in_channels,
        resolution,
        z_channels,
        give_pre_end=False,
        tanh_out=False,
        use_linear_attn=False,
        attn_type="vanilla",
        **ignorekwargs,
    ):
        super().__init__()
        if use_linear_attn:
            attn_type = "linear"
        self.num_resolutions = len(ch_mult)
        self.num_res_blocks = num_res_blocks
        self.give_pre_end = give_pre_end
        self.tanh_out = tanh_out
        block_in = ch * ch_mult[self.num_resolutions - 1]
        curr_res = resolution // 2 ** (self.num_resolutions - 1)
        self.conv_in = nn.Conv2d(z_channels, block_in, kernel_size=3, stride=1, padding=1)
        self.mid = nn.Module()
        self.mid.block_1 = ResnetBlock(in_channels=block_in, out_channels=block_in, temb_channels=0, dropout=dropout)
        self.mid.attn_1 = make_attn(block_in, attn_type=attn_type)
        self.mid.block_2 = ResnetBlock(in_channels=block_in, out_channels=block_in, temb_channels=0, dropout=dropout)
        self.up = nn.ModuleList()
        for i_level in reversed(range(self.num_resolutions)):
            block = nn.ModuleList()
            attn = nn.ModuleList()
            block_out = ch * ch_mult[i_level]
            for _ in range(self.num_res_blocks + 1):
                block.append(ResnetBlock(in_channels=block_in, out_channels=block_out, temb_channels=0, dropout=dropout))
                block_in = block_out
                if curr_res in attn_resolutions:
                    attn.append(make_attn(block_in, attn_type=attn_type))
            up = nn.Module()
            up.block = block
            up.attn = attn
            if i_level != 0:
                up.upsample = Upsample(block_in, resamp_with_conv)
                curr_res *= 2
            self.up.insert(0, up)
        self.norm_out = Normalize(block_in)
        self.conv_out = nn.Conv2d(block_in, out_ch, kernel_size=3, stride=1, padding=1)

    def forward(self, z):
        h = self.conv_in(z)
        h = self.mid.block_1(h, None)
        h = self.mid.attn_1(h)
        h = self.mid.block_2(h, None)
        for i_level in reversed(range(self.num_resolutions)):
            for i_block in range(self.num_res_blocks + 1):
                h = self.up[i_level].block[i_block](h, None)
                if len(self.up[i_level].attn) > 0:
                    h = self.up[i_level].attn[i_block](h)
            if i_level != 0:
                h = self.up[i_level].upsample(h)
        if self.give_pre_end:
            return h
        h = self.conv_out(nonlinearity(self.norm_out(h)))
        return torch.tanh(h) if self.tanh_out else h


class TransformerBlock(nn.Module):
    def __init__(self, dim: int, num_heads: int = 8, mlp_ratio: float = 4.0, dropout: float = 0.0):
        super().__init__()
        self.norm1 = nn.LayerNorm(dim)
        self.attn = nn.MultiheadAttention(dim, num_heads, dropout=dropout, batch_first=True)
        self.norm2 = nn.LayerNorm(dim)
        mlp_hidden = int(dim * mlp_ratio)
        self.mlp = nn.Sequential(
            nn.Linear(dim, mlp_hidden),
            nn.GELU(),
            nn.Dropout(dropout),
            nn.Linear(mlp_hidden, dim),
            nn.Dropout(dropout),
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        x_norm = self.norm1(x)
        attn_out, _ = self.attn(x_norm, x_norm, x_norm)
        x = x + attn_out
        return x + self.mlp(self.norm2(x))


class SemanticEncoder(nn.Module):
    def __init__(self, in_dim: int = 768, latent_dim: int = 96, num_blocks: int = 3, num_heads: int = 8):
        super().__init__()
        self.in_proj = nn.Linear(in_dim, latent_dim)
        self.blocks = nn.ModuleList([TransformerBlock(latent_dim, num_heads=num_heads) for _ in range(num_blocks)])
        self.out_proj = nn.Linear(latent_dim, latent_dim)
        self.norm = nn.LayerNorm(latent_dim)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        x = self.in_proj(x)
        for block in self.blocks:
            x = block(x)
        return self.out_proj(self.norm(x))


class PixelDecoder(nn.Module):
    def __init__(self, latent_dim: int = 96, out_channels: int = 3, out_size: int = 256):
        super().__init__()
        self.latent_dim = latent_dim
        self.decoder = Decoder(
            ch=128,
            out_ch=out_channels,
            ch_mult=(1, 1, 2, 2, 4),
            num_res_blocks=2,
            attn_resolutions=[16],
            dropout=0.0,
            resamp_with_conv=True,
            in_channels=out_channels,
            resolution=out_size,
            z_channels=latent_dim,
            give_pre_end=False,
            tanh_out=False,
            attn_type="vanilla",
        )

    def forward(self, z: torch.Tensor, spatial_hw: tuple = None) -> torch.Tensor:
        b, n, c = z.shape
        if spatial_hw is not None:
            h, w = spatial_hw
        else:
            h = w = int(n**0.5)
            assert h * w == n
        return self.decoder(z.permute(0, 2, 1).reshape(b, self.latent_dim, h, w))


class DINOv3Encoder(nn.Module):
    def __init__(self, model_name: str = "vit_base_patch16_dinov3.lvd1689m", pretrained: bool = True):
        super().__init__()
        self.model = _create_dinov3_model(model_name, features_only=True, out_indices=[11])
        self.embed_dim = 768

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        features = self.model(x)[0]
        b, c, h, w = features.shape
        return features.permute(0, 2, 3, 1).reshape(b, h * w, c)


class PSVAE(nn.Module):
    def __init__(self, latent_dim: int = 96, dino_model: str = "vit_base_patch16_dinov3.lvd1689m", pretrained: bool = True):
        super().__init__()
        self.dino_encoder = DINOv3Encoder(dino_model, pretrained)
        self.semantic_encoder = SemanticEncoder(in_dim=self.dino_encoder.embed_dim, latent_dim=latent_dim)
        self.pixel_decoder = PixelDecoder(latent_dim=latent_dim, out_size=256)

    def encode_dino(self, x: torch.Tensor) -> torch.Tensor:
        return self.dino_encoder(x)

    def encode_semantic(self, dino_features: torch.Tensor) -> torch.Tensor:
        return self.semantic_encoder(dino_features)

    def decode_pixel(self, z: torch.Tensor, spatial_hw: tuple = None) -> torch.Tensor:
        return self.pixel_decoder(z, spatial_hw=spatial_hw)


class NanoSaurVAE(nn.Module):
    def __init__(self, latent_dim: int = 96):
        super().__init__()
        self.dino_encoder = DINOv3Encoder(pretrained=False)
        self.semantic_encoder = SemanticEncoder(
            in_dim=self.dino_encoder.embed_dim,
            latent_dim=latent_dim,
        )
        self.pixel_decoder = PixelDecoder(latent_dim=latent_dim, out_size=256)
        self.img_mean = (0.485, 0.456, 0.406)
        self.img_std = (0.229, 0.224, 0.225)

    def _to_imagenet_norm(self, x: torch.Tensor) -> torch.Tensor:
        x = (x + 1.0) / 2.0
        img_mean = torch.tensor(self.img_mean, device=x.device, dtype=x.dtype).view(1, 3, 1, 1)
        img_std = torch.tensor(self.img_std, device=x.device, dtype=x.dtype).view(1, 3, 1, 1)
        return (x - img_mean) / img_std

    def _from_imagenet_norm(self, x: torch.Tensor) -> torch.Tensor:
        img_mean = torch.tensor(self.img_mean, device=x.device, dtype=x.dtype).view(1, 3, 1, 1)
        img_std = torch.tensor(self.img_std, device=x.device, dtype=x.dtype).view(1, 3, 1, 1)
        x = x * img_std + img_mean
        return x * 2.0 - 1.0

    @torch.no_grad()
    def encode(self, x: torch.Tensor) -> torch.Tensor:
        b, _, h, w = x.shape
        x_norm = self._to_imagenet_norm(x)
        dino_features = self.dino_encoder(x_norm)
        z = self.semantic_encoder(dino_features)
        return z.permute(0, 2, 1).reshape(b, -1, h // 16, w // 16)

    @torch.no_grad()
    def decode(self, z: torch.Tensor) -> torch.Tensor:
        b, c, h, w = z.shape
        z = z.reshape(b, c, h * w).permute(0, 2, 1)
        x_norm = self.pixel_decoder(z, spatial_hw=(h, w))
        return self._from_imagenet_norm(x_norm)
