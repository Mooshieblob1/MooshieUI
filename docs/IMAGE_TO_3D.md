# Image to 3D (experimental)

> **Experimental.** The page is hidden by default. Turn it on in
> **Settings → Appearance → Experimental: Image to 3D**. Results are often much
> rougher than Idol's published examples, especially for anime illustrations,
> scenes, and images that aren't a single object on a plain background.

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
   best, seen at a three-quarter angle like Idol's examples (figures, toys,
   props, food). Flat 2D artwork and busy scenes come out poorly.
2. Keep **Remove background** on unless the image already has a transparent
   background. With it off, the image's alpha channel is used as the subject
   mask (an opaque image becomes a full-frame subject).
3. **Detail** sets the number of Gaussians (131,072 to 1,048,576). More is
   denser but not sharper, and costs memory and time.
4. **Advanced**: steps (default 50), guidance (default 4) and seed (-1 for
   random). These match Idol's own sampler; see below.

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
`TripoSplatConditioning` (DINOv3 + FLUX.2 VAE) → `KSampler` (euler, simple)
→ `VAEDecodeTripoSplat` → `SplatToFile3D` (ply) → `SaveGLB`.
`src-tauri/src/commands/splat.rs` picks a worker that has every node and file,
uploads the image to that worker, and reads the `.ply` back only from that
prompt's history. Jobs never enter the image gallery or image progress.

### Sampling settings

ComfyUI's stock Idol template reuses TripoSplat's settings: 20 steps, cfg 3.0,
`dpmpp_2m`. Idol's own pipeline (`speridlabs/idol-sdk`, which made every
published example) uses 50 Euler steps, time shift 3.0 and guidance 3.0, but
writes guidance as `(1 + w) * cond - w * uncond`. In ComfyUI's
`uncond + cfg * (cond - uncond)` that is `cfg = 4.0`. The time shift needs no
node because ComfyUI's TripoSplat model config already sets `shift: 3.0`. So the
page defaults to 50 steps, cfg 4.0, `euler` and `simple`.

Idol also has a "real photo" mode that leaves more room around the subject
(crop 1.7 instead of 1.2). `TripoSplatPreprocessImage` always crops at 1.2,
which matches the synthetic mode the examples use.

MCP: not exposed yet. The library lives in the client, so a server-side tool's
result would not reach the 3D page.
