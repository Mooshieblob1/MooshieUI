---
name: workflow-template-builder
description: >-
  Builds or modifies ComfyUI workflow JSON templates in MooshieUI's Rust backend
  (src-tauri/src/templates). Use for new generation modes, template nodes, LoRA chains,
  upscale append chains, or ComfyUI workflow wiring.
---

# Workflow Template Builder (MooshieUI)

Specialist workflow for `src-tauri/src/templates/`. Each template builds `serde_json::Map<String, Value>` for the ComfyUI API.

## Before coding

1. Read `src-tauri/src/templates/mod.rs`
2. Read `txt2img.rs` (reference pattern)
3. If new params needed → **add-generation-param** skill first
4. If a custom ComfyUI node is needed → **add-comfyui-node** skill first

## Rules

1. Return **`WorkflowResult`** with all fields set
2. Node IDs: `next_id.to_string()` after each node (`"1"`, `"2"`, …)
3. Use `json!{}` for nodes
4. Connections: `[node_id, port]` arrays from `(String, u32)` tuples
5. Register: `pub mod x;` + `build_workflow()` match arm in `mod.rs`
6. `SaveImage` is appended in `mod.rs`, not per-template

## WorkflowResult

```rust
pub struct WorkflowResult {
    pub workflow: serde_json::Map<String, Value>,
    pub next_id: u32,
    pub image_output: (String, u32),
    pub model_source: (String, u32),
    pub clip_source: (String, u32),
    pub positive_source: (String, u32),
    pub negative_source: (String, u32),
    pub vae_source: (String, u32),
    /// KSampler node ID — needed to rewire ± after ControlNet injection.
    pub sampler_id: String,
}
```

## Pipeline order (summary)

| Mode | Flow |
|------|------|
| txt2img | Checkpoint → LoRAs → VAE? → CLIP×2 → EmptyLatent → KSampler → VAEDecode |
| img2img | … → LoadImage → VAEEncode → KSampler (denoise < 1) → VAEDecode |
| inpainting | … → LoadImage + Mask → VAEEncodeForInpaint → KSampler → VAEDecode |
| upscale append | IMAGE → upscale → VAEEncodeTiled → KSampler → VAEDecodeTiled |

## finish_workflow chain (mod.rs)

Post-process steps append to the template's final IMAGE in **this fixed order**:

```
template image → upscale? → facefix? → segment refinement? → MooshieSaveImage
```

Seed offsets per step: `seed+2` facefix, `seed+3+i` per `<segment>` tag. New chains pick an unused offset.

## New template checklist

```
- [ ] src-tauri/src/templates/new_mode.rs
- [ ] pub fn build(params: &GenerationParams, seed: i64) -> WorkflowResult
- [ ] mod.rs: pub mod + build_workflow match
- [ ] cargo check --manifest-path src-tauri/Cargo.toml
```

## Reference

ComfyUI node tables, connection syntax, append-chain pattern: [reference.md](reference.md)


---

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
