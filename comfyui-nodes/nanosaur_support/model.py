from functools import lru_cache
import math
from typing import Tuple

import torch
import torch.nn as nn
import torch.nn.functional as F
import comfy.patcher_extension
from einops import rearrange
from torch.nn.functional import scaled_dot_product_attention as attention

_compile_disable = torch.compiler.disable if hasattr(torch, "compiler") else torch._dynamo.disable


class Norm(nn.Module):
    def __init__(self, hidden_size, eps=1e-6):
        super().__init__()
        self.weight = nn.Parameter(torch.ones(hidden_size))
        self.variance_epsilon = eps

    def forward(self, hidden_states):
        input_dtype = hidden_states.dtype
        hidden_states = hidden_states.to(torch.float32)
        variance = hidden_states.pow(2).mean(-1, keepdim=True)
        hidden_states = hidden_states * torch.rsqrt(variance + self.variance_epsilon)
        return (self.weight * hidden_states).to(input_dtype)


class FeedForward(nn.Module):
    def __init__(self, dim: int, hidden_dim: int):
        super().__init__()
        self.w12 = nn.Linear(dim, hidden_dim * 2, bias=False)
        self.w3 = nn.Linear(hidden_dim, dim, bias=False)

    def forward(self, x):
        x1, x2 = self.w12(x).chunk(2, dim=-1)
        return self.w3(F.silu(x1) * x2)


class Embed(nn.Module):
    def __init__(self, in_chans: int, embed_dim: int, norm_layer=None, bias: bool = True):
        super().__init__()
        self.proj = nn.Linear(in_chans, embed_dim, bias=bias)
        self.norm = norm_layer(embed_dim) if norm_layer else nn.Identity()

    def forward(self, x):
        return self.norm(self.proj(x))


class TimestepEmbedder(nn.Module):
    def __init__(self, hidden_size, frequency_embedding_size=256):
        super().__init__()
        self.mlp = nn.Sequential(
            nn.Linear(frequency_embedding_size, hidden_size, bias=True),
            nn.SiLU(),
            nn.Linear(hidden_size, hidden_size, bias=True),
        )
        self.frequency_embedding_size = frequency_embedding_size

    @staticmethod
    def timestep_embedding(t, dim, max_period=10):
        half = dim // 2
        freqs = torch.exp(
            -math.log(max_period) * torch.arange(start=0, end=half, dtype=torch.float32, device=t.device) / half
        )
        args = t[..., None].float() * freqs[None, ...]
        embedding = torch.cat([torch.cos(args), torch.sin(args)], dim=-1)
        if dim % 2:
            embedding = torch.cat([embedding, torch.zeros_like(embedding[:, :1])], dim=-1)
        return embedding

    def forward(self, t):
        emb = self.timestep_embedding(t, self.frequency_embedding_size)
        return self.mlp(emb.to(dtype=self.mlp[0].weight.dtype))


def precompute_freqs_cis_2d(dim: int, height: int, width: int, theta: float = 10000.0, scale=1.0):
    if isinstance(scale, (float, int)):
        scale = (float(scale), float(scale))
    scale_y, scale_x = float(scale[0]), float(scale[1])

    rotary_dim = (dim // 4) * 4
    if rotary_dim == 0:
        return torch.empty(height * width, 0, 2, dtype=torch.float32)

    axis_dim = rotary_dim // 2
    inv_freq = 1.0 / (theta ** (torch.arange(0, axis_dim, 2, dtype=torch.float32) / axis_dim))
    y_pos = (torch.arange(height, dtype=torch.float32) + 0.5) / height * scale_y
    x_pos = (torch.arange(width, dtype=torch.float32) + 0.5) / width * scale_x
    y_pos, x_pos = torch.meshgrid(y_pos, x_pos, indexing="ij")
    y_pos = y_pos.reshape(-1)
    x_pos = x_pos.reshape(-1)

    x_freqs = torch.outer(x_pos, inv_freq)
    y_freqs = torch.outer(y_pos, inv_freq)
    cos = torch.cat([torch.cos(x_freqs), torch.cos(y_freqs)], dim=-1)
    sin = torch.cat([torch.sin(x_freqs), torch.sin(y_freqs)], dim=-1)
    return torch.stack((cos, sin), dim=-1)


def apply_rotary_emb(xq: torch.Tensor, xk: torch.Tensor, freqs_cis: torch.Tensor) -> Tuple[torch.Tensor, torch.Tensor]:
    cos, sin = freqs_cis.unbind(dim=-1)
    rotary_dim = cos.shape[-1] * 2
    if rotary_dim == 0:
        return xq, xk

    cos = cos[None, None, :, :].to(dtype=xq.dtype, device=xq.device)
    sin = sin[None, None, :, :].to(dtype=xq.dtype, device=xq.device)

    xq_rot, xq_pass = xq[..., :rotary_dim], xq[..., rotary_dim:]
    xk_rot, xk_pass = xk[..., :rotary_dim], xk[..., rotary_dim:]
    xq1, xq2 = xq_rot.chunk(2, dim=-1)
    xk1, xk2 = xk_rot.chunk(2, dim=-1)
    xq_rot = torch.cat([xq1 * cos - xq2 * sin, xq1 * sin + xq2 * cos], dim=-1)
    xk_rot = torch.cat([xk1 * cos - xk2 * sin, xk1 * sin + xk2 * cos], dim=-1)

    if xq_pass.shape[-1] == 0:
        return xq_rot, xk_rot
    return torch.cat([xq_rot, xq_pass], dim=-1), torch.cat([xk_rot, xk_pass], dim=-1)


def modulate(x, shift, scale):
    return x * (1 + scale) + shift


class LocalContext2D(nn.Module):
    def __init__(self, dim, num_layers):
        super().__init__()
        self.convs = nn.ModuleList(
            [nn.Conv2d(dim, dim, kernel_size=3, padding=1, groups=dim) for _ in range(num_layers)]
        )
        self.lambdas = nn.Parameter(0.1 * torch.ones(num_layers))

    def forward(self, x, layer_idx, h, w):
        b, n, d = x.shape
        x_2d = x.view(b, h, w, d).permute(0, 3, 1, 2)
        local = self.convs[layer_idx](x_2d).permute(0, 2, 3, 1).view(b, n, d)
        return x + self.lambdas[layer_idx] * local


class Attention(nn.Module):
    def __init__(self, dim: int, num_heads: int = 8, qkv_bias: bool = False, use_cross_attention: bool = True):
        super().__init__()
        assert dim % num_heads == 0
        self.num_heads = num_heads
        self.use_cross_attention = use_cross_attention
        self.qkv_x = nn.Linear(dim, dim * 3, bias=qkv_bias)
        if use_cross_attention:
            self.kv_y = nn.Linear(dim, dim * 2, bias=qkv_bias)
        self.q_norm = Norm(dim // num_heads)
        self.k_norm = Norm(dim // num_heads)
        self.proj = nn.Linear(dim, dim)

    def forward(self, x: torch.Tensor, y, pos, y_token_weights: torch.Tensor | None = None) -> torch.Tensor:
        b, n, c = x.shape
        qkv_x = self.qkv_x(x).reshape(b, n, 3, self.num_heads, c // self.num_heads).permute(2, 0, 3, 1, 4)
        q, kx, vx = qkv_x[0], qkv_x[1], qkv_x[2]
        q = self.q_norm(q)
        kx = self.k_norm(kx)
        q, kx = apply_rotary_emb(q, kx, freqs_cis=pos)
        if self.use_cross_attention:
            kv_y = self.kv_y(y).reshape(b, -1, 2, self.num_heads, c // self.num_heads).permute(2, 0, 3, 1, 4)
            ky, vy = kv_y[0], kv_y[1]
            ky = self.k_norm(ky)
            k = torch.cat([kx, ky], dim=2)
            v = torch.cat([vx, vy], dim=2)
        else:
            k = kx
            v = vx
        q = q.view(b, self.num_heads, -1, c // self.num_heads)
        k = k.view(b, self.num_heads, -1, c // self.num_heads)
        v = v.view(b, self.num_heads, -1, c // self.num_heads)
        if self.use_cross_attention and y_token_weights is not None:
            y_token_weights = y_token_weights.to(device=q.device, dtype=q.dtype)
            y_token_bias = torch.log(torch.clamp(y_token_weights, min=1e-4))
            x_token_bias = torch.zeros(b, n, device=q.device, dtype=q.dtype)
            attn_bias = torch.cat([x_token_bias, y_token_bias], dim=1)[:, None, None, :]
            x = attention(q, k, v, attn_mask=attn_bias)
        else:
            x = attention(q, k, v)
        return self.proj(x.transpose(1, 2).reshape(b, n, c))


class FlattenDiTBlock(nn.Module):
    def __init__(self, hidden_size, groups, mlp_ratio=4, is_encoder_block=False, use_cross_attention=True):
        super().__init__()
        self.norm1 = Norm(hidden_size, eps=1e-6)
        self.attn = Attention(hidden_size, num_heads=groups, qkv_bias=False, use_cross_attention=use_cross_attention)
        self.norm2 = Norm(hidden_size, eps=1e-6)
        self.mlp = FeedForward(hidden_size, int(hidden_size * mlp_ratio))
        self.is_encoder_block = is_encoder_block
        if not is_encoder_block:
            self.adaLN_modulation = nn.Sequential(nn.Linear(hidden_size, 6 * hidden_size, bias=True))

    def forward(self, x, y, c, pos, shared_ada_ln=None, local_context=None, layer_idx=None, h=None, w=None, y_token_weights: torch.Tensor | None = None):
        ada_ln_output = shared_ada_ln(c) if self.is_encoder_block else self.adaLN_modulation(c)
        if local_context is not None and h is not None and w is not None:
            x = local_context(x, layer_idx, h, w)
        shift_msa, scale_msa, gate_msa, shift_mlp, scale_mlp, gate_mlp = ada_ln_output.chunk(6, dim=-1)
        x = x + gate_msa * self.attn(modulate(self.norm1(x), shift_msa, scale_msa), y, pos, y_token_weights=y_token_weights)
        x = x + gate_mlp * self.mlp(modulate(self.norm2(x), shift_mlp, scale_mlp))
        return x


def precompute_freqs_cis_ex2d(dim: int, height: int, width: int, theta: float = 10000.0, scale=1.0):
    if isinstance(scale, float):
        scale = (scale, scale)
    x_pos = torch.linspace(0, height * scale[0], width)
    y_pos = torch.linspace(0, width * scale[1], height)
    y_pos, x_pos = torch.meshgrid(y_pos, x_pos, indexing="ij")
    y_pos = y_pos.reshape(-1)
    x_pos = x_pos.reshape(-1)
    freqs = 1.0 / (theta ** (torch.arange(0, dim, 4)[: (dim // 4)].float() / dim))
    x_freqs = torch.outer(x_pos, freqs).float()
    y_freqs = torch.outer(y_pos, freqs).float()
    x_cis = torch.polar(torch.ones_like(x_freqs), x_freqs)
    y_cis = torch.polar(torch.ones_like(y_freqs), y_freqs)
    return torch.cat([x_cis.unsqueeze(dim=-1), y_cis.unsqueeze(dim=-1)], dim=-1).reshape(height * width, -1)


class NerfEmbedder(nn.Module):
    def __init__(self, in_channels, hidden_size_input, max_freqs=8):
        super().__init__()
        self.max_freqs = max_freqs
        self.embedder = nn.Sequential(nn.Linear(in_channels + max_freqs**2, hidden_size_input, bias=True))

    @lru_cache
    def fetch_pos(self, patch_size, device, dtype):
        pos = precompute_freqs_cis_ex2d(self.max_freqs**2 * 2, patch_size, patch_size)
        return pos[None, :, :].to(device=device, dtype=dtype)

    @torch.compiler.disable
    def forward(self, inputs):
        batch, patch_tokens, _ = inputs.shape
        patch_size = int(patch_tokens**0.5)
        dct = self.fetch_pos(patch_size, inputs.device, inputs.dtype).repeat(batch, 1, 1)
        return self.embedder(torch.cat([inputs, dct], dim=-1))


class TextRefineAttention(nn.Module):
    def __init__(self, dim: int, num_heads: int = 8, qkv_bias: bool = False):
        super().__init__()
        assert dim % num_heads == 0
        self.num_heads = num_heads
        self.qkv = nn.Linear(dim, dim * 3, bias=qkv_bias)
        self.q_norm = Norm(dim // num_heads)
        self.k_norm = Norm(dim // num_heads)
        self.proj = nn.Linear(dim, dim)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        b, n, c = x.shape
        qkv_x = self.qkv(x).reshape(b, n, 3, self.num_heads, c // self.num_heads).permute(2, 0, 3, 1, 4)
        q, k, v = qkv_x[0], qkv_x[1], qkv_x[2]
        q = self.q_norm(q)
        k = self.k_norm(k)
        x = attention(
            q.view(b, self.num_heads, -1, c // self.num_heads),
            k.view(b, self.num_heads, -1, c // self.num_heads),
            v.view(b, self.num_heads, -1, c // self.num_heads),
        )
        return self.proj(x.transpose(1, 2).reshape(b, n, c))


class TextRefineBlock(nn.Module):
    def __init__(self, hidden_size, groups, mlp_ratio=4):
        super().__init__()
        self.norm1 = Norm(hidden_size, eps=1e-6)
        self.attn = TextRefineAttention(hidden_size, num_heads=groups, qkv_bias=False)
        self.norm2 = Norm(hidden_size, eps=1e-6)
        self.mlp = FeedForward(hidden_size, int(hidden_size * mlp_ratio))
        self.adaLN_modulation = nn.Sequential(nn.Linear(hidden_size, 6 * hidden_size, bias=True))

    def forward(self, x, c):
        shift_msa, scale_msa, gate_msa, shift_mlp, scale_mlp, gate_mlp = self.adaLN_modulation(c).chunk(6, dim=-1)
        x = x + gate_msa * self.attn(modulate(self.norm1(x), shift_msa, scale_msa))
        x = x + gate_mlp * self.mlp(modulate(self.norm2(x), shift_mlp, scale_mlp))
        return x


class ResBlock(nn.Module):
    def __init__(self, channels):
        super().__init__()
        self.in_ln = nn.LayerNorm(channels, eps=1e-6)
        self.mlp = nn.Sequential(
            nn.Linear(channels, channels, bias=True),
            nn.SiLU(),
            nn.Linear(channels, channels, bias=True),
        )
        self.adaLN_modulation = nn.Sequential(nn.SiLU(), nn.Linear(channels, 3 * channels, bias=True))

    def forward(self, x, y):
        shift_mlp, scale_mlp, gate_mlp = self.adaLN_modulation(y).chunk(3, dim=-1)
        return x + gate_mlp * self.mlp(modulate(self.in_ln(x), shift_mlp, scale_mlp))


class FinalLayer(nn.Module):
    def __init__(self, model_channels, out_channels):
        super().__init__()
        self.norm_final = nn.LayerNorm(model_channels, elementwise_affine=False, eps=1e-6)
        self.linear = nn.Linear(model_channels, out_channels, bias=True)

    def forward(self, x):
        return self.linear(self.norm_final(x))


class SimpleMLPAdaLN(nn.Module):
    def __init__(self, in_channels, model_channels, out_channels, z_channels, num_res_blocks, patch_size):
        super().__init__()
        self.patch_size = patch_size
        self.cond_embed = nn.Linear(z_channels, patch_size**2 * model_channels)
        self.input_proj = nn.Linear(in_channels, model_channels)
        self.res_blocks = nn.ModuleList([ResBlock(model_channels) for _ in range(num_res_blocks)])
        self.final_layer = FinalLayer(model_channels, out_channels)

    def forward(self, x, c):
        x = self.input_proj(x)
        y = self.cond_embed(c).reshape(c.shape[0], self.patch_size**2, -1)
        for block in self.res_blocks:
            x = block(x, y)
        return self.final_layer(x)


class NanoSaurTransformer2DModel(nn.Module):
    def __init__(
        self,
        in_channels=96,
        num_groups=16,
        hidden_size=1536,
        decoder_hidden_size=2048,
        num_encoder_blocks=26,
        num_decoder_blocks=3,
        num_text_blocks=2,
        patch_size=1,
        txt_embed_dim=640,
        sprint_num_f=2,
        sprint_num_h=2,
        rope_scale=2 * math.pi,
        device=None,
        dtype=None,
        operations=None,
        **kwargs,
    ):
        super().__init__()
        del operations, kwargs
        assert (hidden_size // num_groups) % 4 == 0
        self.in_channels = in_channels
        self.hidden_size = hidden_size
        self.num_groups = num_groups
        self.patch_size = patch_size
        self.sprint_num_f = sprint_num_f
        self.sprint_num_h = sprint_num_h
        self.sprint_num_g = num_encoder_blocks - sprint_num_f - sprint_num_h
        self.rope_scale = rope_scale
        self.mask_token = nn.Parameter(torch.zeros(1, 1, hidden_size, device=device))
        self.fusion_proj = nn.Linear(2 * hidden_size, hidden_size, bias=True, device=device)
        self.s_embedder = Embed(in_channels * patch_size**2, hidden_size, bias=True)
        self.x_embedder = NerfEmbedder(in_channels, decoder_hidden_size, max_freqs=8)
        self.t_embedder = TimestepEmbedder(hidden_size)
        self.y_embedder = Embed(txt_embed_dim, hidden_size, bias=True, norm_layer=Norm)
        self.shared_encoder_adaLN = nn.Sequential(nn.Linear(hidden_size, 6 * hidden_size, bias=True, device=device))
        self.blocks = nn.ModuleList(
            [
                FlattenDiTBlock(hidden_size, num_groups, is_encoder_block=True, use_cross_attention=(i % 2 == 0))
                for i in range(num_encoder_blocks)
            ]
        )
        self.text_refine_blocks = nn.ModuleList([TextRefineBlock(hidden_size, num_groups) for _ in range(num_text_blocks)])
        self.local_context = LocalContext2D(hidden_size, num_encoder_blocks)
        self.dec_net = SimpleMLPAdaLN(
            in_channels=decoder_hidden_size,
            model_channels=decoder_hidden_size,
            out_channels=in_channels,
            z_channels=hidden_size,
            num_res_blocks=num_decoder_blocks,
            patch_size=patch_size,
        )
        self.precompute_pos = {}
        if dtype is not None:
            self.to(dtype=dtype)

    @property
    def dtype(self):
        return self.s_embedder.proj.weight.dtype

    @property
    def device(self):
        return self.s_embedder.proj.weight.device

    @_compile_disable
    def fetch_pos(self, height, width, device):
        key = (int(height), int(width))
        pos = self.precompute_pos.get(key)
        if pos is None:
            pos = precompute_freqs_cis_2d(self.hidden_size // self.num_groups, key[0], key[1], scale=self.rope_scale)
            self.precompute_pos[key] = pos
        return pos.to(device=device)

    def _sprint_fuse(self, s_enc, g_pad):
        return self.fusion_proj(torch.cat([s_enc, g_pad], dim=-1))

    def _forward(self, x: torch.Tensor, timesteps: torch.Tensor, context: torch.Tensor, uncond: bool = False, token_weights: torch.Tensor | None = None) -> torch.Tensor:
        device = self.s_embedder.proj.weight.device
        embed_dtype = self.s_embedder.proj.weight.dtype
        if context.device != device:
            context = context.to(device)
        y_emb = self.y_embedder(context).view(context.size(0), -1, self.hidden_size).to(embed_dtype)
        batch, _, height, width = x.shape
        x_tokens = rearrange(x, "b c (h p1) (w p2) -> b (h w) (c p1 p2)", p1=self.patch_size, p2=self.patch_size)
        xpos = self.fetch_pos(height // self.patch_size, width // self.patch_size, x.device)
        t_emb = self.t_embedder(timesteps.view(-1)).view(batch, -1, self.hidden_size)
        condition = torch.nn.functional.silu(t_emb)
        y_latent = y_emb.to(dtype=t_emb.dtype)
        for block in self.text_refine_blocks:
            y_latent = block(y_latent, condition)

        s = self.s_embedder(x_tokens)
        h_patches, w_patches = height // self.patch_size, width // self.patch_size
        for i in range(self.sprint_num_f):
            s = self.blocks[i](
                s,
                y_latent,
                condition,
                xpos,
                shared_ada_ln=self.shared_encoder_adaLN,
                local_context=self.local_context,
                layer_idx=i,
                h=h_patches,
                w=w_patches,
                y_token_weights=token_weights,
            )
        s_enc = s
        s_sparse = s
        for i in range(self.sprint_num_f, self.sprint_num_f + self.sprint_num_g):
            if not uncond:
                s_sparse = self.blocks[i](s_sparse, y_latent, condition, xpos, shared_ada_ln=self.shared_encoder_adaLN, y_token_weights=token_weights)
        g_pad = self.mask_token.expand_as(s_sparse) if uncond else s_sparse
        s = self._sprint_fuse(s_enc, g_pad)
        for i in range(self.sprint_num_f + self.sprint_num_g, len(self.blocks)):
            s = self.blocks[i](
                s,
                y_latent,
                condition,
                xpos,
                shared_ada_ln=self.shared_encoder_adaLN,
                local_context=self.local_context,
                layer_idx=i,
                h=h_patches,
                w=w_patches,
                y_token_weights=token_weights,
            )

        s = torch.nn.functional.silu(t_emb + s)
        batch_size, length, _ = s.shape
        x_dec = x_tokens.reshape(batch_size * length, self.in_channels, self.patch_size**2).transpose(1, 2)
        s_dec = s.view(batch_size * length, self.hidden_size)
        x_dec = self.x_embedder(x_dec)
        x_dec = self.dec_net(x_dec, s_dec).transpose(1, 2).reshape(batch_size, length, -1)
        return rearrange(
            x_dec,
            "b (h w) (c p1 p2) -> b c (h p1) (w p2)",
            h=height // self.patch_size,
            w=width // self.patch_size,
            p1=self.patch_size,
            p2=self.patch_size,
            c=self.in_channels,
        )

    def forward(self, x, timestep, context=None, control=None, transformer_options=None, **kwargs):
        del control
        if context is None:
            raise ValueError("NanoSaurTransformer2DModel requires text context.")
        x0 = comfy.patcher_extension.WrapperExecutor.new_class_executor(
            self._forward,
            self,
            comfy.patcher_extension.get_all_wrappers(
                comfy.patcher_extension.WrappersMP.DIFFUSION_MODEL,
                transformer_options or {},
            ),
        ).execute(x, timestep, context, **kwargs)
        return (x - x0) / timestep.view(-1, 1, 1, 1)
