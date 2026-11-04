# Image to 3D (experimental)

> **Experimental.** The page is hidden by default. Turn it on in
> **Settings → Appearance → Experimental: Image to 3D**. Both methods work best
> on a single object or figure on a plain background. Anime illustrations,
> scenes and flat 2D artwork come out much rougher than the published examples.

The **3D** page turns one picture of a character or object into a 3D model,
back side included. It has two methods, picked at the top of the form:

- **Textured mesh (Pixal3D)**, the default. [Pixal3D](https://github.com/TencentARC/Pixal3D)
  by Tencent ARC (MIT) on ComfyUI's own TRELLIS.2, MoGe and mesh nodes. It makes
  a solid mesh with baked base colour, roughness/metal/occlusion and normal maps,
  saved as `.glb`, which opens in Blender, game engines and any glTF viewer.
- **Gaussian splat (Idol)**. [Idol](https://huggingface.co/speridlabs/idol)
  by Speridlabs (Apache 2.0), a fine-tune of TripoSplat, on the TripoSplat nodes.
  It makes a cloud of soft points that looks right from every side, but only
  splat tools open it. It is faster and lighter.

Both need ComfyUI v0.41 or newer (the managed runtime is pinned at v0.43.0).

## Pixal3D (textured mesh)

### Requirements

- VRAM needs are not published. The diffusion model alone is 5.6 GB (int8), and
  the TRELLIS.2 stages decode a 1536³ sparse grid, so expect it to need more
  than Idol and to take minutes rather than seconds.
- These files, which the page offers to download (about 9.95 GB in total) from
  Comfy-Org's ungated repackages. No Hugging Face token is needed.

| Folder | File | Size |
|--------|------|------|
| `models/diffusion_models` | `pixal3d_int8_convrot.safetensors` | 5.58 GB |
| `models/clip_vision` | `dino_v3_L_naf_fp32.safetensors` (DINOv3 ViT-L + NAF) | 1.22 GB |
| `models/vae` | `trellis_2_shape_vae_bf16.safetensors` | 1.10 GB |
| `models/vae` | `trellis_2_texture_vae_bf16.safetensors` | 0.95 GB |
| `models/geometry_estimation` | `moge_2_vitl_normal_fp16.safetensors` (MoGe 2) | 0.66 GB |
| `models/background_removal` | `birefnet.safetensors` (shared with Idol, only for background removal) | 0.44 GB |

### Settings

- **Remove background**: as for Idol below.
- **Detail**: *High* (default) decimates to 700,000 faces and bakes 4096 px
  textures, *Standard* to 300,000 faces and 2048 px. The normal map is 2048 px
  and ambient occlusion 1024 px either way.
- **Advanced**: seed (-1 for random). The sampler settings are fixed to
  ComfyUI's Pixal3D template, which follows Tencent ARC's pipeline.

### How it works

`src-tauri/src/templates/mesh3d.rs` builds the graph from ComfyUI's
"Pixal3D & TRELLIS.2: Image to Model" template, Pixal3D branch only:

1. `LoadImage` → `RemoveBackground` (BiRefNet) → `ImageCropToMask` (1024², pad 1.1).
2. MoGe 2 estimates the camera field of view (`MoGeInference` →
   `MoGeGeometryToFOV`), which `Pixal3DConditioning` uses with DINOv3-L features.
3. Three TRELLIS.2 sampling stages on the Pixal3D model: sparse structure
   (12 steps), shape (20 steps), and shape upsampled to 1536 (12 steps), each
   with `CFGOverride` + `RescaleCFG`, then the texture stage (12 steps, cfg 1).
4. Mesh clean-up and baking with core nodes: `RemeshMesh` at 768 in the
   template's watertight `solid` mode (fill 20) on ComfyUI v0.43 and newer, or
   its earlier `udf` mode on v0.41 and v0.42, picked from the worker's
   `object_info`,
   `DecimateMesh`, `MeshSmoothNormals`, `UnwrapMesh`, `BakeTextureFromVoxel`,
   `BakeNormalMapFromMesh`, `BakeAmbientOcclusion`, `ApplyTextureToMesh`.
5. `SaveGLB` writes `output/3d/mooshie_mesh_*.glb`.

`src-tauri/src/commands/mesh3d.rs` checks each worker for every node and file,
uploads the image to a worker that has them, and reads the `.glb` back only from
that prompt's history (at most 384 MiB). Like the splat, jobs never enter the
image gallery or image progress.

The viewer draws the mesh with a small bundled WebGL 2 renderer
(`meshRenderer.ts`, `utils/glb.ts`): base colour, the ORM map, the normal map
through a derivative tangent frame, and a three-light studio rig. It is a
preview; Blender or a glTF viewer will show the same file with their own lighting.

## Idol (Gaussian splat)

### Requirements

- ComfyUI v0.41 or newer.
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

### Using it

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

The viewer is a small bundled WebGL 2 renderer for both kinds: drag to orbit, right-drag
or Shift-drag to pan, scroll or pinch to zoom, double-click to reset, arrow keys
work when it has focus. **Spin** toggles the turntable.

## Library and export

Finished models of both kinds are stored per account in this browser's
IndexedDB, like the music library, so they are not shared between devices.
For meshes, **Save .glb** writes the file ComfyUI produced. For splats, **Save .ply** writes the
file ComfyUI produced (standard 3DGS PLY, SH degree 0). **Save .splat** writes
the compact 32-bytes-per-Gaussian format most web splat viewers open. Both open
in SuperSplat, Blender add-ons and other Gaussian splat tools. ComfyUI also
keeps its copy in `output/3d/`.

The splat uses the usual 3DGS convention (+Y down, camera looking along +Z),
so some tools show it upside down until you flip it.

## How the splat graph works

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

## MCP

Not exposed for either method. The library lives in the client, so a
server-side tool's result would not reach the 3D page.
