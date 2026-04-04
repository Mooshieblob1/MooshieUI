# Changelog

## What's New in v0.6.0

### Fixed Clipboard Copy
- Fixed image clipboard copy silently failing on Linux — the "Copied to clipboard" toast appeared but pasting produced nothing
- Restored native platform clipboard tools (`xclip`/`wl-copy` on Linux, `osascript` on macOS, PowerShell on Windows) replacing the broken `arboard`-based Tauri clipboard plugin which doesn't work reliably on Linux/Wayland
- Affects all copy actions: preview pane, bottom panel, and gallery lightbox

### Custom Gallery Storage Path
- New **Gallery location** setting in Settings › Gallery lets you choose any directory to store generated images
- Useful for pointing the gallery at a larger drive or a shared network folder
- Browse to select a folder or reset to the default `{data_dir}/gallery` location
- When moving installations, the gallery is preserved in place (not copied) to avoid duplicating potentially hundreds of gigabytes of images

### Prevent Recursive Installation Move
- Installation move now detects and blocks recursive nesting (moving into a subdirectory of itself or to a parent directory)
- Added a depth limit safety net (`MAX_COPY_DEPTH = 64`) to `copy_dir_recursive` to prevent infinite loops if overlap detection is somehow bypassed
- The copy function now skips the destination directory if it appears inside the source tree

---

## What's New in v0.5.9

### Bug Fix: Import Images from Directory
- Fixed a bug where "Import images from directory" in Settings showed an "Importing..." status but images never appeared in the gallery
- The gallery now refreshes automatically after a successful import without requiring a manual reload

---

## What's New in v0.5.8

### Manual Save Mode
- New **Manual save mode** setting in Settings › Gallery: when enabled, generated images are not auto-saved to the internal gallery
- A **Save to folder** button appears on each image (grid hover, list view, and lightbox) to write the image — with full embedded metadata — to a directory of your choice
- Configure one or more save directories; if multiple are set, a picker prompts you to choose on each save
- Per-directory browsing via the native folder picker
- All 11 locales fully translated

### LoRA Panel Image Caching
- CivitAI preview images in the LoRA bottom panel now load through the Rust backend with your CivitAI API key, fixing the white question-mark placeholder caused by unauthenticated CDN requests
- Images are cached to disk (`{data_dir}/image_cache/`) with a 7-day TTL so they load instantly on subsequent app launches without re-downloading
- Navigating between preview images (next/prev) pre-resolves the adjacent image in the background

---

## What's New in v0.5.7

### Mugen Model Support
- Added support for Mugen (CabalResearch/Mugen) — an SDXL architecture model using a Flux2 VAE and rectified flow scheduling
- MooshieUI automatically detects Mugen checkpoints by filename and applies the correct generation pipeline: `ModelSamplingSD3` (shift=10) for rectified flow and `VAEDecodeTiled` for the Flux2 VAE
- Bundled the SDXL-Flux2VAE custom node as a flat deployment to fix a circular import issue that prevented the model-loading patch from applying

### PI-Chan Discord Bot Support
- MooshieUI images now embed `mooshie_extra` metadata alongside the existing `sui_image_params` block
- PI-Chan will display "MooshieUI Parameters" instead of "SwarmUI" for MooshieUI-generated images
- `mooshie_extra.software` acts as the detection marker; future MooshieUI-exclusive params prefixed with `mooshie_` appear automatically in PI-Chan embeds
- Full backward compatibility — SwarmUI and other parsers ignore the new key

### Model Hub Download Hardening
- Pasting a HuggingFace model page URL (without `/resolve/`) now shows a clear error before attempting any download
- Downloads that return `Content-Type: text/html` are rejected with a user-facing error instead of silently writing an HTML file as `.safetensors`
- Zero-byte leftover files from failed downloads are cleaned up and re-downloaded rather than being treated as complete
- Error messages now include a formatted example of the correct `/resolve/main/` URL format

---

## What's New in v0.5.6

### LoRA Metadata Fetching — Path Resolution Fixed
- Fixed LoRA metadata and CivitAI images not loading for models stored in extra model directories (`extra_model_paths`)
- `resolve_model_path` now searches all known subdirectory variants (`loras/`, `Lora/`, `LoRA/`, `LyCORIS/`, etc.) matching the same paths ComfyUI itself scans — previously only the canonical `loras/` subdirectory was checked
- Flat directories (models stored directly in the root with no subdirectory) now also work correctly
- Error display in the LoRA gallery now shows the actual error message (e.g. "LoRA file not found") instead of always showing "Not on CivitAI"
- LoRA file hashing (`full_sha256`) moved to a background thread (`spawn_blocking`), preventing async runtime stalls on large model files

### Windows Venv Auto-Repair After Directory Move
- Fixed startup failure when users move their MooshieUI data directory: `uv trampoline failed to spawn Python child process — entity not found (os error 2)`
- On startup, MooshieUI now detects stale venv paths by checking both whether the Python binary exists and whether `pyvenv.cfg`'s `home` key points to a valid directory
- If stale paths are detected, `uv venv --allow-existing` is run automatically to regenerate trampoline executables and fix path references — no manual intervention required
- The in-app Move Directory feature also now runs venv repair immediately after copying files to the new location

---

## What's New in v0.5.5

### Gallery Performance
- Gallery now renders progressively — first 48 images load immediately, additional batches load as you scroll, eliminating the initial lag spike when opening large galleries
- An `IntersectionObserver` sentinel at the bottom of the grid seamlessly loads the next 48 images as needed
- Sort, filter, and group changes reset to the first page, keeping the initial render instant
- Reduced thumbnail pre-fetch distance (`rootMargin`) from 200 px to 100 px, cutting simultaneous network requests in half when the gallery opens

---

## What's New in v0.5.4

### Re-Release Stability
- Re-issued the prior release payload as `v0.5.4` after the cancelled `v0.5.3` run to ensure a clean, complete release pipeline execution
- Preserved the same MooshieUI metadata compatibility behavior introduced previously, including `mooshie_extra` identification and backward compatibility with SwarmUI parsers

### Release Pipeline Integrity
- Re-ran version synchronization and build validation (`cargo check` + frontend production build) before tagging
- Published a fresh release tag to guarantee CI artifacts and GitHub Release assets are generated from a finalized main branch state

---

## What's New in v0.5.3

### MooshieUI Metadata Identity
- PNG metadata now includes a `mooshie_extra` object alongside the existing SwarmUI-compatible `sui_image_params` — images are identified as "MooshieUI" by parsers like PI-Chan instead of generic "SwarmUI"
- Detection marker `"software": "MooshieUI"` is always present in embedded metadata
- Full backward compatibility preserved — SwarmUI and other parsers ignore `mooshie_extra`

### Extended Metadata Parameters
- **Model Architecture** — now embedded in image metadata (SD1.5, SDXL, Flux, etc.)
- **Smart Guidance** — recorded when enabled
- **Differential Diffusion** — recorded when enabled (inpainting)
- **ControlNet** — preset name, model, and strength now embedded when ControlNet is active
- **Upscale details** — tiling, tile size, upscale steps, and soft guidance multiplier now included
- All MooshieUI-exclusive params round-trip correctly when re-importing images

---

## What's New in v0.5.2

### Bug Fix: Guidance Nodes Not Installed
- Fixed `MooshieSoftGuidance` and `MooshieSmartGuidance` nodes failing with "Node not found" error because `nodes_guidance.py` was not deployed to ComfyUI's `custom_nodes/` directory
- The Rust auto-deploy in `nodes.rs` now embeds and writes `nodes_guidance.py` alongside `nodes_tiled_diffusion.py` on every launch

---

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

---

## What's New in v0.4.0

### Image Interrogator
- Added image interrogation using the WD EVA02 Large v3 ONNX model — automatically captions images with tags
- Supports drag/drop, browse, paste, gallery images, and session history as input sources
- New interrogator threshold settings in Settings page

### Right-Click Context Menus
- Right-click any gallery or session history image to access actions (upscale, copy, delete, interrogate, etc.)

### Lightbox Redesign
- Compact icon-only toolbar with grouped action buttons
- Cleaner layout with more screen space for the image

### Other Improvements
- Release notes in About page now rendered as proper markdown (via `marked`)
- LoRA cards scale proportionally by panel height with a constant 3:4 ratio
- Auto-detects upscale scale factor from model name in UpscaleSettings and metadata import

---

## v0.3.9

**UI Density, Click-to-Edit Values, Dynamic Release Notes**
- Tightened spacing across all generation sections, sticky headers, and controls to reduce scrolling
- Added collapsible left, right, and bottom panels with toggle buttons — panels remember their size
- Narrower sidebar (cleaner layout with smaller nav buttons and icons)
- Sampler settings reworked into compact 2-column grid layout
- Click-to-edit slider values — click any slider readout (Steps, CFG, Batch, Denoise, Scale, Tile Size, etc.) to type an exact value
- Settings page now uses responsive masonry layout (1/2/3 columns)
- Release notes in Settings now fetched live from GitHub Releases instead of hardcoded changelog
- Fixed pip install errors when using uv-created virtual environments (uv pip install fallback)
- Quality prompts badge moved inline with Positive Prompt label

---

## v0.3.8

**Metadata Import, Face Fix Auto-Setup, Windows Fixes**
- Drag-and-drop or Ctrl+V images onto generation sections to import metadata (prompts, sampler, dimensions, model, upscale) — or drop on the preview area to apply all parameters
- Imported prompts automatically strip quality tags already applied by the app to avoid duplication
- Face fix now auto-downloads the detection model and installs ultralytics before generating — no manual setup needed
- Toggling off "Random" seed now recalls the last generated seed instead of resetting to 0
- Fixed drag-and-drop not working on Windows (disabled Tauri native drag handler intercepting HTML5 events)
- Fixed gallery thumbnails not loading on Windows (cross-platform protocol URL fix via `convertFileSrc`)
- Smooth scroll improved with time-based interpolation for consistent frame rates

---

## v0.3.7

**Model Integrity & Thumbnail Recovery**
- Fixed recommended models (Anima, SIH) incorrectly showing as installed when VAE or text encoder components are missing — all components are now verified
- Selecting a recommended model now downloads only the missing components instead of skipping entirely
- Fixed gallery thumbnails appearing broken/dead after app update or relaunch (added retry logic, fixed query param parsing, removed aggressive caching)
- Added SwarmUI `Models/` and `dlbackend/comfyui/models/` paths to external model directory scanning for broader compatibility
- App now stops ComfyUI before restarting during updates, preventing orphan processes

---

## v0.3.6

**Quality Tag Controls**
- Added option to disable automatic quality tag injection (Settings > Performance)
- Confirmation popup warns users about quality impact before disabling
- Removed quality tags from prompt placeholder text (tags are auto-applied, no need to suggest them)
- Added preview tip for users with auto quality tags disabled suggesting to re-enable if results are poor

---

## v0.3.5

**Model Category Filtering**
- Fixed LoRAs, upscale models, VAEs, and other non-checkpoint files appearing in the checkpoint dropdown when using external model directories (SwarmUI, A1111, etc.)
- Added server-side path-prefix filtering to exclude models whose directory path indicates they belong to a different category (e.g. entries starting with `Lora\`, `upscale_models\`, `yolov8\` are excluded from checkpoints)
- Same filtering applied to LoRA list to exclude checkpoint/VAE/upscale entries

---

## v0.3.4

**Extra Model Paths Fix**
- Fixed LoRA files in external model directories (SwarmUI, A1111, etc.) incorrectly appearing as checkpoints
- Removed wildcard `.` base scan from all categories in extra_model_paths YAML — each category now only scans its specific subdirectories, preventing cross-contamination
- Added `Stable-Diffusion` (SwarmUI naming) to checkpoint path detection

---

## v0.3.3

**Anima Quality Tags + Changelog Fix**
- Fixed Anima positive quality tags (`year 2025, newest, masterpiece, ...`) now prepended before the user prompt instead of appended after — matches the correct tag ordering for Anima/COSMOS models
- Fixed custom node deployment unreachable when ComfyUI is started externally (non-AutoLaunch mode)
- Fixed in-app changelog in About section stuck on v0.2.6–v0.2.9 — now shows all versions through v0.3.3

---

## v0.3.2

**Custom Node Deployment Hotfix**
- Fixed "Node MooshieSaveImage not found" error caused by silent failure during custom node deployment
- `ensure_mooshie_nodes` now returns a proper error instead of silently continuing when node files can't be written

---

## v0.3.1

**Updater Hotfix**
- Fixed updater 404 issue caused by release tag/version filename mismatches in `latest.json`
- Release pipeline now generates updater URLs/signature references from actual built artifact names
- Bumped app versioning to `0.3.1` across frontend, Tauri config, and Rust crate for consistent release metadata

---

## v0.3.0

**Streaming Output Pipeline + 16-bit Support**
- Added `MooshieSaveImage` output node to stream final PNG bytes over WebSocket instead of relying on ComfyUI disk output round-trips
- Added optional 16-bit PNG output mode (`8-bit` / `16-bit`) exposed in generation settings and passed through workflow params
- Added Rust-side `save_to_gallery_bytes` command so streamed images can be persisted directly from in-memory bytes
- Added WebSocket binary event handling for Mooshie output payloads, including prompt association and 16-bit timing diagnostics

**Metadata Embedding + Compatibility**
- Added metadata mode options: `Text Chunk`, `Stealth Alpha`, and `Both`
- Implemented SwarmUI-compatible stealth alpha embedding/reading with gzip payload support and 8-bit/16-bit handling
- Added automatic compatibility policy for 16-bit PNGs: if `Stealth Alpha` is selected, effective save mode upgrades to `Both`
- Added UI feedback for the upgrade so users can see effective metadata behavior while 16-bit is active

**Queueing + Gallery Workflow**
- Reworked generation progress state to support queued prompts and active-prompt tracking
- Updated generate/cancel controls to support queueing, cancel current, and cancel-all behavior
- Switched output finalization path to use streamed images and persist metadata-rich gallery entries from memory
- Added lightbox metadata actions: `Reuse Settings`, `Remix` (random seed), and `Reuse Seed`

**Sampler + Model UX Improvements**
- Added model-aware recommendation cards for Anima and SIH in sampler settings
- Updated Anima defaults to practical baseline values: 30 steps, CFG 4, sampler `er_sde`, scheduler `sgm_uniform`
- Face-fix and upscale steps now track model recommendations (1/3 of primary steps in recommendation paths)
- Added sampler-aware recommended ranges for steps/CFG with one-click fix actions

**Preview + Readability Improvements**
- Replaced idle preview placeholder with a rotating tips carousel (manual arrows + scroll wheel + auto-cycle)
- Added progress indicator for tip auto-cycle and tuned dwell time for readability
- Added light/dark support in preview idle UI styling

**Other UI/Behavior Updates**
- Simplified `InfoTip` to native title/aria tooltip behavior
- Improved dimension-control synchronization to reflect persisted/updated generation dimensions

---

## v0.2.9

**Face Fix (FaceDetailer)**
- Added built-in face detection and re-denoising — detects faces with YOLOv8, crops each to a configurable guide size, re-denoises, and composites back seamlessly with feathered blending
- Lightweight custom `MooshieFaceDetailer` node bundled with the app — replaces the heavyweight Impact Pack dependency entirely
- Node auto-deploys to ComfyUI's `custom_nodes/` on every startup via Rust `include_str!` embedding — no manual installation or restarts needed
- Configurable settings: denoise strength, steps, guide size, and detector model selector
- Auto-download YOLOv8 face detection models (yolov8m recommended, yolov8n lightweight) with progress bars
- Compatible with video VAEs (WanVAE/Anima) — handles 5D tensor output gracefully

**UI Improvements**
- Mode selector (Text to Image / Image to Image / Inpainting) now stays pinned at the top of the panel when scrolling
- Session history paginated to 4 images per page with prev/next controls — auto-jumps to first page on new generations
- Session history images are now clickable to open in the lightbox — action buttons moved to bottom of thumbnail overlay
- Removed horizontal scrollbar from the left panel
- ControlNet section auto-hidden for Anima/COSMOS models (not supported)

---

## v0.2.8

- Fix installation move breaking setup and path resolution

---

## v0.2.7

**Separate Model/Sampler Sections, Auto-Start Toggle, Performance Fixes**
- Split "Model & Sampler" into two independent collapsible/draggable sections
- Section collapse states persist to localStorage across sessions
- Added "Auto-start ComfyUI" toggle in Connection settings — when off, a manual "Start ComfyUI" button appears in the status banner
- Image panning now works with left click and middle mouse at any zoom level
- Added "What's New" patch notes to About section in Settings
- Fixed event listener leak in ModelSelector (accumulated on every mount)
- Debounced all localStorage writes (300ms) to reduce synchronous I/O
- Pre-computed recommended filenames Set outside search-filtered derived
- Removed backdrop-blur-sm from gallery overlays and sticky elements to eliminate GPU compositing bottleneck causing scroll jank

---

## v0.2.6

**Install Location Chooser, Model Auto-Detection, Model Hub Fixes**
- Setup wizard now lets users pick any drive/folder for the ~5–10 GB installation instead of defaulting to AppData/C: drive
- Supports `MOOSHIEUI_DATA_DIR` environment variable override
- Existing users can relocate their entire installation to a new drive via Settings > Paths > Data Location
- Auto-detects existing model directories from ComfyUI, A1111/Forge, SwarmUI, and StabilityMatrix — one-click to share models without duplicating files
- Added ControlNet settings, presets, and mask editor UI
- Added download manager banner component for tracking model downloads
- Fixed Model Hub file format filter — CivitAI API expects array params (`fileFormats[]`), was sending as plain string causing 400 errors
- Fixed Model Hub infinite scroll — IntersectionObserver now uses reactive `$effect` instead of one-shot `onMount`
- Removed non-functional Model Hub status filter (CivitAI public API doesn't support it)
- Fixed collapsed settings sections resetting on tab switch — collapse state now persisted in localStorage
- Fixed checkpoints not loading from custom model folders — YAML generation now quotes paths and includes root dir as search path
- Fixed app reinstalling when data directory was moved

---

## v0.2.5

**AppImage Startup Fix, Manual Update Check**
- Fixed AppImage on Linux failing to start ComfyUI with "No module named 'encodings'" — the AppImage's bundled `PYTHONHOME`/`PYTHONPATH` env vars were poisoning the venv's Python interpreter; now cleared when spawning ComfyUI
- Added About section in Settings with current version display and a manual "Check for Updates" button with download progress and restart

---

## v0.2.4

**Model Hub, SwarmUI Metadata, ModelSpec, Native Clipboard**
- Added Model Hub: browse and download CivitAI models with image previews, NSFW filtering, API key setup, and expanded base model filters (NoobAI, Pony, Illustrious, SD 3.5, Flux, etc.)
- Switched image metadata from A1111 to SwarmUI JSON format with backward-compatible reading; lightbox metadata panel is now resizable
- Added ModelSpec support: reads Stability AI ModelSpec metadata from safetensors headers and displays model title, author, architecture, resolution, trigger phrases, tags, and usage hints in the model selector
- Fixed clipboard copy on Linux with native Wayland/X11 detection (`wl-copy` / `xclip`)
- Fixed seed showing -1 in metadata (return resolved seed from generate)
- Fixed denoise incorrectly included in txt2img metadata
- Fixed preview image save/copy using saved gallery image instead of blob
- Improved gallery scroll performance with RAF throttling and CSS containment
- Added missing ControlNet template module (`controlnet.rs`)

---

## v0.2.3

**Tier-1 Features, Metadata, Queue, Boards**
- Drag & drop image/mask inputs for img2img and inpainting
- PNG metadata embed, read, and apply (SwarmUI-compatible generation params in PNG text chunks)
- Prompt history and favorites — auto-saves generated prompts with quick reload and starring
- Style presets (Fooocus-style) — now optional via settings toggle, disabled by default
- Gallery boards — assign images to named boards, filter by board, manage boards in gallery/lightbox
- Shared model directory support — point to an external/shared models folder via extra model paths
- Queue management page — view running/pending queue with interrupt and remove actions
- Model-aware presets — auto-applies generation defaults based on detected model architecture
- Tooltip/help content filled out across remaining generation controls
- Fixed Linux inpainting responsiveness — removed per-stroke mask uploads to eliminate brush lag

---

## v0.2.2

**Hash-Based Model Detection, CivitAI Integration, Installer UX Overhaul**
- Added full SHA256 / AutoV2 hash-based model identification (CivitAI-compatible) — renamed files are still detected
- Added CivitAI hash lookup command for model metadata (name, version, preview images)
- Rewrote installer to stream terminal output as a matrix-style backdrop with per-step progress bars
- Suppressed all terminal windows on Windows (setup, ComfyUI server, cleanup)
- Added download progress bars for model downloads (checkpoint, VAE, upscale)
- SIH-1.5 now auto-downloads on selection (same as Anima) with file size display
- Regenerated app icons from logo
- Added version display in sidebar below connection status
- Expanded gallery with lightbox improvements and metadata support

---

## v0.2.1

**Blackwell GPU Fix**
- Upgraded PyTorch CUDA index to `cu128` for RTX 50-series (Blackwell) support — RTX 5070/5080/5090 require CUDA 12.8+; the previous `cu124` index only supported up to sm_90

---

## v0.2.0

**Anima Support, Settings Page, Live Preview, Cross-Platform Builds**
- Added Anima Preview 2 as a recommended model with split model loading (UNETLoader + CLIPLoader + VAELoader), auto-download of 3 component files, quality prompt injection, and optimized defaults (steps=30, CFG=4)
- Added settings page with collapsible sections and search bar
- Added config read/write commands (`get_config`, `update_config`)
- Added InfoTip tooltip component with hover persistence
- Added CFG++ sampler auto-detection (soft-sets CFG to 1.4)
- Fixed 5D latent tiled diffusion for Anima/COSMOS models — MultiDiffusion and SpotDiffusion now handle both 4D and 5D tensors
- Enabled live generation previews (`--preview-method auto`)
- Added phase-aware progress display (Generating / Upscaling / Preparing)
- Added lightbox zoom (scroll-wheel at cursor) and dismiss (Escape / click-outside)
- Fixed clipboard copy — copies gallery file as URI reference, preserving format and metadata
- Added Windows and Linux cross-platform CI release builds
- Auto-downloads SIH-1.5 checkpoint and SDXL VAE on first run when no model is installed
- Added persistent gallery with save, load, list, and delete commands
- Added toast notifications for image operations
- Added resizable panels and enhanced LoRA management

---

## v0.1.0

**Initial Release**
- Core text-to-image, image-to-image, and inpainting generation modes
- ComfyUI integration via REST API and WebSocket bridge
- One-click setup wizard — installs uv, Python, ComfyUI, and PyTorch automatically
- Auto GPU detection (NVIDIA CUDA / AMD ROCm / CPU)
- Sampler, scheduler, steps, CFG, seed, batch size, and dimension controls
- Aspect ratio presets and custom ratio with resolution slider
- Upscaling with MultiDiffusion tiled diffusion (MultiDiffusion and SpotDiffusion)
- LoRA support with per-LoRA strength sliders
- Real-time progress bar and live latent preview via WebSocket
- Cancel button to interrupt generation
- Basic gallery grid
- Cross-platform CI release workflow (Linux `.deb` / `.AppImage`, Windows `.msi` / `.exe`)
