# Style Reference

Style reference lets you upload a reference image and apply its style to any
generation. Colors, textures, and composition are influenced without replacing
the text prompt. The implementation selects the correct technique for the active
model family automatically.

## Supported model families

| Family | Technique | Required custom nodes |
|--------|-----------|-----------------------|
| Flux.1 (flux1d, flux1s, flux1krea) | Flux Redux | None (core ComfyUI) |
| SD1.5 (sd15) | IP-Adapter Plus | ComfyUI_IPAdapter_plus |
| SDXL, Illustrious, Pony (sdxl, illustrious, pony) | IP-Adapter Plus | ComfyUI_IPAdapter_plus |
| All other families | Not supported | -- |

Unsupported families show an explanatory hint in the panel. Generation with
style reference enabled on an unsupported family is blocked at the template
validation stage with a clear error message.

## Required model files

### Flux.1 (Flux Redux)

Place files in the paths shown, relative to your ComfyUI root.

**models/style_models/**

- `flux1-redux-dev.safetensors`
  Gated on Hugging Face. Accept the license at
  https://huggingface.co/black-forest-labs/FLUX.1-Redux-dev
  and download the file from that page. Cannot be downloaded directly without
  accepting the license.

**models/clip_vision/**

- `sigclip_vision_patch14_384.safetensors`
  Download from Comfy-Org:
  https://huggingface.co/Comfy-Org/sigclip_vision_384/resolve/main/sigclip_vision_patch14_384.safetensors

### SD1.5 (IP-Adapter Plus)

**models/ipadapter/**

- `ip-adapter-plus_sd15.safetensors`
  Download from h94/IP-Adapter:
  https://huggingface.co/h94/IP-Adapter/resolve/main/models/ip-adapter-plus_sd15.safetensors

**models/clip_vision/**

- `CLIP-ViT-H-14-laion2B-s32B-b79K.safetensors`
  Download from h94/IP-Adapter (image_encoder folder):
  https://huggingface.co/h94/IP-Adapter/resolve/main/models/image_encoder/CLIP-ViT-H-14-laion2B-s32B-b79K.safetensors

### SDXL / Illustrious / Pony (IP-Adapter Plus)

**models/ipadapter/**

- `ip-adapter-plus_sdxl_vit-h.safetensors`
  Download from h94/IP-Adapter (sdxl_models folder):
  https://huggingface.co/h94/IP-Adapter/resolve/main/sdxl_models/ip-adapter-plus_sdxl_vit-h.safetensors

**models/clip_vision/**

- `CLIP-ViT-H-14-laion2B-s32B-b79K.safetensors` (same file as SD1.5 above)

## Using the panel

1. Open the Style Reference section in the Generation page sidebar.
2. Enable the toggle.
3. Drop or browse to a style reference image.
4. Adjust Strength (0.0 to 1.0; 0.6 is a good starting point).
5. Choose a weight type:
   - Flux Redux: multiply (default), attn_bias, average
   - IP-Adapter: linear (balanced), style transfer (emphasizes style over composition),
     composition (emphasizes composition), strong style transfer
6. For IP-Adapter only: adjust Start and End to control which fraction of
   sampling steps the style reference influences.
7. Generate as normal.

If IP-Adapter Plus custom nodes are not installed, the panel shows an Install
button that clones the pack from GitHub and installs its pip dependencies.
The custom nodes are only needed for SD1.5 and SDXL; Flux.1 uses built-in
ComfyUI nodes.

## Implementation notes

- Workflow templates: `src-tauri/src/templates/style_ref.rs`
- Generation params: `style_ref_enabled`, `style_ref_image`, `style_ref_strength`,
  `style_ref_weight_type`, `style_ref_start`, `style_ref_end`
- IP-Adapter lazy install follows the same pattern as INT8-Fast (ComfyUI-GGUF):
  verified by checking for the `IPAdapterUnifiedLoader` class in the ComfyUI
  object info endpoint.
- Model categories `style_models`, `ipadapter`, and `clip_vision` are registered
  in `category_subdirs()` in `src-tauri/src/commands/api.rs`.
