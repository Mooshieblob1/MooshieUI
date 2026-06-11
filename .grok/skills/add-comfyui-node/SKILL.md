---
name: add-comfyui-node
description: >-
  Adds a custom ComfyUI Python node to MooshieUI — Python class in mooshie_nodes.py,
  Rust required-class registration, and optional workflow template chain hookup.
  Use for new image-processing nodes (detailers, detectors, compositors) or /add-comfyui-node.
---

# Add Custom ComfyUI Node (MooshieUI)

MooshieUI deploys its own ComfyUI nodes from `src-tauri/src/comfyui/mooshie_nodes.py` (embedded via `include_str!`, written to ComfyUI's `custom_nodes/` at startup). Existing examples: `MooshieSaveImage`, `MooshieFaceDetailer`, `MooshieSegmentDetailer`.

## Touchpoints (in order)

### 1. Python — `src-tauri/src/comfyui/mooshie_nodes.py`

```python
class MooshieMyNode:
    """One-line summary of what the node does."""

    @classmethod
    def INPUT_TYPES(cls):
        return {
            "required": {
                "image": ("IMAGE",),
                "model": ("MODEL",),
                "vae": ("VAE",),
                "positive": ("CONDITIONING",),
                "negative": ("CONDITIONING",),
                "seed": ("INT", {"default": 0, "min": 0, "max": 0xFFFFFFFFFFFFFFFF}),
                # sampler/scheduler dropdowns:
                "sampler_name": (comfy.samplers.KSampler.SAMPLERS,),
                "scheduler": (comfy.samplers.KSampler.SCHEDULERS,),
            }
        }

    RETURN_TYPES = ("IMAGE",)
    FUNCTION = "process"
    CATEGORY = "mooshie"

    def process(self, image, ...):
        # IMAGE tensors are [B, H, W, C] float 0–1
        return (result,)
```

Register at the bottom of the file:

```python
NODE_CLASS_MAPPINGS = { ..., "MooshieMyNode": MooshieMyNode }
NODE_DISPLAY_NAME_MAPPINGS = { ..., "MooshieMyNode": "Mooshie My Node" }
```

### 2. Rust verification — `src-tauri/src/comfyui/nodes.rs`

Add the class name to `REQUIRED_MOOSHIE_NODE_CLASSES` so startup verifies ComfyUI actually loaded it (catches stale-server cases where files exist on disk but `/object_info` lacks the class until restart).

### 3. Workflow hookup — `src-tauri/src/templates/`

- New post-process chain → `append_*_chain(result, params, image, seed) -> (String, u32)` module, called from `finish_workflow` in `mod.rs`.
- **`finish_workflow` chain order matters**: upscale → facefix → segment → `MooshieSaveImage`. Insert new steps deliberately.
- Seed offsets: base seed for KSampler, `seed+2` facefix, `seed+3+i` segments. Pick an unused offset.

## Python node conventions

- Heavy imports (`transformers`, `ultralytics`) go **inside** methods, not module top — keeps node load cheap and the dependency optional.
- Model weights cache under `folder_paths.models_dir` subdirs (e.g. `models/clipseg/`); `del` model objects in `finally` to release VRAM.
- Sample with `comfy.sample.sample(...)` + `comfy.sample.prepare_noise`; preview via `latent_preview.prepare_callback`.
- Soft masks blend better than binary: return sigmoid/confidence values and let the composite weight per-pixel.
- Guard empty detections (`mask is None`, `ys.numel() == 0`) — return the input image unchanged, print a `[MooshieMyNode]` prefixed line for diagnosability (ComfyUI stdout, not Tauri logs).
- Python deps the node needs at runtime: ensure via `installPipPackage("pkg==x.y.z")` from the frontend before generation (see `ensureFacefixPythonDependency` in `GenerateButton.svelte`).

## Verify

```powershell
python -m py_compile src-tauri/src/comfyui/mooshie_nodes.py
cargo check --manifest-path src-tauri/Cargo.toml
```

```
- [ ] Class + NODE_CLASS_MAPPINGS + NODE_DISPLAY_NAME_MAPPINGS
- [ ] REQUIRED_MOOSHIE_NODE_CLASSES in nodes.rs
- [ ] finish_workflow / template chain wired (if workflow-facing)
- [ ] ComfyUI restarted when testing (deploy alone doesn't reload classes)
```
