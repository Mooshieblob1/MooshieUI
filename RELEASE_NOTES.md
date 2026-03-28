## What's New in v0.4.5

### Full Internationalization (i18n)
- Added a complete localization system — every user-facing string in the app now goes through a translation layer
- Ships with **English** and **Spanish** out of the box; adding a new language only requires creating one translation file
- Language selector in Settings → Appearance lets you switch instantly — no restart needed
- 618 translation keys covering all UI areas: generation controls, gallery, lightbox, Model Hub, settings, setup wizard, canvas tools, downloads, and toast messages
- Reactive translated dropdown labels in Model Hub (sort, period, file format, model type) update live when switching language

### Customizable Quality Tags
- Quality tags for Anima and Illustrious/NoobAI models are now **editable** in Settings instead of hardcoded
- Separate positive and negative tag fields for each model family (Anima, Illustrious)
- Defaults ship with the recommended tags — customize them to match your preferred style
- Changes persist across sessions

### Tiled Upscale Quality Prompts
- Tiled upscales now use **quality-only prompts** for the KSampler pass instead of the full creative prompt
- Reduces visible tile seam artifacts by preventing the KSampler from trying to generate new content at tile boundaries
- When quality tags are enabled, the upscale pass automatically uses your quality tag settings as its conditioning
- New `upscale_positive_prompt` and `upscale_negative_prompt` fields in the workflow template

### Native Clipboard Image Paste
- New `read_clipboard_image` Tauri command reads images directly from the OS clipboard
- Bypasses WebView clipboard restrictions that prevented `navigator.clipboard.read()` from working on Linux
- Converts clipboard RGBA data to PNG and returns it to the frontend for use in img2img, inpainting, or ControlNet

### Pre-Commit Validation Agent
- Added i18n-specific checks to the pre-commit validation agent
- Automatically verifies locale key parity (en ↔ es), interpolation variable matching, key naming conventions, and detects hardcoded UI strings in changed files

---

## What's New in v0.4.4

### Native Drag-and-Drop for Image Import
- Dragging images from your file manager onto MooshieUI now works reliably via Tauri's native OS drag-drop API — replaces the flaky HTML5 drag-drop that WebKitGTK silently blocked
- Drop an image onto any section (Prompts, Sampler, Dimensions, Model) to import its embedded metadata into that section, or onto the preview area to import everything
- Drop onto the ControlNet zone to set a control image, or onto the Interrogate zone to auto-caption
- Each drop zone highlights with a dashed border and label so you can see exactly where you're dropping

### Path-Based IPC Optimization
- Native file drops now send just the file path (~50 bytes) to Rust instead of serializing the entire image as a JSON number array over IPC
- Metadata extraction, ControlNet uploads, and interrogation all use path-based Tauri commands — eliminates redundant multi-megabyte IPC round-trips
- New `read_image_metadata_path` Rust command reads and parses metadata directly from an OS file path

### Tiled Diffusion Node Fix
- Fixed "Node 'ApplyTiledDiffusion' not found" error by deploying the tiled diffusion custom node to ComfyUI's `custom_nodes/` directory instead of the wrong location
- Updated both the setup installer and the node deployment script

### Editable Number Inputs Fix
- Fixed Steps, CFG, and Batch Size value labels not being editable — clicking the number now properly opens a text input for direct keyboard entry
- Root cause: the `EditableValue` component was inside a `<label>` that stole focus from the text input before it could receive keystrokes
- Also improved the edit input styling with a visible background and border so it's clearly in edit mode

### Range Slider Fix on Linux
- Fixed range sliders (Steps, CFG) being unresponsive on Linux — WebKitGTK was intercepting slider thumb drags as OS drag-drop gestures after `dragDropEnabled` was turned on
- Added `-webkit-user-drag: none` to all range inputs and their thumb pseudo-elements

---

## What's New in v0.4.3

### Automatic CUDA 13.0 PyTorch for Blackwell GPUs
- The setup wizard and **Reinstall PyTorch** button now auto-detect NVIDIA Blackwell GPUs (compute capability ≥ 12.0) and install PyTorch with the `cu130` CUDA toolkit instead of `cu128`
- Fixes the "You need pytorch with cu130 or higher to use optimized CUDA operations" warning that disabled the optimized `triton` and `cuda` execution backends
- Detection uses `nvidia-smi --query-gpu=compute_cap` — silently falls back to `cu128` if nvidia-smi is unavailable

### VRAM Flush After Interrupt
- Interrupting a generation now also calls ComfyUI's `/free` endpoint to fully unload models and flush the execution cache
- Prevents corrupted VRAM state from rapid cancellations that could cause subsequent generations to produce **all-black images** — especially on Blackwell GPUs with `cudaMallocAsync`

### All-Black Image Detection
- MooshieSaveImage now detects when an output image is entirely black (pixel max < 1e-6) and prints a diagnostic warning to the ComfyUI log
- Helps identify VRAM corruption issues that produce zero-valued tensors (as opposed to NaN-based black images caught in v0.4.1)

---

## What's New in v0.4.2

### Import Images from External Directories
- New **Gallery** section in Settings lets you import image output folders from ComfyUI, SwarmUI, or any other tool
- Recursively scans for PNG, JPG, and WebP files and copies them into MooshieUI's gallery
- Skips duplicates automatically — safe to re-import the same directory
- Metadata embedded in imported images (prompts, settings) is preserved and readable in the gallery lightbox

### SwarmUI Metadata Compatibility
- When importing metadata from images generated by SwarmUI, inline syntax like `<segment:...>`, `<lora:...>`, `<random:...>`, and `<wildcard:...>` is now automatically stripped from prompts
- Prevents garbled prompt fields when browsing or re-using metadata from SwarmUI-generated images

### Export Diagnostic Logs
- New **Export Logs** button in Settings > About for troubleshooting
- Saves a single file containing: ComfyUI subprocess log, GPU info, Python/PyTorch versions, and app configuration
- Users can share this file when reporting issues — no more hunting through temp directories

---

## What's New in v0.4.1

### Black Image Fix (NaN Guard)
- Fixed a critical issue where generated images could come out **entirely black** due to NaN (Not-a-Number) values in the VAE output tensor
- Root cause: fp16 VAE decode overflow under VRAM pressure (especially with WanVAE and large batches) produces NaN values that `np.clip()` cannot catch
- Added `np.nan_to_num()` guards in all three image encoding paths:
  - **MooshieFaceDetailer**: input image frames are now sanitized before face detection
  - **MooshieSaveImage (8-bit PNG)**: output tensor is checked and clamped before uint8 conversion
  - **MooshieSaveImage (16-bit PNG)**: `_encode_16bit()` sanitizes before the 65535 multiply
- When NaN values are detected, a warning is printed to the ComfyUI log identifying the affected batch index

### Automatic BF16 VAE for Blackwell GPUs
- MooshieUI now **auto-detects NVIDIA Blackwell GPUs** (compute capability ≥ 12.0) at launch and automatically applies `--bf16-vae` to ComfyUI
- BFloat16 VAE uses the same exponent range as fp32 (preventing overflow/NaN) at half the VRAM cost — the best of both worlds
- This prevents the fp16 VAE overflow that causes black images in the first place, without the VRAM penalty of `--fp32-vae`
- Detection uses `nvidia-smi --query-gpu=compute_cap` — silently skipped if nvidia-smi is unavailable (e.g. AMD/Intel GPUs)
- **User override**: if you've manually set any VAE precision flag (`--bf16-vae`, `--fp16-vae`, `--fp32-vae`, `--cpu-vae`) in Settings > Extra Args, the auto-detection is skipped
