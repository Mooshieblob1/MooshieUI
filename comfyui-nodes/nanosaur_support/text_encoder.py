from __future__ import annotations

import re
from dataclasses import dataclass

from comfy import sd1_clip
from comfy.text_encoders.spiece_tokenizer import SPieceTokenizer
import comfy.model_management
import comfy.text_encoders.llama
import torch


TEXT_MAX_LENGTH = 128


@dataclass
class Gemma3_270M_Config:
    vocab_size: int = 262144
    hidden_size: int = 640
    intermediate_size: int = 2048
    num_hidden_layers: int = 18
    num_attention_heads: int = 4
    num_key_value_heads: int = 1
    max_position_embeddings: int = 32768
    rms_norm_eps: float = 1e-6
    rope_theta = [1000000.0, 10000.0]
    transformer_type: str = "gemma3"
    head_dim = 256
    rms_norm_add = True
    mlp_activation = "gelu_pytorch_tanh"
    qkv_bias = False
    rope_dims = None
    q_norm = "gemma3"
    k_norm = "gemma3"
    sliding_attention = [512, 512, 512, 512, 512, False]
    rope_scale = None
    final_norm: bool = True
    lm_head: bool = False
    stop_tokens = [1, 106]


class Gemma3_270M(
    comfy.text_encoders.llama.BaseLlama,
    comfy.text_encoders.llama.BaseGenerate,
    torch.nn.Module,
):
    def __init__(self, config_dict, dtype, device, operations):
        super().__init__()
        config = Gemma3_270M_Config(**config_dict)
        self.num_layers = config.num_hidden_layers
        self.model = comfy.text_encoders.llama.Llama2_(
            config,
            device=device,
            dtype=dtype,
            ops=operations,
        )
        self.dtype = dtype


def parse_prompt_emphasis(caption: str) -> tuple[str, list[tuple[int, int, float]]]:
    if not caption:
        return "", []

    weight_pattern = re.compile(r"[+-]?(?:\d+(?:\.\d+)?|\.\d+)$")
    spans: list[tuple[int, int, float]] = []
    parts: list[str] = []
    cursor = 0
    output_len = 0
    idx = 0
    while idx < len(caption):
        if caption[idx] != "(":
            idx += 1
            continue

        depth = 1
        end = idx + 1
        while end < len(caption) and depth > 0:
            if caption[end] == "(":
                depth += 1
            elif caption[end] == ")":
                depth -= 1
            end += 1

        if depth != 0:
            idx += 1
            continue

        inner = caption[idx + 1:end - 1]
        inner_depth = 0
        colon_idx = -1
        for inner_idx, char in enumerate(inner):
            if char == "(":
                inner_depth += 1
            elif char == ")":
                inner_depth -= 1
            elif char == ":" and inner_depth == 0:
                colon_idx = inner_idx

        if colon_idx == -1:
            idx += 1
            continue

        emphasized_text = inner[:colon_idx]
        weight_text = inner[colon_idx + 1:].strip()
        if not emphasized_text or not weight_pattern.fullmatch(weight_text):
            idx += 1
            continue

        literal = caption[cursor:idx]
        if literal:
            parts.append(literal)
            output_len += len(literal)

        span_start = output_len
        parts.append(emphasized_text)
        output_len += len(emphasized_text)
        spans.append((span_start, output_len, float(weight_text)))
        cursor = end
        idx = end

    tail = caption[cursor:]
    if tail:
        parts.append(tail)

    return "".join(parts), spans


def token_weight_for_span(token_begin: int, token_end: int, emphasis_spans: list[tuple[int, int, float]]) -> float:
    token_weight = 1.0
    for span_begin, span_end, span_weight in emphasis_spans:
        if token_begin < span_end and token_end > span_begin:
            token_weight *= span_weight
    return token_weight


class NanosaurGemma270MTokenizer(sd1_clip.SDTokenizer):
    def __init__(self, embedding_directory=None, tokenizer_data={}):
        tokenizer = tokenizer_data.get("spiece_model", None)
        special_tokens = {"<image_soft_token>": 262144, "<end_of_turn>": 106}
        self.offset_tokenizer = SPieceTokenizer.from_pretrained(tokenizer, add_bos=False, add_eos=False, special_tokens=special_tokens).tokenizer
        super().__init__(
            tokenizer,
            pad_with_end=False,
            embedding_size=640,
            embedding_key="nanosaur_gemma270m",
            tokenizer_class=SPieceTokenizer,
            has_end_token=False,
            pad_to_max_length=True,
            max_length=TEXT_MAX_LENGTH,
            min_length=TEXT_MAX_LENGTH,
            tokenizer_args={"add_bos": True, "add_eos": False, "special_tokens": special_tokens},
            disable_weights=True,
            tokenizer_data=tokenizer_data,
        )

    def tokenize_with_weights(self, text: str, return_word_ids=False, **kwargs):
        cleaned_text, emphasis_spans = parse_prompt_emphasis(text)
        proto = self.offset_tokenizer.encode_as_immutable_proto(cleaned_text)

        batch = []
        if self.start_token is not None:
            batch.append((self.start_token, 1.0, 0))

        for word_id, piece in enumerate(proto.pieces, start=1):
            token_weight = token_weight_for_span(piece.begin, piece.end, emphasis_spans)
            batch.append((piece.id, token_weight, word_id))

        if len(batch) > self.max_length:
            batch = batch[:self.max_length]
        if len(batch) < self.max_length:
            self.pad_tokens(batch, self.max_length - len(batch))

        if not return_word_ids:
            batch = [(token, weight) for token, weight, _ in batch]
        return [batch]

    def state_dict(self):
        return {"spiece_model": self.tokenizer.serialize_model()}


class NanoSaurTokenizer(sd1_clip.SD1Tokenizer):
    def __init__(self, embedding_directory=None, tokenizer_data={}):
        super().__init__(
            embedding_directory=embedding_directory,
            tokenizer_data=tokenizer_data,
            name="nanosaur_gemma270m",
            tokenizer=NanosaurGemma270MTokenizer,
        )


class NanosaurGemma270MModel(sd1_clip.SDClipModel):
    def __init__(self, device="cpu", layer="last", layer_idx=None, dtype=None, attention_mask=True, model_options={}):
        llama_quantization_metadata = model_options.get("llama_quantization_metadata", None)
        if llama_quantization_metadata is not None:
            model_options = model_options.copy()
            model_options["quantization_metadata"] = llama_quantization_metadata

        super().__init__(
            device=device,
            layer=layer,
            layer_idx=layer_idx,
            textmodel_json_config={},
            dtype=dtype,
            special_tokens={"start": 2, "pad": 0},
            layer_norm_hidden_state=False,
            model_class=Gemma3_270M,
            enable_attention_masks=attention_mask,
            return_attention_masks=attention_mask,
            model_options=model_options,
        )

    def encode_token_weights(self, token_weight_pairs):
        if isinstance(token_weight_pairs, dict):
            token_weight_pairs = token_weight_pairs["nanosaur_gemma270m"]
        tokens = [[token for token, _ in section] for section in token_weight_pairs]
        encoded = self.encode(tokens)
        out, pooled = encoded[:2]

        if pooled is not None:
            first_pooled = pooled[0:1].to(device=comfy.model_management.intermediate_device())
        else:
            first_pooled = pooled

        result = (out.to(device=comfy.model_management.intermediate_device()), first_pooled)
        extra = {}
        if len(encoded) > 2:
            for key, value in encoded[2].items():
                if key == "attention_mask":
                    value = value[: len(token_weight_pairs)].flatten().unsqueeze(dim=0).to(device=comfy.model_management.intermediate_device())
                extra[key] = value

        extra["token_weights"] = torch.tensor(
            [[weight for _, weight in section] for section in token_weight_pairs],
            device=comfy.model_management.intermediate_device(),
            dtype=torch.float32,
        ).flatten().unsqueeze(dim=0)

        return result + (extra,)


class NanoSaurClipModel(sd1_clip.SD1ClipModel):
    def __init__(self, device="cpu", dtype=None, model_options={}):
        super().__init__(
            device=device,
            dtype=dtype,
            name="nanosaur_gemma270m",
            clip_model=NanosaurGemma270MModel,
            model_options=model_options,
        )


def te(dtype_llama=None, llama_quantization_metadata=None):
    class NanoSaurTEModel_(NanoSaurClipModel):
        def __init__(self, device="cpu", dtype=None, model_options={}):
            if llama_quantization_metadata is not None:
                model_options = model_options.copy()
                model_options["quantization_metadata"] = llama_quantization_metadata
            if dtype_llama is not None:
                dtype = dtype_llama
            super().__init__(device=device, dtype=dtype, model_options=model_options)

    return NanoSaurTEModel_
