# MooshieUI

A modern, beginner-friendly desktop frontend for [ComfyUI](https://github.com/comfyanonymous/ComfyUI). Built with **Svelte 5** + **Tauri** (Rust), MooshieUI hides ComfyUI's node-graph complexity behind a clean, intuitive interface — no workflow editing required.

![License](https://img.shields.io/github/license/Mooshieblob1/MooshieUI)

---

## ✨ Features

### 🎨 Three Generation Modes

| Mode | Description |
|------|-------------|
| **Text to Image** | Generate images from scratch using positive & negative prompts |
| **Image to Image** | Transform existing images with adjustable denoise strength |
| **Inpainting** | Selectively edit parts of images using masks with grow-mask control |

Switch between modes with a single click — all settings carry over.

### 🔧 Full Generation Controls

- **Positive & Negative Prompts** — Multi-line text areas with auto-resize
- **Checkpoint Selector** — Searchable dropdown, auto-populated from ComfyUI
- **VAE Selector** — Optional override (defaults to checkpoint's built-in VAE)
- **LoRA Support** — Add unlimited LoRAs with independent model/CLIP strength sliders (0–2)
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
- **Model-based (ESRGAN)** — Uses dedicated upscale models. Scale is determined by the model (2x, 4x, etc.)
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

#### Upscale Sampler Controls
- **Denoise** — 0 to 1 (lower = more detail preservation from original)
- **Steps** — 1 to 50

#### One-Click Upscale
Hover over any generated image to reveal an **Upscale** button — instantly upscale the last output without changing your settings.

### 🖼️ Gallery

- **Thumbnail Grid** — All generated images in a responsive grid (3–5 columns)
- **Lightbox** — Click any image to view full-size with a close button
- **Session History** — All outputs from the current session are saved

### 📊 Real-Time Progress

- **Live Preview** — See the image as it's being generated (streamed via WebSocket)
- **Step Counter** — "Step 12 / 20" with percentage
- **Progress Bar** — Smooth animated bar
- **Cancel Button** — Interrupt any generation in progress

### 💾 Settings Persistence

All settings are automatically saved to disk and restored on next launch:
- Generation mode, prompts, model selections
- Sampler, scheduler, steps, CFG, seed, dimensions
- All upscale settings (enabled, method, model, tiling, etc.)

### 🔌 Connection Management

- **Auto-connect** to ComfyUI on startup
- **Status indicator** — Green/red dot shows connection state
- **WebSocket streaming** — Real-time progress, previews, and completion events
- Works with both local and remote ComfyUI instances

---

## 🏗️ Architecture

```
MooshieUI
├── src/                    # Svelte 5 frontend (UI)
│   ├── App.svelte          # Main app shell, gallery, WebSocket listeners
│   ├── lib/
│   │   ├── components/     # UI components (generation, progress)
│   │   ├── stores/         # Svelte 5 rune-based state ($state, $derived)
│   │   ├── types/          # TypeScript interfaces
│   │   └── utils/          # Tauri API bridge
├── src-tauri/              # Rust/Tauri backend
│   └── src/
│       ├── commands/       # Tauri command handlers
│       ├── comfyui/        # ComfyUI API client, WebSocket, process management
│       └── templates/      # Workflow builders (txt2img, img2img, inpainting, upscale)
└── comfyui-nodes/          # Custom ComfyUI nodes (install into comfy_extras/)
    └── nodes_tiled_diffusion.py
```

**How it works:**
1. User adjusts settings in the Svelte UI
2. On "Generate", settings are sent to the Rust backend via Tauri `invoke()`
3. Rust builds a ComfyUI workflow JSON from templates (no node graph exposed)
4. Workflow is submitted to ComfyUI's `/prompt` API
5. WebSocket streams progress/previews back to the UI in real-time

---

## 📦 Installation

### Prerequisites

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) (latest stable)
- [ComfyUI](https://github.com/comfyanonymous/ComfyUI) installed and running
- Tauri prerequisites — see [Tauri v2 docs](https://v2.tauri.app/start/prerequisites/)

### Setup

```bash
# Clone the repository
git clone https://github.com/Mooshieblob1/MooshieUI.git
cd MooshieUI

# Install frontend dependencies
npm install

# Install the custom ComfyUI nodes (required)
chmod +x comfyui-nodes/install.sh
./comfyui-nodes/install.sh /path/to/ComfyUI

# Restart ComfyUI to load the new nodes
```

### Development

```bash
# Run in development mode (hot-reload)
npm run tauri dev

# Build for production
npm run tauri build
```

### Configuration

On first launch, configure the ComfyUI connection in `src-tauri/src/config.rs`:

| Setting | Default | Description |
|---------|---------|-------------|
| `server_url` | `http://127.0.0.1:8188` | ComfyUI API URL |
| `server_port` | `8188` | ComfyUI port |
| `comfyui_path` | *(empty)* | Path to ComfyUI installation (for auto-launch & model downloads) |
| `venv_path` | *(empty)* | Python venv path (for auto-launch mode) |

---

## 🧩 Custom Node: Tiled Diffusion

MooshieUI ships with a custom ComfyUI node (`nodes_tiled_diffusion.py`) that implements:

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

---

## 🛠️ Tech Stack

| Layer | Technology |
|-------|------------|
| Frontend | Svelte 5, TypeScript, Tailwind CSS 4 |
| Desktop | Tauri v2 (Rust) |
| State | Svelte 5 runes (`$state`, `$derived`) |
| Persistence | `@tauri-apps/plugin-store` |
| Backend API | ComfyUI REST + WebSocket |
| Styling | Tailwind CSS with neutral/indigo theme |

---

## 📄 License

This project is open source. See [LICENSE](LICENSE) for details.

---

## 🙏 Acknowledgments

- [ComfyUI](https://github.com/comfyanonymous/ComfyUI) — The powerful node-based backend
- [Tauri](https://tauri.app/) — Lightweight desktop app framework
- [Svelte](https://svelte.dev/) — Reactive UI framework
- [OmniSR](https://huggingface.co/Acly/Omni-SR) — Recommended upscale models by Acly
- MultiDiffusion paper — Tiled diffusion algorithm
- SpotDiffusion paper — Fast tiled diffusion variant
