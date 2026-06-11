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
