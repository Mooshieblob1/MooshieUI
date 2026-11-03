# Image to 3D

The **3D** page turns one picture of a character or object into a 3D Gaussian
splat, back side included. It runs [Idol](https://huggingface.co/speridlabs/idol)
by Speridlabs (Apache 2.0), a fine-tune of TripoSplat, on the TripoSplat nodes
that ship with ComfyUI (v0.41 and newer). TripoSplat weights work too.

## Requirements

- ComfyUI v0.41 or newer (the managed runtime is pinned at v0.41.1).
- About 7 GB of VRAM. Idol's authors report 7.2 GB peak at the default settings.
- These files, which the page offers to download (about 3.8 GB in total) from
  Comfy-Org's ungated repackage. No Hugging Face token is needed.

| Folder | File | Size |
|--------|------|------|
| `models/diffusion_models` | `idol_fp16.safetensors` | 0.74 GB |
| `models/clip_vision` | `dino_v3_vit_h_bf16.safetensors` (DINOv3 ViT-H/16+) | 1.68 GB |
| `models/vae` | `idol_decoder_fp16.safetensors` | 0.58 GB |
| `models/vae` | `flux2-vae.safetensors` (shared with FLUX.2 / Klein) | 0.34 GB |
| `models/background_removal` | `birefnet.safetensors` (only for background removal) | 0.44 GB |

## Using it

1. Drop, paste or choose an image, pick one from the gallery, or press **3D**
   on a gallery image. One clear, centred subject on a plain background works
   best. Generated anime characters work well.
2. Keep **Remove background** on unless the image already has a transparent
   background. With it off, the image's alpha channel is used as the subject
   mask (an opaque image becomes a full-frame subject).
3. **Detail** sets the number of Gaussians (131,072 to 1,048,576). More is
   denser but not sharper, and costs memory and time.
4. **Advanced**: steps (default 20 as in ComfyUI's blueprint; Idol's reference
   is 50), guidance (default 3) and seed (-1 for random).

The viewer is a small bundled WebGL 2 splat renderer: drag to orbit, right-drag
or Shift-drag to pan, scroll or pinch to zoom, double-click to reset, arrow keys
work when it has focus. **Spin** toggles the turntable.

## Library and export

Finished models are stored per account in this browser's IndexedDB, like the
music library, so they are not shared between devices. **Save .ply** writes the
file ComfyUI produced (standard 3DGS PLY, SH degree 0). **Save .splat** writes
the compact 32-bytes-per-Gaussian format most web splat viewers open. Both open
in SuperSplat, Blender add-ons and other Gaussian splat tools. ComfyUI also
keeps its copy in `output/3d/`.

The splat uses the usual 3DGS convention (+Y down, camera looking along +Z),
so some tools show it upside down until you flip it.

## How it works

`src-tauri/src/templates/splat.rs` builds the graph from ComfyUI's
"Image to Gaussian Splat (TripoSplat)" blueprint: `LoadImage` →
`RemoveBackground` (BiRefNet) → `TripoSplatPreprocessImage` →
`TripoSplatConditioning` (DINOv3 + FLUX.2 VAE) → `KSampler` (dpmpp_2m, simple)
→ `VAEDecodeTripoSplat` → `SplatToFile3D` (ply) → `SaveGLB`.
`src-tauri/src/commands/splat.rs` picks a worker that has every node and file,
uploads the image to that worker, and reads the `.ply` back only from that
prompt's history. Jobs never enter the image gallery or image progress.

MCP: not exposed yet. The library lives in the client, so a server-side tool's
result would not reach the 3D page.
