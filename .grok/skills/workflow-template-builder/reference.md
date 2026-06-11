# ComfyUI Node Reference (MooshieUI templates)

## Model loading

| class_type | Inputs | Outputs |
|------------|--------|---------|
| CheckpointLoaderSimple | ckpt_name | 0 MODEL, 1 CLIP, 2 VAE |
| LoraLoader | model, clip, lora_name, strength_model, strength_clip | 0 MODEL, 1 CLIP |
| VAELoader | vae_name | 0 VAE |
| UpscaleModelLoader | model_name | 0 UPSCALE_MODEL |

## Text / latent / sample / decode

| class_type | Notes |
|------------|--------|
| CLIPTextEncode | clip, text → CONDITIONING |
| EmptyLatentImage | width, height, batch_size |
| VAEEncode / VAEEncodeForInpaint / VAEEncodeTiled | pixels + vae (+ mask for inpaint) |
| KSampler | model, ±, latent, seed, steps, cfg, sampler_name, scheduler, denoise |
| VAEDecode / VAEDecodeTiled | samples, vae |

## Images

| class_type | Notes |
|------------|--------|
| LoadImage / LoadImageMask | |
| SaveImage | terminal (usually via mod.rs) |
| ImageUpscaleWithModel / ImageScaleBy | |

## Advanced

| class_type | Notes |
|------------|--------|
| ApplyTiledDiffusion | model, method, tile_* |
| CLIPSetLastLayer | clip, stop_at_clip_layer |
| ControlNetLoader / ControlNetApplyAdvanced | |

## Mooshie custom nodes

Defined in `src-tauri/src/comfyui/mooshie_nodes.py`, deployed at startup, verified via `/object_info` (`REQUIRED_MOOSHIE_NODE_CLASSES` in `comfyui/nodes.rs`). New node → **add-comfyui-node** skill.

| class_type | Notes |
|------------|--------|
| MooshieSaveImage | terminal output over WebSocket (no disk round-trip); appended by `finish_workflow` in mod.rs |
| MooshieFaceDetailer | YOLO face detect → crop → re-denoise → composite; appended by facefix chain |
| MooshieSegmentDetailer | CLIPSeg (free text) or YOLO (`yolo-<model>`) region detect → re-denoise → soft-mask composite; one per `<segment:...>` prompt tag (`templates/segment_detail.rs`) |
| MooshieSoftGuidance / MooshieSmartGuidance | guidance variants |
| NanoSaurLoader | NanoSaur checkpoint loader |

## Connection syntax

```rust
"model": [model_source.0, model_source.1],
"steps": params.steps,
"text": params.positive_prompt,
```

## Append chain

```rust
pub fn append_my_chain(
    result: &mut WorkflowResult,
    params: &GenerationParams,
    seed: i64,
) -> (String, u32) { /* ... */ }
```

LoRA chaining: thread `model_source` / `clip_source` through sequential `LoraLoader` nodes.
