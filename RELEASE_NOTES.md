## What's New in v0.7.7

### Full i18n Coverage
- **40+ hardcoded English strings localized** — toast messages, context menu labels, panel collapse/expand titles, drop overlay texts, alt attributes, ON/OFF toggles, ControlNet install status messages, and clipboard errors are now all routed through the `locale.t()` system with translations for all 11 supported languages (English, German, Spanish, French, Italian, Japanese, Korean, Portuguese, Russian, Chinese, Traditional Chinese).

### Browser-Mode Clipboard Improvements
- **Interrogate from clipboard in browser mode** — the "Interrogate Clipboard" feature now works on headless servers by reading images directly via the Web Clipboard API instead of relying on the unavailable Tauri clipboard command.
- **`readClipboardImageSafe` fallback** — new clipboard utility that automatically falls through from the native Tauri command to the browser Clipboard API, used by both ControlNet image paste and generation input paste.
- **Simplified gallery clipboard flow** — removed redundant `navigator.clipboard?.write` feature-detection guard in favor of the unified `writeBlobToClipboard` helper, which already handles insecure-context fallback internally.

### Bug Fixes
- **Face fix model hash updated** — the YOLOv8n face detection model SHA-256 hash was corrected to match the current upstream file, preventing false integrity failures during download.
- **Docker OpenCV fix** — added `opencv-python-headless` to the Docker build so ControlNet preprocessors that depend on OpenCV work out of the box.

---

## What's New in v0.7.6

### Pip Install Fix
- **Fixed pip path resolution** — custom node and pip package installs failed with "No such file or directory (os error 2)" when the `uv` tool wasn't available. The fallback pip path was constructed with string formatting instead of proper OS path joining, breaking on paths with spaces or unusual separators. All 4 affected locations now use `PathBuf::join()`.

### Moderator Account Creation
- **Moderators can now create accounts** — the "Add Account" button was only visible to admins despite the backend already permitting moderators to create accounts. The button now appears in the moderator account management section.

### Browser-Mode Clipboard Copy
- **Image copy works on headless servers** — copying images in browser mode failed on servers without `xclip`/`wl-copy` installed. The copy flow now falls through from server-side clipboard to the browser's native Clipboard API (available over HTTPS), so copy works without any tools installed on the server.

### UI Polish
- **Username tooltip on hover** — account list entries now show the full username on mouseover, so long names truncated by narrow windows are still readable.

---

## What's New in v0.7.5

### Generation Reliability Fix
- **SSE race condition resolved** — fixed a timing bug where the `output_image` handler (async HTTP fetch) could lose the race against the synchronous `executing: node=null` completion event, causing generated images to silently disappear despite successful execution. In-flight image fetches are now tracked and awaited before finalizing output.

### Right-Click Copy with Metadata
- **MooshieSaveImage outputs RGBA** — the custom ComfyUI output node now produces RGBA PNGs (alpha=255) instead of RGB, ensuring the alpha channel is available for stealth metadata embedding
- **Server-side metadata embedding** — new `_embed_temp_metadata` endpoint allows the browser to embed stealth alpha metadata into temp images without serializing multi-MB image data over JSON
- **Automatic blob URL upgrade** — in browser mode, generated images are displayed immediately and then silently upgraded with metadata-embedded versions in the background, so right-click → Copy Image includes stealth alpha metadata from the start

### Clipboard & Lightbox Reliability
- **Persist promise tracking** — gallery image persistence is now tracked with per-image promises, eliminating race conditions where clipboard copy or lightbox display tried to use gallery URLs before the image was saved
- **Lightbox URL upgrade** — lightbox now shows the blob URL immediately and upgrades to the gallery URL once persistence completes, instead of waiting or showing a broken reference

---

## What's New in v0.7.4

### Image Storage Limits & Expiry
- **Per-user storage limit** — 2 GB default storage quota per user; admins and moderators can adjust limits per account via the API
- **Automatic image expiry** — gallery images expire after 7 days and are cleaned up automatically every 30 minutes
- **Expiry warning banners** — amber warning banners in the gallery and bottom panel remind users to download images before they expire, with a count of images expiring within 24 hours
- **Storage usage display** — users see their current storage usage and limit in the gallery UI
- **Admin exemption** — admin and localhost galleries are exempt from both expiry and storage limits

### Server-Mode Bug Fixes
- **Model commands in browser mode** — `hash_model_file`, `get_model_install_dirs`, `find_model_by_hash`, `read_modelspec`, and Civitai info commands now work correctly in headless server mode
- **Interrogation in server mode** — WD14 tagger / interrogation feature now available in headless server mode (previously desktop-only)
- **SSE connection stability** — reduced SSE keepalive interval from 30s to 15s to prevent Cloudflare Tunnel disconnects
- **Expanded moderator permissions** — moderators can now manage accounts, view system info, and access model tools (with privilege escalation guards)
- **YOLOv8m face model hash** — corrected the SHA256 hash for the face detection model used by FaceFix

### i18n Updates
- Added gallery expiry and storage translation keys across all 11 supported languages

---

## What's New in v0.7.3

### Headless Server Mode + Docker/K8s Support
- **Headless server binary** — `mooshieui-server` runs without Tauri/webkit, serving the Svelte frontend via embedded axum. Designed for Docker and K8s deployments
- **Dockerfile** — multi-stage build (Node → Rust → CUDA runtime) with ComfyUI + PyTorch pre-installed
- **docker-compose.yml** — GPU passthrough, persistent volumes, optional Cloudflare Tunnel sidecar
- **K8s manifests** — namespace, PVCs, configmap, secret, deployment + service with GPU resource limits and health probes
- **Cargo feature gating** — all Tauri dependencies behind `desktop` feature flag; `server` feature for headless binary
- **CI/CD** — release workflow builds server binary, publishes Docker image to GHCR with semver + latest tags

### Auth Lockdown
- **No open access** — remote users must authenticate; self-registration disabled (admin creates accounts)
- **Stored admin role** — accounts can now have `"admin"` role for full remote access (account management, settings, filesystem operations)
- **Env-var admin seeding** — `MOOSHIEUI_ADMIN_USER` + `MOOSHIEUI_ADMIN_PASS` environment variables seed the initial admin account on first boot
- **Model downloads for users** — `download_model` command moved from moderator-only to user level

### Server Update Notifications
- **Update check endpoint** — `GET /internal-api/_check_update` queries GitHub Releases API for newer versions (admin/moderator only)
- **Redeploy banner** — admin and moderator users in browser mode see a notification when a new version is available: "MooshieUI vX.Y.Z is available — please redeploy to update!"
- **Desktop updater unchanged** — Tauri auto-updater continues to work as before for desktop users

---

## What's New in v0.7.1

### Prompt Scheduling (FromTo)
- **Timestep-based prompt scheduling** — apply specific prompt tags only during certain portions of the denoising process, giving you fine-grained control over when concepts appear during generation
- **MooshieUI syntax** — `<from:0.5>text</from>` (apply from 50% onward), `<to:0.8>text</to>` (apply up to 80%), `<range:0.2:0.8>text</range>` (apply between 20%–80%)
- **SwarmUI syntax** — `<fromto[0.5]:cat, dog>` swaps between two phrases at the specified timestep, with `,`, `|`, and `||` separators supported
- **Visual highlighting** — scheduling blocks glow with a gold border in the textarea so you can see at a glance which tags are scheduled
- **Visual helper panel** — collapsible panel below the prompt shows each scheduled segment with a mini range bar and percentage labels
- **Full autocomplete support** — tag autocomplete works normally inside scheduling blocks, preserving the wrapper syntax when accepting suggestions
- **Clean metadata** — prompts in image metadata show all tags without scheduling syntax; scheduling info is stored in a separate `mooshie_prompt_schedule` field for round-trip clarity
- **Zero overhead** — when no scheduling tags are used, the workflow is identical to before (no extra nodes)
- **Backend support** — uses ComfyUI's built-in `ConditioningSetTimestepRange` + `ConditioningCombine` nodes; works with txt2img, img2img, and inpainting

### i18n Updates
- Added `generation.prompts.scheduling` and `generation.prompts.scheduling_segments` translations across all 11 supported languages

---

## What's New in v0.7.0

### Enhanced Account Management
- **Searchable account list** — filter accounts by username with a real-time search box
- **Sortable columns** — sort by Name, Date Joined, or Last Online with ascending/descending toggle
- **Online-first grouping** — online users always appear at the top regardless of sort column
- **Scrollable account list** — shows 6 accounts at a time with smooth scrolling for larger lists
- **Account timestamps** — tracks when each account was created and when they were last active (persisted to disk every 60 seconds)
- **Delete confirmation with data retention** — deleting an account now shows a confirmation dialog with a "Keep user data" checkbox; when checked, gallery images are preserved and restored when an account with the same username is re-created

### SSE Image Delivery Fix
- **Temp-file based image delivery** — preview and output images are now saved to temporary files and delivered via lightweight JSON references over SSE, fixing dropped images when using Cloudflare tunnels or reverse proxies that reject large SSE payloads
- **Dual-path emission** — Tauri desktop mode still receives full base64 inline for maximum performance; browser/LAN mode uses the temp-file path

### Windows GPU Detection Fix
- Fixed GPU detection and CUDA mismatch error on Windows systems

### i18n Updates
- Added missing `gallery.saving` and `gallery.toast.copying` translations across all 10 supported languages

---

## What's New in v0.6.9 — The "Nice" Update

### Compare Grid Fixes
- **Model switching actually works now** — compare grid cells now properly capture and apply split-model fields (`diffusionModel`, `clipModel`, `clipType`, `modelspecArchitecture`), so each cell truly generates with its own model instead of silently reusing whichever model was last selected
- **Smart generation order** — cells are sorted by model before queuing so all cells using the same model generate consecutively, minimizing expensive ComfyUI model swaps

### Compare Grid Visual Improvements
- **Full-coverage color borders** — cell color coding now renders as an overlay that covers the entire panel including the sticky mode selector and generate button sections (previously hidden behind opaque backgrounds)
- **Pulsing glow effect** — active cell border has a subtle animated glow that pulses to clearly indicate which cell is being edited
- **Rounded corners** — compare border overlay uses 6px rounded corners for a polished look

---

## What's New in v0.6.8

### Anima Preview 3 Support
- **One-click Anima Preview 3 setup** — added to the recommended models list with split-model auto-download (diffusion model, Qwen 3 CLIP, and Qwen Image VAE) and tuned defaults (30 steps, CFG 4, er_sde sampler)
- Optimized upscale and face fix defaults for Anima Preview 3 (10 upscale steps at 0.3 denoise, 10 face fix steps)

---

## What's New in v0.6.7

### Security: Supply Chain Hardening
- **SHA256 verification for YOLOv8 model downloads** — `face_yolov8m.pt` and `face_yolov8n.pt` are now verified against known-good hashes after download; a mismatch deletes the file and returns an error rather than silently running a corrupt or tampered model
- **SHA256 check on cached files** — previously downloaded models are re-verified on next use; a tampered cached file is re-downloaded rather than trusted
- **Pinned `ultralytics` version** — the face fix dependency is now installed as `ultralytics==8.4.34` instead of an unpinned `ultralytics`, preventing a malicious future PyPI release from being pulled automatically
- **`npm audit` and `cargo audit` in CI** — release builds now run dependency vulnerability scans for both frontend and Rust crates
- **Pre-commit enforcement** — the pre-commit agent now flags any `installPipPackage()` call that omits a `==version` pin as a blocking error

### Docs Fix
- Corrected README: MooshieFaceFix uses the `ultralytics` Python package with `.pt` PyTorch weights for YOLOv8 detection (not ONNX Runtime). ONNX Runtime (`ort` crate) is used only for the WD EVA02 image tagger (Describe feature)

---

## What's New in v0.6.6

### Compare Grid
- **XYZ compare grid** — new Compare tab in the bottom panel lets you create a grid of cells, each with its own generation parameters. Change prompts, checkpoints, samplers, seeds, or any setting per cell to compare results side by side
- **Grid generation** — pressing Generate with multiple cells queues all cells sequentially with a shared random seed for consistent comparisons
- **Grid stitching** — completed grids are automatically stitched into a single image with per-cell labels showing only what differs (e.g., "blue eyes" vs "green eyes") and a MooshieUI watermark
- **Spreadsheet-style naming** — cells use A1/B1/C1 labels; position-stable colors so each grid slot always has the same ring color
- **Add/remove columns & rows** — new cells clone the adjacent neighbor for quick parameter tweaking

### Face Fix Compositing Fix
- Fixed a square-box artifact in the face fix node caused by incorrect mask compositing — replaced hard-cutoff blending with smooth cosine falloff

### i18n
- Compare Grid strings fully localized in all 11 languages

---

## What's New in v0.6.5

### Scroll-to-Adjust Sliders
- **Click-to-capture scroll wheel** — click any slider thumb or its value label to "capture" it, then use the mouse scroll wheel anywhere on the page to adjust the value. Click outside the slider to release
- **Glow indicator** — a pulsing indigo glow animation highlights the captured slider so you always know which control the scroll wheel is adjusting
- Applied to all 20 range inputs: Steps, CFG, Batch, Denoise, Scale, Tile Size, Soft Guidance, Face Fix (denoise/steps/guide size), ControlNet (strength/start%/end%), LoRA strengths, and card size sliders

### Windows Updater Fix
- Changed Windows update installer to `quiet` mode — the previous `passive` mode still showed the uninstall/reinstall wizard on some systems. Quiet mode runs the update entirely in the background with no UI

---

## What's New in v0.6.4

### UI Polish
- **Card size sliders** — bottom panel Images and LoRA tabs now each have a range slider to adjust card size on the fly (persisted across sessions)
- **Always-visible cancel button** — the Cancel button is now always shown in the generation footer; greyed out when idle, red when a generation is running
- **Swap panels button** — new horizontal-arrows button next to the mode selector to swap left/right generation panels
- **Autocomplete mid-prompt fix** — tag autocomplete now works correctly when the cursor is in the middle of a prompt, not just at the end
- **Button spacing** — slightly increased gap between Generate and Cancel buttons; Cancel button is wider for easier targeting
- **Taller generation footer** — increased bottom padding on the sticky footer to prevent overlap with the Windows taskbar

### Open Model Folders
- New "Open Model Folders" section in Settings → Paths with buttons to open each model category directory (Checkpoints, LoRAs, VAE, Upscalers, Face Fix, Embeddings, ControlNet, CLIP/T.Enc, Diffusion) directly in the native file explorer
- If a category has multiple configured directories, a picker dialog lets you choose which one to open
- Directories are created automatically if they don't exist yet

### Windows Updater Fix
- Reverted Windows update installer to `passive` mode — fixes a regression in v0.6.2 where the installer showed a full uninstall/reinstall wizard instead of updating silently

### i18n
- Added all new UI strings to all 11 supported languages (de, es, fr, it, ja, ko, pt, ru, zh, zh-tw, en)

---

## What's New in v0.6.3

### Nanosaur 1.2B Support
- Full support for the Nanosaur 1.2B-Preview model — a 1.2B parameter DiT with 96-channel DINOv3 VAE and Gemma 3 text encoder
- Custom ComfyUI nodes (NanoSaurLoader, NanoSaurLatentFormat, VAE wrapper) are auto-deployed on startup
- Architecture auto-detection applies recommended settings: 40 steps, CFG 7, euler sampler, simple scheduler, 896×1152 resolution
- Sampler settings panel shows a Nanosaur recommendation bar with one-click apply
- Quality tag customization in Settings for Nanosaur models
- Latent preview support with Ridge-regularised RGB factors derived from the full VAE encoder

### Windows Clipboard Performance
- Clipboard copy on Windows is now instant — uses `SetFileDropList` instead of decoding/re-encoding the image through .NET `System.Drawing`
- Preserves PNG metadata (generation parameters) in the copied file

### Bug Fixes
- Fixed error messages being invisible on dark theme (text-red-800 → text-red-400)

### i18n
- Added Nanosaur locale keys to all 11 supported languages

---

## What's New in v0.6.2

### Update Reliability Improvements
- Added version mismatch detection: if an update is applied but the running version doesn't match the expected version, a warning is shown with a link to download manually
- Windows update installer now uses `basicUi` mode instead of silent `passive` mode, making the update progress visible and reducing cases where the installer appeared to hang

### i18n
- Added `updater.version_mismatch` translation key to all 11 supported locales (de, es, fr, it, ja, ko, pt, ru, zh, zh-tw)

---

## What's New in v0.6.1

### Fixed Lightbox Metadata for Session Images
- Fixed metadata panel showing empty in the lightbox when viewing images from the preview pane or bottom panel
- Session images now display their generation parameters (prompt, model, sampler, seed, etc.) immediately without waiting for the async gallery save to complete
- Previously the lightbox only loaded metadata from disk, ignoring in-memory metadata that session images already had

---

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
