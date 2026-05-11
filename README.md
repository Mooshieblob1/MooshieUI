# MooshieUI

> ⚠️ **Work in Progress** — MooshieUI is under active development. Some features are incomplete or may have rough edges. Contributions and feedback are welcome!

A modern, beginner-friendly desktop frontend for [ComfyUI](https://github.com/comfyanonymous/ComfyUI). Built with **Svelte 5** + **Tauri** (Rust), MooshieUI hides ComfyUI's node-graph complexity behind a clean, intuitive interface — no workflow editing required.

![License](https://img.shields.io/github/license/Mooshieblob1/MooshieUI?v=2)

<p align="center">
  <img src="src/lib/assets/logo.png" alt="Logo" width="200">
</p>

![MooshieUI Screenshot](screenshot.avif)

---

## ✨ Features

> Features marked with ✅ are implemented. Features marked with 🔧 work but need polish.

### 🎨 Three Generation Modes

| Mode | Status | Description |
|------|--------|-------------|
| **Text to Image** | ✅ | Generate images from scratch using positive & negative prompts |
| **Image to Image** | ✅ | Transform existing images with adjustable denoise strength |
| **Inpainting** | ✅ | Selectively edit parts of images using a built-in canvas editor with mask painting |

Switch between modes with a single click — all settings carry over.

### 🔧 Full Generation Controls

- **Positive & Negative Prompts** — Multi-line text areas, manually resizable height
- **Checkpoint Selector** — Searchable dropdown, auto-populated from ComfyUI, with recommended models (auto-download on selection with progress bars and file size display)
- **VAE Selector** — Optional override (defaults to checkpoint's built-in VAE)
- **LoRA Support** — Add unlimited LoRAs with independent model/CLIP strength sliders (0–2), per-LoRA enable/disable toggle, searchable dropdown, and active count badge
- **Sampler & Scheduler** — All ComfyUI samplers and schedulers available
- **Steps** — 1 to 150 (slider)
- **CFG Scale** — 0 to 30 with 0.1 precision (number input + slider)
- **Seed** — Fixed or random (-1 for new seed each generation)
- **Batch Size** — Generate 1–8 images per prompt
- **Denoise Strength** — 0 to 1 for img2img and inpainting modes

### 📐 Smart Dimension Controls

- **Aspect Ratio Presets** — 1:1, 4:3, 3:2, 16:9, 21:9, 3:4, 2:3, 9:16
- **Custom Aspect Ratio** — Enter any width/height, ratio is maintained when adjusting resolution
- **Swap Dimensions** — One-click width↔height swap
- **Resolution Slider** — 64px to 2048px, automatically calculates dimensions from your aspect ratio

### 🔍 Upscale (Tiled Diffusion)

Built-in upscaling with **MultiDiffusion** tiled diffusion — the same approach used by SwarmUI. No slow tile-by-tile processing; all tiles are denoised simultaneously each step for seamless, high-quality results.

#### Upscale Methods
- **Model-based** — Uses dedicated upscale models (e.g., Omni-SR). Scale is determined by the model (2x, 4x, etc.)
- **Algorithmic (Lanczos)** — Fast pixel-space upscaling with adjustable 1–4x scale

#### Recommended Models (Auto-Download)
When you select a recommended model that isn't installed, MooshieUI automatically downloads it to ComfyUI's `models/upscale_models/` directory:

| Model | Scale | Size | Source |
|-------|-------|------|--------|
| **Omni 2x** (Recommended) | 2x | ~1.6 MB | [Acly/Omni-SR](https://huggingface.co/Acly/Omni-SR) |
| **Omni 4x** (Recommended) | 4x | ~1.6 MB | [Acly/Omni-SR](https://huggingface.co/Acly/Omni-SR) |

Any other upscale models you place in `models/upscale_models/` will also appear in the dropdown.

#### Tiled Diffusion (Optional)
- Toggle on/off per generation — recommended for large images and **required for Anima (COSMOS) models**
- Adjustable tile size (256–2048px)
- Uses cosine-feathered blending for seamless tile boundaries
- Supports both **MultiDiffusion** and **SpotDiffusion** algorithms

#### Guidance Nodes (Anti-Hallucination)
- **Soft Guidance** (CFG Rescale) — toggle in Upscale Settings to reduce extra hands, objects, and other hallucinations at low denoise; adjustable multiplier (0.0–1.0, default 0.4)
- **Smart Guidance** (Positive-Biased Adaptive) — toggle in Sampler Settings to bias the model toward positive conditioning across all passes
- Custom ComfyUI nodes (`MooshieSoftGuidance`, `MooshieSmartGuidance`) auto-installed alongside tiled diffusion

#### Upscale Sampler Controls
- **Denoise** — 0 to 1 (lower = more detail preservation from original)
- **Steps** — 1 to 50

#### One-Click Upscale
Hover over any generated image to reveal an **Upscale** button — instantly upscale the last output without changing your settings.

### 🖼️ Gallery

- **Persistent Gallery** — All generated images are saved to disk and available across sessions
- **Thumbnail Grid** — Responsive grid with sorting by date
- **Lightbox** — Click any image to view full-size; scroll-wheel zoom at cursor, Escape or click-outside to close, double-click to reset zoom
- **Image Management** — Rename, delete, copy to clipboard, and upscale from the gallery
- **Generation Mode Labels** — Each image shows whether it was created via txt2img, img2img, or inpainting

### � Compare Grid (XYZ Grid)

- **Multi-cell comparison** — create a grid of cells in the Compare tab (bottom panel), each with its own generation parameters; tweak prompts, checkpoints, samplers, seeds, or any setting per cell
- **Grid generation** — pressing Generate queues all cells sequentially with a shared random seed for consistent side-by-side comparison
- **Auto-stitching** — completed grids are stitched into a single labelled image with per-cell annotations showing only what differs (e.g., "blue eyes" vs "green eyes") and a MooshieUI watermark
- **Spreadsheet naming** — cells use A1/B1/C1 labels with position-stable colors
- **Add/remove columns & rows** — new cells clone the adjacent neighbor for quick parameter variation

### �📊 Real-Time Progress

- **Live Preview** — See the image as it's being generated (latent previews streamed via WebSocket)
- **Phase Labels** — "Generating...", "Upscaling...", or "Preparing..." with step counter
- **Progress Bar** — Smooth animated bar (indigo for generation, emerald for upscale pass)
- **Cancel Button** — Interrupt any generation in progress

### 🌐 Internationalization (11 Languages)

- **11 languages** — English, German, Spanish, French, Italian, Japanese, Korean, Portuguese, Russian, Chinese (Simplified), and Chinese (Traditional)
- **860+ translation keys** covering every UI string — generation controls, settings, setup wizard, canvas, model hub, compare grid, tooltips, and more
- **Instant switching** in Settings → Appearance — no restart needed
- **Full parity** — all keys present in all locales with native translations

### 🏷️ Quality Tags

- **Anima & Illustrious** — customizable positive/negative quality tags auto-injected into prompts
- **Pony Diffusion** — auto-applied score-based quality tags (`score_9, score_8_up, score_7_up, source_anime`)
- All customizable in Settings and persisted across sessions

### 💾 Settings Persistence

All settings are automatically saved to disk and restored on next launch:
- Generation mode, prompts, model selections
- Sampler, scheduler, steps, CFG, seed, dimensions
- All upscale settings (enabled, method, model, tiling, etc.)

### 🖥️ Flexible Layout

- **Three-panel layout** — Image settings (left), preview (center), model & sampler settings (right)
- **Resizable panels** — Drag dividers between panels to adjust widths
- **Resizable prompts** — Drag prompt text areas to adjust height

### 🔌 Connection Management

- **Auto-connect** to ComfyUI on startup
- **Status indicator** — Green/red dot shows connection state with version number
- **WebSocket streaming** — Real-time progress, previews, and completion events
- Works with both local and remote ComfyUI instances
- **Silent background process** — ComfyUI runs without any visible terminal windows (Windows)

### 🧬 Smart Model Detection & Architecture Presets

- **13 model architectures** — SD 1.5, SDXL, Illustrious/NoobAI, SD3/SD3.5, Flux, Pony Diffusion, AuraFlow, PixArt, HunyuanDiT, Stable Cascade, Kolors, Mugen (Flux2VAE SDXL), and Nanosaur (1.2B DiT)
- **Auto-presets** — each architecture auto-applies optimal sampler, scheduler, steps, CFG, and resolution when selected
- **Accelerated model detection** — models with "turbo", "lightning", "lcm", or "hyper" in the name get reduced steps (4–6), lower CFG, and appropriate settings
- **Rectified flow scheduling** — SD3, Flux, AuraFlow, Mugen, and Nanosaur models automatically inject the correct ModelSampling node with architecture-specific shift values
- **FluxGuidance** — Flux Dev models auto-inject a FluxGuidance node; Flux Schnell (guidance-distilled) is detected and skipped
- **Hash-based identification** — Models are recognized by SHA256 hash (CivitAI AutoV2 format), not just filename — renamed files are still detected
- **CivitAI integration** — Look up any model's metadata (name, version, preview images) via CivitAI's hash database
- **Recommended models** — ΣIH-1.5 (~7.5 GB) and Anima Preview 2 (~13 GB) auto-download on selection with real-time progress bars and file size display
- **Mugen support** — SDXL models using the Flux2 VAE (128-channel latents) with rectified flow scheduling, auto-detected and routed through dedicated VAE conversion nodes
- **Nanosaur support** — 1.2B DiT architecture with 96-channel VAE, custom ComfyUI nodes for model loading, text encoding, and VAE decode, auto-installed alongside MooshieUI's node pack

---

## 🏗️ Architecture

```
MooshieUI
├── src/                    # Svelte 5 frontend (UI)
│   ├── App.svelte          # Main app shell, gallery, WebSocket listeners, grid stitching
│   ├── lib/
│   │   ├── components/     # UI components
│   │   │   ├── generation/ # Model selector, prompts, dimensions, upscale, compare grid
│   │   │   ├── canvas/     # Inpainting canvas editor (Konva)
│   │   │   ├── mask-editor/# Mask painting for inpainting
│   │   │   ├── modelhub/   # CivitAI/HuggingFace model browser
│   │   │   ├── downloads/  # Download progress manager
│   │   │   ├── progress/   # Live preview and progress display
│   │   │   ├── queue/      # Batch queue management
│   │   │   ├── settings/   # Settings pages (paths, appearance, accessibility)
│   │   │   ├── setup/      # Setup wizard with streaming installer
│   │   │   ├── updater/    # In-app auto-update UI
│   │   │   └── ui/         # Shared UI components (tooltips, etc.)
│   │   ├── stores/         # Svelte 5 rune-based state ($state, $derived)
│   │   ├── assets/         # Danbooru & Anima tag databases, logo
│   │   ├── config/         # ControlNet presets
│   │   ├── locales/        # 11 language translation files
│   │   ├── types/          # TypeScript interfaces
│   │   └── utils/          # Tauri API bridge (models, gallery, hashing, CivitAI)
├── src-tauri/              # Rust/Tauri backend
│   └── src/
│       ├── commands/       # Tauri command handlers (API, config, server, WebSocket, workflow)
│       ├── comfyui/        # ComfyUI API client, WebSocket, process management
│       ├── setup.rs        # One-click installer (uv, Python, ComfyUI, PyTorch)
│       ├── metadata.rs     # PNG metadata embedding (text chunk + stealth alpha)
│       └── templates/      # Workflow builders (txt2img, img2img, inpainting, upscale, facefix, controlnet)
└── comfyui-nodes/          # Custom ComfyUI nodes (auto-installed)
    ├── nodes_tiled_diffusion.py  # MultiDiffusion & SpotDiffusion
    ├── nodes_guidance.py         # Soft & Smart Guidance
    ├── nodes_sdxl_flux2vae.py    # SDXL↔Flux VAE adapter (Mugen)
    └── nanosaur_support/         # Nanosaur DiT model loader & VAE
```

**How it works:**
1. User adjusts settings in the Svelte UI
2. On "Generate", settings are sent to the Rust backend via Tauri `invoke()`
3. Rust builds a ComfyUI workflow JSON from templates (no node graph exposed)
4. Workflow is submitted to ComfyUI's `/prompt` API
5. WebSocket streams progress/previews back to the UI in real-time

---

## 📦 Installation

### One-Click Setup (Windows/Linux Releases)

MooshieUI handles everything automatically on first launch:

1. **Download** a release from [Releases](https://github.com/Mooshieblob1/MooshieUI/releases) (Windows/Linux artifacts)
2. **Run the app** — the setup wizard will:
  - Download [uv](https://github.com/astral-sh/uv) (fast Python package manager)
  - Install Python 3.11 (isolated, won't affect your system)
  - Download ComfyUI (latest from GitHub)
  - Create a virtual environment
  - Auto-detect your GPU (NVIDIA CUDA / AMD ROCm / Intel XPU / CPU)
  - Install PyTorch with the right acceleration backend (including CUDA 13.0 for Blackwell GPUs)
  - Install all ComfyUI dependencies
  - Install MooshieUI's custom nodes
3. **Start generating** — ComfyUI launches automatically

The installer shows real-time terminal output streamed as a matrix-style backdrop behind the setup UI, with per-step progress bars and a checklist so you always know what's happening. No separate terminal windows are opened.

**No Python, no pip, no manual configuration required.** Everything is self-contained in the app's data directory.

> **Disk space:** ~5–10 GB (Python + PyTorch + ComfyUI). Installation takes 5–15 minutes depending on your internet connection.

### macOS (Manual Build From Source)

macOS prebuilt release artifacts are currently disabled. On macOS, use a source build:

**Prerequisites:**
- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) (latest stable)
- Xcode Command Line Tools (`xcode-select --install`)
- Tauri prerequisites — see [Tauri v2 docs](https://v2.tauri.app/start/prerequisites/)

```bash
# Clone the repository
git clone https://github.com/Mooshieblob1/MooshieUI.git
cd MooshieUI

# Install dependencies
npm install

# Development mode
npm run tauri dev

# Production build
npm run tauri build
```

After first launch, the setup wizard still installs ComfyUI/Python/PyTorch automatically.

### Development Setup (All Platforms)

If you want to build from source:

**Prerequisites:**
- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) (latest stable)
- Tauri prerequisites — see [Tauri v2 docs](https://v2.tauri.app/start/prerequisites/)

```bash
# Clone the repository
git clone https://github.com/Mooshieblob1/MooshieUI.git
cd MooshieUI

# Install frontend dependencies
npm install

# Run in development mode (hot-reload)
npm run tauri dev

# Build for production
npm run tauri build
```

The app will run the one-click setup wizard on first launch — no manual ComfyUI installation needed.

---

## 🧩 Custom Nodes

MooshieUI ships with custom ComfyUI nodes that are auto-installed into ComfyUI's `custom_nodes/` directory.

### Tiled Diffusion (`nodes_tiled_diffusion.py`)

### MultiDiffusion
*Bar-Tal et al., "MultiDiffusion: Fusing Diffusion Paths for Controlled Image Generation", ICML 2023*

- Splits the latent into overlapping tiles at each denoising step
- All tiles are denoised in parallel (not one-by-one)
- Results are blended using cosine (Hann window) feathering
- Seamless output with no visible tile boundaries

### SpotDiffusion
*Ding et al., 2024*

- Applies random circular shifts before non-overlapping tiling
- Even faster than MultiDiffusion (no overlap computation)
- Seams are eliminated by randomization across many steps

Both methods:
- Work with all model architectures (SD 1.5, SDXL, Flux, COSMOS/Anima)
- Automatically detect the model's latent downscale ratio
- Support ControlNet (proportional cropping/shifting per tile)
- Handle inpainting conditioning (c_concat)

### Guidance Nodes (`nodes_guidance.py`)

**MooshieSoftGuidance** — CFG Rescale for upscale passes. Rescales classifier-free guidance to reduce hallucinated details (extra hands, objects) that appear at low denoise strengths. Adjustable multiplier (0.0 = off, 0.4 = recommended for upscale).

**MooshieSmartGuidance** — Positive-Biased Adaptive guidance. Patches the model's forward pass to bias toward positive conditioning, reducing negative prompt interference. Applied globally across all generation passes.

### SDXL↔Flux VAE Adapter (`nodes_sdxl_flux2vae.py`)

Patches the SDXL model forward pass to convert between packed (128-channel) Flux2 latents and standard (32-channel) SDXL latents. Used automatically for **Mugen** models that pair an SDXL UNet with the Flux VAE for higher-fidelity decoding.

### Nanosaur Support (`nanosaur_support/`)

Custom ComfyUI nodes for the **Nanosaur** 1.2B DiT architecture:
- **NanoSaurModelLoader** — loads the DiT transformer with correct configuration
- **NanoSaurTextEncoder** — tokenizer and text encoder adapted for Nanosaur's conditioning format
- **NanoSaurVAEDecode** — decodes 96-channel latents through Nanosaur's custom VAE
- RGB factors computed from the model's VAE for accurate latent previews

### Face Fix (`mooshie_nodes.py`)

**MooshieFaceFix** — lightweight face detection and re-denoising using YOLOv8 (via the `ultralytics` Python package). Detects faces, crops them with configurable padding, re-denoises at higher resolution, then composites back using smooth cosine-falloff blending. No Impact Pack dependency — bundled as a self-contained node with auto-download of detection models (`.pt` PyTorch weights).

---

## 🚧 Roadmap

### Done
- [x] **Image upload** for img2img and inpainting modes
- [x] **Inpainting canvas** — paint masks directly on images in the UI
- [x] **Queue management page** — view and cancel queued generations
- [x] **Settings page** — configure ComfyUI connection, paths, defaults, and extra args
- [x] **Gallery upscale button** — upscale any image from the gallery grid or lightbox
- [x] **Anima Preview 2 support** — auto-download split model (diffusion + CLIP + VAE), quality prompt injection, optimized defaults
- [x] **ΣIH-1.5 support** — auto-download checkpoint + VAE on selection, with file size display
- [x] **CFG++ auto-detect** — soft-sets CFG to 1.4 when selecting CFG++ samplers
- [x] **Info tooltips** — hover (?) icons explain technical settings in plain English
- [x] **Collapsible settings sections** — with search bar to filter settings
- [x] **Live generation preview** — latent previews streamed via WebSocket during KSampler
- [x] **Phase-aware progress** — shows "Generating..." / "Upscaling..." / "Preparing..." with step counters
- [x] **5D latent tiled diffusion** — MultiDiffusion/SpotDiffusion compatible with Anima (COSMOS) models
- [x] **Lightbox zoom & dismiss** — scroll-wheel zoom at cursor, Escape/click-outside to close, pan with left or middle mouse
- [x] **Clipboard copy as file** — copies gallery images as file references (preserves format & metadata)
- [x] **Windows & Linux builds** — cross-platform CI releases (Windows .msi/.exe, Linux .deb/.AppImage)
- [x] **Hash-based model detection** — SHA256/AutoV2 hash identification with CivitAI API integration, models recognized even if renamed
- [x] **Installer UX overhaul** — streamed terminal backdrop, per-step progress bars, download progress with bytes/total, no separate terminal windows
- [x] **Persistent gallery** — images saved to disk across sessions with rename, delete, and management
- [x] **Gallery boards** — organize images into named boards/folders
- [x] **Version display** — app version shown in sidebar below connection status
- [x] **Download progress** — real-time progress bars with file size for all model downloads (checkpoints, VAEs, upscale models)
- [x] **Drag & drop** — drop images and masks directly into img2img/inpainting inputs
- [x] **Image metadata (SwarmUI-compatible)** — embed generation params as SwarmUI JSON into PNG text chunks, read them in lightbox with resizable side panel, backward-compatible with A1111 format
- [x] **Prompt history & favorites** — auto-saves generated prompts with quick reload and starring
- [x] **Style presets (Fooocus-style)** — one-click style modifiers for beginner-friendly prompting
- [x] **Shared model directory** — point to an external/shared models folder from settings
- [x] **Model-specific presets** — auto-applies defaults based on detected model architecture
- [x] **Model Hub** — browse CivitAI models with image previews and metadata directly in the app; download with one click; HuggingFace direct URL support; NSFW content filtering with blurred badges; API key setup with guided instructions; expanded base model filters (NoobAI, Pony, Illustrious, SD 3.5, Flux, etc.)
- [x] **ModelSpec support** — reads Stability AI ModelSpec metadata from safetensors headers; displays model title, author, architecture, resolution, trigger phrases (click to add to prompt), tags, and usage hints in the model selector
- [x] **Native clipboard** — copies images via native OS clipboard (Wayland `wl-copy` and X11 `xclip` with automatic detection)
- [x] **Auto-update** — check for and apply MooshieUI updates in-app
- **ControlNet support** — depth, canny, pose, and other control methods with preset-based and custom modes, image upload/paste/drag-drop, preprocessor installation, strength/start/end controls (SD 1.5, SDXL, Illustrious/NoobAI, Flux, SD3.5)
- [x] **Dark & light mode** — toggle between dark and light themes in settings
- [x] **Draggable two-column layout** — drag sections between left/right columns and reorder them; layout persists across sessions
- [x] **Manual ComfyUI start** — optional toggle to start ComfyUI manually instead of on app launch
- [x] **Movable installation** — relocate the ComfyUI data directory to another drive from settings
- [x] **Face Fix (FaceDetailer)** — built-in lightweight face detection and re-denoising using YOLOv8, bundled as a custom node (no Impact Pack dependency); configurable denoise, steps, guide size, and detector model with auto-download
- [x] **Batch queue** — queue multiple generations with different settings
- [x] **Streaming final outputs** — final PNGs stream over WebSocket via `MooshieSaveImage` (no output fetch disk round-trip)
- [x] **16-bit output mode** — selectable 8-bit/16-bit PNG output in generation settings
- [x] **Metadata modes** — `Text Chunk`, `Stealth Alpha`, and `Both` with 16-bit compatibility upgrade to `Both`
- [x] **Metadata reuse actions** — lightbox actions for `Reuse Settings`, `Remix`, and `Reuse Seed`
- [x] **Preview tips carousel** — idle preview area shows rotating, auto-cycling tips with manual navigation
- [x] **Metadata drag-and-drop import** — drag images onto generation sections to import specific settings, or drop on preview to apply all; Ctrl+V paste supported; auto-strips duplicate quality tags
- [x] **Face fix auto-setup** — auto-downloads detection model and installs ultralytics on first use
- [x] **Seed recall** — toggling off random seed recalls the last generated seed

- [x] **Localization** — 11 languages with 860+ keys, full parity across all locales, instant switching
- [x] **Guidance nodes** — Soft Guidance (CFG Rescale) and Smart Guidance (Positive-Biased Adaptive) for hallucination-free upscaling
- [x] **13 model architectures** — auto-detection with optimal presets for SD1.5, SDXL, Illustrious, SD3, Flux, Pony, AuraFlow, PixArt, HunyuanDiT, Stable Cascade, Kolors, Mugen, Nanosaur
- [x] **Accelerated model detection** — Turbo/Lightning/LCM/Hyper variants auto-detected with reduced steps and CFG
- [x] **Rectified flow scheduling** — SD3, Flux, AuraFlow, Mugen, Nanosaur, Stable Cascade auto-inject correct ModelSampling nodes
- [x] **FluxGuidance** — automatic guidance node injection for Flux Dev (skipped for Schnell)
- [x] **Pony quality tags** — auto-applied score-based tags, customizable in Settings
- [x] **Flux & SD3 ControlNet** — presets for XLabs-AI and Stability official controlnets
- [x] **BF16 VAE auto-detection** — Blackwell GPUs auto-apply `--bf16-vae` to prevent fp16 overflow
- [x] **NaN guard** — detects and clamps NaN values in VAE output to prevent black images
- [x] **CUDA 13.0 for Blackwell** — auto-detects compute capability ≥ 12.0 and installs cu130 PyTorch
- [x] **VRAM flush on interrupt** — calls `/free` endpoint after cancel to prevent corrupted state
- [x] **Wayland AppImage fix** — auto-detects Wayland sessions and preloads libwayland for WebKitGTK
- [x] **AMD multi-GPU fix** — correct ROCm architecture detection for RDNA 4 on mixed iGPU/dGPU systems
- [x] **Gallery import** — import image output folders from ComfyUI, SwarmUI, or any other tool with duplicate detection
- [x] **Export diagnostic logs** — single-file export with ComfyUI log, GPU info, and app configuration
- [x] **SwarmUI metadata compatibility** — auto-strips inline syntax from imported SwarmUI image metadata
- [x] **Info tips toggle** — hide/show tooltip icons via Settings → Accessibility
- [x] **Native clipboard image paste** — reads images directly from OS clipboard via Tauri command
- [x] **Compare grid** — XYZ grid comparison with per-cell parameters, shared seed generation, auto-stitching with diff labels and watermark
- [x] **Mugen support** — Flux2VAE SDXL architecture with 128-channel latents and rectified flow scheduling
- [x] **Nanosaur support** — 1.2B DiT with 96-channel VAE, custom nodes for model loading and inference
- [x] **Face fix compositing fix** — smooth cosine falloff blending replaces hard-cutoff mask compositing
- [x] **Scroll-to-adjust sliders** — click-to-capture scroll wheel for all range inputs with glow indicator

### To Do
- [ ] **Theme customization** — custom accent colors and themes
- [ ] **Video generation** — AnimateDiff / COSMOS video workflows
- [ ] **Training UI** — LoRA training from within the app
- [ ] **Plugin system** — extend MooshieUI with custom panels and features
- [x] **Cloud rendering** — the LAN/browser mode allows any device on the network to submit generations to the host GPU; combined with Docker deployment, this effectively enables remote/cloud GPU offloading without a dedicated cloud rendering backend
- [x] **PWA Support** — MooshieUI ships a built-in web server with full browser-mode support; users can self-host via Docker (`docker-compose.yml` included) and access the UI from any browser, functioning as a hosted web app with multi-user accounts and per-user galleries

---

## 📋 Changelog

See [CHANGELOG.md](CHANGELOG.md) for the full version history.

---

## ️ Tech Stack

| Layer | Technology |
|-------|------------|
| Frontend | Svelte 5, TypeScript 5, Tailwind CSS 4 |
| Desktop | Tauri v2 (Rust) |
| State | Svelte 5 runes (`$state`, `$derived`) — class-based singleton stores |
| Canvas | Konva + svelte-konva (inpainting editor, mask painting) |
| Persistence | `@tauri-apps/plugin-store` (JSON) |
| Backend API | ComfyUI REST + WebSocket (proxied via Rust) |
| HTTP Client | reqwest (Rust) — shared connection pool |
| WebSocket | tokio-tungstenite → Tauri event bridge |
| Inference | ONNX Runtime (`ort` crate) — WD EVA02 image tagger (Describe feature) |
| Model API | CivitAI REST API (hash-based model lookup), HuggingFace |
| Autocomplete | Danbooru + Anima tag databases (~100k tags) |
| i18n | 11 languages, 860+ keys, runtime switching |
| Styling | Tailwind CSS with neutral/indigo dark theme |
| Build | Vite 6 + `@sveltejs/vite-plugin-svelte` |

---

## 🔒 Security

This repository runs automated **GlassWorm resistance checks** on every push and pull request to detect a class of supply-chain attacks that use invisible Unicode variation-selector characters to embed hidden payloads in source files, combined with force-pushed commits whose author/committer timestamps have been tampered with to conceal the injection.

### What is checked

| Check | Scope | Details |
|-------|-------|---------|
| Marker variable | Full repo | Detects the known GlassWorm beacon string |
| Unicode steganography | `.py .js .ts .svelte .rs` | Scans for codepoints U+FE00–FE0F and U+E0100–E01EF (zero-width variation selectors used to encode payloads) |
| Git date tampering | Full history | Flags any commit where the committer timestamp is more than 1 hour ahead of the author timestamp — a sign of force-pushed history rewriting |
| Obfuscated `eval()` | `.py .js .ts .svelte` | Detects `eval()` calls whose argument contains `decode`, `atob`, `fromCharCode`, `Buffer.from`, or `base64` — the execution pattern used by the Unicode loader |

The CI workflow (`.github/workflows/glassworm-scan.yml`) runs all four checks and **blocks merges** if any check fails.

### Local pre-commit hook

Contributors should activate the same checks locally so issues are caught before they reach CI:

```bash
bash scripts/setup-hooks.sh
```

This sets `core.hooksPath` to `.githooks` and makes the pre-commit script executable. The hook runs automatically on every `git commit` and blocks the commit with a clear error message if anything suspicious is found.

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).

---

## 🙏 Acknowledgments

### Core Infrastructure
- [ComfyUI](https://github.com/comfyanonymous/ComfyUI) — The powerful node-based Stable Diffusion backend that MooshieUI wraps
- [Tauri](https://tauri.app/) — Lightweight, secure desktop app framework (Rust + WebView)
- [Svelte](https://svelte.dev/) — Reactive UI framework with rune-based state management
- [Tailwind CSS](https://tailwindcss.com/) — Utility-first CSS framework

### AI Models & Research
- **ΣIH** — Illustrious-family checkpoint with curated defaults, auto-download support
- **Anima (COSMOS)** — Breakthrough anime model with 5D latent space, split model loading (diffusion + CLIP + VAE), custom quality tags
- **Nanosaur** — 1.2B DiT architecture with 96-channel VAE; custom ComfyUI nodes for model loading, text encoding, and inference
- **Mugen** — SDXL models using Flux2 VAE (128-channel latents) with rectified flow scheduling
- [OmniSR](https://huggingface.co/Acly/Omni-SR) — Recommended lightweight upscale models (2x/4x) by Acly
- [YOLOv8](https://docs.ultralytics.com/) — Face detection model used by MooshieFaceFix via the `ultralytics` Python package (`.pt` weights)

### Research Papers
- Bar-Tal et al., "MultiDiffusion: Fusing Diffusion Paths for Controlled Image Generation" (ICML 2023) — Tiled diffusion algorithm
- Ding et al., "SpotDiffusion" (2024) — Fast tiled diffusion variant using random circular shifts

### APIs & Data
- [CivitAI](https://civitai.com/) — Model hash database, metadata API, and model marketplace
- [Danbooru](https://danbooru.donmai.us/) — Tag database used for prompt autocomplete
- [HuggingFace](https://huggingface.co/) — Model hosting and direct download support

### Frontend Libraries
- [Konva](https://konvajs.org/) + [svelte-konva](https://github.com/konvajs/svelte-konva) — HTML5 Canvas framework for the inpainting editor and mask painting
- [SortableJS](https://sortablejs.github.io/Sortable/) — Drag-and-drop reordering for the two-column layout
- [marked](https://marked.js.org/) — Markdown rendering for release notes display in-app
- [ntc-ts](https://www.npmjs.com/package/ntc-ts) — Nearest color name lookup

### Rust Crates
- [reqwest](https://docs.rs/reqwest) — HTTP client for ComfyUI API and model downloads
- [tokio-tungstenite](https://docs.rs/tokio-tungstenite) — WebSocket client for real-time progress streaming
- [ort](https://docs.rs/ort) — ONNX Runtime bindings for WD EVA02 tagger inference (Describe feature)
- [image](https://docs.rs/image) — Image processing (PNG, JPEG, WebP)
- [serde](https://serde.rs/) / [serde_json](https://docs.rs/serde_json) — Serialization for ComfyUI workflow JSON and config persistence

### Tauri Plugins
- `@tauri-apps/plugin-store` — JSON key-value persistence for settings
- `@tauri-apps/plugin-updater` — In-app auto-update with signature verification
- `@tauri-apps/plugin-fs` — Native filesystem access for gallery and model management
- `@tauri-apps/plugin-dialog` — Native file/folder picker dialogs
- `@tauri-apps/plugin-clipboard-manager` — Native clipboard operations (copy images as files)
- `@tauri-apps/plugin-shell` — Subprocess management for ComfyUI process lifecycle
