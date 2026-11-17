"""ChatGLM3-6B text encoder for Kolors, written against ComfyUI's own ops.

Kolors conditions its UNet on ChatGLM3-6B instead of CLIP. Core ComfyUI has
no ChatGLM, and the community packs that add it wrap THUDM's remote modelling
code, which leans on transformers internals that keep moving. This is a
small, self-contained re-implementation of the encoder forward pass only (no
generation, no KV cache) on top of `comfy.ops`, so the encoder loads through
`comfy.sd.CLIP` like any other text encoder: it gets ComfyUI's memory
management, offloading and partial loading, and works with the stock
CLIPTextEncode node.

What Kolors feeds the UNet (KolorsPipeline.encode_prompt):
  * prompt embeds: hidden states after layer 27 of 28 (hidden_states[-2]),
    for all 256 positions, padding included.
  * pooled: hidden states after layer 28, before the final norm, at the last
    position (the last real token, since padding is on the left).
The tokenizer prepends [gMASK] and sop, truncates to 256 tokens and pads on
the left with id 0. Padded positions get position id 0, and the attention
mask follows ChatGLM's `get_masks`: real tokens attend causally to real
tokens, padded positions attend to everything.
"""

from __future__ import annotations

import os

import torch
import torch.nn.functional as F


from comfy import sd1_clip

MAX_LENGTH = 256
GMASK_TOKEN = 64790
SOP_TOKEN = 64792
PAD_TOKEN = 0
CLIP_NAME = "chatglm3"

# ChatGLM3's SentencePiece model, as Kwai-Kolors ships it. It is part of the
# Kolors release (Kolors model licence), so it is fetched next to the encoder
# weights on first use instead of being bundled with MooshieUI. Pinned to a
# revision and checked by SHA-256.
TOKENIZER_FILENAME = "kolors_chatglm3_tokenizer.model"
TOKENIZER_URL = (
    "https://huggingface.co/Kwai-Kolors/Kolors-diffusers/resolve/"
    "7e091c75199e910a26cd1b51ed52c28de5db3711/tokenizer/tokenizer.model"
)
TOKENIZER_SHA256 = "e7dc4c393423b76e4373e5157ddc34803a0189ba96b21ddbb40269d31468a6f2"
TOKENIZER_BYTES = 1018370


def _sha256(path):
    import hashlib

    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def ensure_tokenizer(encoder_dir):
    """Path to the ChatGLM3 tokenizer model, downloading it beside the encoder if needed."""
    for name in (TOKENIZER_FILENAME, "chatglm3_tokenizer.model", "tokenizer.model"):
        path = os.path.join(encoder_dir, name)
        if os.path.isfile(path) and _sha256(path) == TOKENIZER_SHA256:
            return path

    import urllib.request

    target = os.path.join(encoder_dir, TOKENIZER_FILENAME)
    partial = target + ".part"
    try:
        with urllib.request.urlopen(TOKENIZER_URL, timeout=60) as response, open(partial, "wb") as out:
            out.write(response.read(TOKENIZER_BYTES + 1))
        if _sha256(partial) != TOKENIZER_SHA256:
            raise RuntimeError("downloaded file does not match the pinned SHA-256")
        os.replace(partial, target)
    except Exception as e:
        if os.path.exists(partial):
            os.remove(partial)
        raise RuntimeError(
            "Kolors needs the ChatGLM3 tokenizer and could not download it ({}). Save {} as {}.".format(
                e, TOKENIZER_URL, target
            )
        )
    return target

CHATGLM3_CONFIG = {
    "vocab_size": 65024,
    "hidden_size": 4096,
    "ffn_hidden_size": 13696,
    "num_layers": 28,
    "num_attention_heads": 32,
    "multi_query_group_num": 2,
    "kv_channels": 128,
    "layernorm_epsilon": 1e-5,
}


def rope_cache(positions, rot_dim, device):
    """cos/sin table for ChatGLM's rotary embedding (interleaved pairs, base 10000)."""
    theta = 1.0 / (10000 ** (torch.arange(0, rot_dim, 2, dtype=torch.float32, device=device) / rot_dim))
    angles = positions.float().unsqueeze(-1) * theta  # [b, s, rot_dim / 2]
    return torch.cos(angles), torch.sin(angles)


def apply_rope(x, cos, sin):
    """x: [b, heads, s, head_dim]. Rotates the first half of each head, in (even, odd) pairs."""
    rot_dim = cos.shape[-1] * 2
    x_rot, x_pass = x[..., :rot_dim], x[..., rot_dim:]
    x_rot = x_rot.reshape(*x_rot.shape[:-1], rot_dim // 2, 2)
    cos = cos.unsqueeze(1).to(x.dtype)
    sin = sin.unsqueeze(1).to(x.dtype)
    x0, x1 = x_rot[..., 0], x_rot[..., 1]
    rotated = torch.stack((x0 * cos - x1 * sin, x1 * cos + x0 * sin), dim=-1).flatten(-2)
    return torch.cat((rotated, x_pass), dim=-1)


class SelfAttention(torch.nn.Module):
    def __init__(self, config, dtype, device, ops):
        super().__init__()
        self.num_heads = config["num_attention_heads"]
        self.num_groups = config["multi_query_group_num"]
        self.head_dim = config["kv_channels"]
        projection = self.num_heads * self.head_dim
        qkv = projection + 2 * self.num_groups * self.head_dim
        self.query_key_value = ops.Linear(config["hidden_size"], qkv, bias=True, dtype=dtype, device=device)
        self.dense = ops.Linear(projection, config["hidden_size"], bias=False, dtype=dtype, device=device)

    def forward(self, x, mask, cos, sin):
        b, s, _ = x.shape
        qkv = self.query_key_value(x)
        q, k, v = qkv.split(
            [self.num_heads * self.head_dim, self.num_groups * self.head_dim, self.num_groups * self.head_dim],
            dim=-1,
        )
        q = q.view(b, s, self.num_heads, self.head_dim).transpose(1, 2)
        k = k.view(b, s, self.num_groups, self.head_dim).transpose(1, 2)
        v = v.view(b, s, self.num_groups, self.head_dim).transpose(1, 2)
        q = apply_rope(q, cos, sin)
        k = apply_rope(k, cos, sin)
        repeat = self.num_heads // self.num_groups
        k = k.repeat_interleave(repeat, dim=1)
        v = v.repeat_interleave(repeat, dim=1)
        out = F.scaled_dot_product_attention(q, k, v, attn_mask=mask)
        out = out.transpose(1, 2).reshape(b, s, self.num_heads * self.head_dim)
        return self.dense(out)


class MLP(torch.nn.Module):
    def __init__(self, config, dtype, device, ops):
        super().__init__()
        self.dense_h_to_4h = ops.Linear(config["hidden_size"], config["ffn_hidden_size"] * 2, bias=False, dtype=dtype, device=device)
        self.dense_4h_to_h = ops.Linear(config["ffn_hidden_size"], config["hidden_size"], bias=False, dtype=dtype, device=device)

    def forward(self, x):
        gate, up = self.dense_h_to_4h(x).chunk(2, dim=-1)
        return self.dense_4h_to_h(F.silu(gate) * up)


class GLMBlock(torch.nn.Module):
    def __init__(self, config, dtype, device, ops):
        super().__init__()
        eps = config["layernorm_epsilon"]
        self.input_layernorm = ops.RMSNorm(config["hidden_size"], eps=eps, dtype=dtype, device=device)
        self.self_attention = SelfAttention(config, dtype, device, ops)
        self.post_attention_layernorm = ops.RMSNorm(config["hidden_size"], eps=eps, dtype=dtype, device=device)
        self.mlp = MLP(config, dtype, device, ops)

    def forward(self, x, mask, cos, sin):
        x = x + self.self_attention(self.input_layernorm(x), mask, cos, sin)
        return x + self.mlp(self.post_attention_layernorm(x))


class Embedding(torch.nn.Module):
    def __init__(self, config, dtype, device, ops):
        super().__init__()
        self.word_embeddings = ops.Embedding(config["vocab_size"], config["hidden_size"], dtype=dtype, device=device)


class Encoder(torch.nn.Module):
    def __init__(self, config, dtype, device, ops):
        super().__init__()
        self.layers = torch.nn.ModuleList([GLMBlock(config, dtype, device, ops) for _ in range(config["num_layers"])])
        self.final_layernorm = ops.RMSNorm(config["hidden_size"], eps=config["layernorm_epsilon"], dtype=dtype, device=device)


def chatglm_attention_mask(padding_mask):
    """Boolean [b, 1, s, s] mask, True = may attend. Mirrors ChatGLMModel.get_masks."""
    pad = padding_mask.to(torch.float32)
    s = pad.shape[-1]
    full = torch.ones(pad.shape[0], s, s, device=pad.device).tril_()
    full = full * pad.unsqueeze(1)
    full = full - (pad.unsqueeze(-1) - 1)
    return (full >= 0.5).unsqueeze(1)


class ChatGLM3Model(torch.nn.Module):
    """ChatGLMModel without the LM head. State dict keys match Kolors' text_encoder folder."""

    def __init__(self, config_dict, dtype, device, operations):
        super().__init__()
        config = {**CHATGLM3_CONFIG, **(config_dict or {})}
        self.config = config
        self.num_layers = config["num_layers"]
        self.embedding = Embedding(config, dtype, device, operations)
        self.encoder = Encoder(config, dtype, device, operations)
        self.dtype = dtype

    def get_input_embeddings(self):
        return self.embedding.word_embeddings

    def set_input_embeddings(self, embeddings):
        self.embedding.word_embeddings = embeddings

    def forward(self, input_ids, attention_mask=None, embeds=None, num_tokens=None, intermediate_output=None,
                final_layer_norm_intermediate=True, dtype=None, embeds_info=[], **kwargs):
        if embeds is not None:
            x = embeds
        else:
            x = self.embedding.word_embeddings(input_ids, out_dtype=dtype)
        b, s, _ = x.shape
        if attention_mask is None:
            attention_mask = torch.ones(b, s, device=x.device, dtype=torch.long)
        attention_mask = attention_mask.to(x.device)

        # Real tokens are numbered 0..n-1 after the left padding; padding gets 0.
        positions = (attention_mask.cumsum(-1) - 1).clamp(min=0)
        cos, sin = rope_cache(positions, self.config["kv_channels"] // 2, x.device)
        mask = chatglm_attention_mask(attention_mask)

        if intermediate_output is None:
            intermediate_output = -2
        if intermediate_output < 0:
            intermediate_output = self.num_layers + intermediate_output

        intermediate = None
        for i, layer in enumerate(self.encoder.layers):
            x = layer(x, mask, cos, sin)
            if i == intermediate_output:
                intermediate = x.clone()

        # Kolors' pooled vector is the last layer's output before the final
        # norm, taken at the last position.
        pooled = x[:, -1, :].clone()
        last = self.encoder.final_layernorm(x)
        return last, intermediate, pooled


class ChatGLM3Tokenizer:
    """Turns a prompt into ChatGLM3 ids the way Kolors' tokenizer does, keeping
    ComfyUI's (text:weight) emphasis syntax."""

    def __init__(self, embedding_directory=None, tokenizer_data={}):
        import sentencepiece

        path = tokenizer_data.get("chatglm3_tokenizer_path", None)
        if path is None:
            raise RuntimeError("The ChatGLM3 tokenizer path was not passed to the Kolors text encoder.")
        self.sp = sentencepiece.SentencePieceProcessor(model_file=path)
        self.max_length = MAX_LENGTH

    def tokenize_with_weights(self, text: str, return_word_ids=False, **kwargs):
        text = sd1_clip.escape_important(text)
        parsed = sd1_clip.token_weights(text, 1.0)
        tokens = []
        word_id = 1
        for segment, weight in parsed:
            segment = sd1_clip.unescape_important(segment)
            if not segment:
                continue
            for token in self.sp.encode(segment):
                tokens.append((token, weight, word_id))
            word_id += 1

        prefix = [(GMASK_TOKEN, 1.0, 0), (SOP_TOKEN, 1.0, 0)]
        tokens = prefix + tokens[: self.max_length - len(prefix)]
        tokens = [(PAD_TOKEN, 1.0, 0)] * (self.max_length - len(tokens)) + tokens
        if not return_word_ids:
            tokens = [(t, w) for t, w, _ in tokens]
        return {CLIP_NAME: [tokens]}

    def untokenize(self, token_weight_pair):
        return [(a, self.sp.id_to_piece(a[0]) if a[0] < self.sp.get_piece_size() else "") for a in token_weight_pair]

    def state_dict(self):
        return {}


class ChatGLM3ClipModel(sd1_clip.SDClipModel):
    def __init__(self, device="cpu", dtype=None, model_options={}):
        super().__init__(
            device=device,
            max_length=MAX_LENGTH,
            layer="hidden",
            layer_idx=-2,
            textmodel_json_config={},
            dtype=dtype,
            special_tokens={"pad": PAD_TOKEN},
            layer_norm_hidden_state=False,
            model_class=ChatGLM3Model,
            enable_attention_masks=True,
            return_projected_pooled=True,
            model_options=model_options,
        )

    def gen_empty_tokens(self, special_tokens, length):
        length = max(length, 2)
        return [PAD_TOKEN] * (length - 2) + [GMASK_TOKEN, SOP_TOKEN]


class KolorsTEModel(sd1_clip.SD1ClipModel):
    def __init__(self, device="cpu", dtype=None, model_options={}):
        super().__init__(device=device, dtype=dtype, name=CLIP_NAME, clip_model=ChatGLM3ClipModel, model_options=model_options)


def prepare_state_dict(sd):
    """Normalises a ChatGLM3 state dict to the keys ChatGLM3Model uses.

    Accepts Kolors' text_encoder weights (no prefix) and full
    ChatGLMForConditionalGeneration dumps (``transformer.`` prefix), and drops
    the LM head and rotary buffer the encoder never uses."""
    out = {}
    for key, value in sd.items():
        if key.startswith("transformer."):
            key = key[len("transformer."):]
        if key.startswith("output_layer.") or key.startswith("rotary_pos_emb."):
            continue
        out[key] = value
    if "embedding.word_embeddings.weight" not in out or "encoder.layers.0.self_attention.query_key_value.weight" not in out:
        raise RuntimeError(
            "This file does not look like a ChatGLM3 text encoder. Kolors needs the ChatGLM3-6B weights "
            "from Kwai-Kolors/Kolors-diffusers (text_encoder/model.fp16-0000N-of-00003.safetensors)."
        )
    if any(key.endswith("weight_scale") or key.endswith(".weight_scale_inv") for key in out):
        raise RuntimeError("Quantized (4-bit/8-bit) ChatGLM3 files are not supported. Use the fp16 weights.")
    return out
