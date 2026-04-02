## What's New in v0.5.1

### Guidance Nodes — Anti-Hallucination for Upscale
- New **Soft Guidance** (CFG Rescale) toggle in Upscale Settings — reduces extra hands, objects, and other hallucinations at low denoise by rescaling classifier-free guidance
- Adjustable multiplier slider (0.0–1.0, default 0.4) for fine-tuning hallucination suppression
- New **Smart Guidance** (Positive-Biased Adaptive) toggle in Sampler Settings — patches the model to bias toward positive conditioning across all generation passes
- Custom ComfyUI nodes (`MooshieSoftGuidance`, `MooshieSmartGuidance`) auto-installed alongside existing tiled diffusion nodes

### Comprehensive Internationalization
- Wired **39 new i18n keys** across 11 components: SetupWizard, SettingsPage, CanvasEditor, ColorPicker, ControlNetSettings, GenerationPage, LoraGallery, ModelSelector, PromptInputs, ModelHubPage, EditableValue
- All 11 locales (EN, DE, ES, FR, IT, JA, KO, PT, RU, ZH, ZH-TW) now at **789 keys** with full parity
- Eliminated all remaining hardcoded UI strings from component templates

### Dependency Updates
- Bumped `png` 0.17 → 0.18 (adapted to new `output_buffer_size()` API)
- Bumped `dirs` 5 → 6, `rand` 0.9 → 0.10, `zip` 2 → 4
- Bumped `actions/upload-artifact` 4 → 7 in CI release workflow

---

## What's New in v0.5.0

### Expanded Model Architecture Support
- Added detection and optimal presets for **10 model architectures**: SD1.5, SDXL, Illustrious/NoobAI, SD3/SD3.5, Flux, Pony Diffusion, AuraFlow, PixArt, HunyuanDiT, Stable Cascade, and Kolors
- Each architecture auto-applies optimal sampler, scheduler, steps, CFG, and resolution when selected

### Accelerated Model Detection (Turbo/Lightning/LCM/Hyper)
- SDXL, SD1.5, and Pony models with "turbo", "lightning", "lcm", or "hyper" in the name are detected automatically
- Accelerated variants get reduced steps (4–6), lower CFG, and appropriate sampler settings instead of incorrect full-step defaults

### Rectified Flow Scheduling
- SD3/SD3.5 models inject `ModelSamplingSD3` (shift 3.0, discrete flow matching)
- Flux models inject `ModelSamplingFlux` (resolution-dependent shift: base 0.5, max 1.15)
- AuraFlow models inject `ModelSamplingAuraFlow` (shift 1.73)
- Stable Cascade models inject `ModelSamplingStableCascade` (shift 2.0)

### FluxGuidance for Flux Dev
- Flux Dev models automatically get a `FluxGuidance` node (guidance 3.5) injected into the positive conditioning
- Flux Schnell (guidance-distilled) is detected and skipped — no unnecessary guidance node

### SD3 Latent Support
- txt2img uses `EmptySD3LatentImage` (16-channel) for SD3, Flux, and Anima/WAN models instead of the standard 4-channel `EmptyLatentImage`

### Pony Diffusion Quality Tags
- Auto-applied score-based quality tags: `score_9, score_8_up, score_7_up, source_anime` (positive) and `score_1, score_2, score_3` (negative)
- Customizable via Settings, persisted alongside existing Anima and Illustrious quality tags

### Flux & SD3 ControlNet Presets
- Added Flux ControlNet models: XLabs-AI Canny v3 and Depth v3
- Added SD3.5 ControlNet models: Stability's official Canny and Depth controlnets
- ControlNet preset system now supports Flux and SD3 architectures with automatic model selection

---

## What's New in v0.4.9

### Bug Fix: Aspect Ratio Input
- Fixed aspect ratio inputs in the Dimensions panel randomly changing values while typing
- Custom ratios like `5:3` or `7:4` now stay exactly as entered instead of being overwritten by GCD-reduced equivalents

### Security: GlassWorm Supply-Chain Protection
- Added pre-commit hook and CI workflow to scan for obfuscated supply-chain attack patterns
- New PR annotation workflow highlights suspicious Unicode or encoded payloads in pull requests

### Maintenance
- Added Dependabot configuration for automated dependency updates
- Bumped Tailwind CSS, svelte-check, and uuid dependencies
- Added CODEOWNERS for automatic PR review routing

---

## What's New in v0.4.8

### Full Internationalization
- Added 9 new languages: Japanese, French, Korean, Chinese (Simplified), Chinese (Traditional), German, Portuguese, Russian, and Italian
- Language selector in Settings → Appearance now lists all 11 supported locales

### Complete i18n Coverage
- Replaced all remaining hardcoded English strings across 11 generation, settings, and canvas components with `locale.t()` calls
- Added 100+ new locale keys covering tooltips, placeholders, ControlNet presets, model selectors, autocomplete settings, and more
- Every key (743 total) is now present in all 11 locale files with proper native translations

### Locale Cleanup
- Removed unused duplicate locale keys across all locale files
- Verified key parity: 0 missing keys across all languages

---

## What's New in v0.4.7

### PyTorch Install Heartbeat
- Long PyTorch downloads (multi-GB CUDA wheels) now show periodic progress messages every 30 seconds so you know the installer hasn't stalled
- Applies to both first-time setup and PyTorch reinstall from Settings

### PyTorch Install Reliability
- Added `--extra-index-url https://pypi.org/simple/` fallback to all PyTorch install commands (NVIDIA, Intel XPU, CPU)
- Fixes installs that failed when non-PyTorch dependencies weren't available on the GPU-specific index

### Info Tips Toggle
- New "Show Info Tips" setting in Settings → Accessibility to hide/show the (?) tooltip icons throughout the interface
- Useful for experienced users who no longer need the contextual help hints

### Dimension Calculation Fix
- Improved the area-faithful aspect ratio formula to pick the dimension pair closest to the target area
- Fixes edge cases where certain aspect ratios produced dimensions slightly off from the expected pixel count

### Anima Minimum Resolution
- Anima models now auto-clamp to at least 1024² total pixel area before generating
- Preserves your chosen aspect ratio while ensuring the model operates at a resolution where it produces good results

---

## What's New in v0.4.6

### Wayland AppImage Fix (Issue #3)
- Fixed white screen on Wayland-based Linux distros (CachyOS, Arch, etc.) when running the AppImage
- The app now automatically detects Wayland sessions, locates the system `libwayland-client.so.0`, and preloads it so WebKitGTK can render correctly
- Removes the forced `GDK_BACKEND=x11` set by the AppImage GTK plugin, allowing native Wayland rendering
- Searches versioned `.so.0` first (required on Arch-based distros), with unversioned `.so` fallback

### AMD Multi-GPU Detection Fix (Issue #2)
- Fixed ROCm GPU architecture detection on systems with both integrated and discrete AMD GPUs (e.g. Ryzen 9950X3D + RX 9070 XT)
- Fixed incorrect RDNA 4 device ID prefix — was checking `0x15xx` but RX 9070 series uses `0x75xx`
- Detection now collects all GPU architectures from rocm-smi and sysfs instead of returning the first match
- Prefers `gfx120X` (RDNA 4 discrete) over older architectures, ensuring the correct PyTorch ROCm index is used

### Code Formatting
- Applied `cargo fmt` across the entire Rust codebase for consistent formatting

---

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
