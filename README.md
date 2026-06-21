# MooshieUI

MooshieUI is a beginner-friendly interface for [ComfyUI](https://github.com/comfyanonymous/ComfyUI) that runs in two modes:
- **Desktop app** via Tauri (Windows/Linux, macOS source build)
- **Browser/server mode** via the built-in web server (LAN/Docker friendly, mobile UI)

Built with **Svelte 5** + **Rust**, it hides ComfyUI's node-graph complexity behind a clean, guided workflow so you can generate without hand-editing graphs.

![License](https://img.shields.io/github/license/Mooshieblob1/MooshieUI?v=2)

<p align="center">
  <img src="src/lib/assets/logo.png" alt="Logo" width="200">
</p>

![MooshieUI Screenshot](screenshot.avif)

---

## 📚 Documentation

Full guides live in the **[MooshieUI Wiki](https://github.com/Mooshieblob1/MooshieUI/wiki)**:

| Guide | Covers |
|-------|--------|
| [Installation](https://github.com/Mooshieblob1/MooshieUI/wiki/Installation) | Desktop, Docker, remote/cloud ComfyUI, macOS source build |
| [Generation Basics](https://github.com/Mooshieblob1/MooshieUI/wiki/Generation-Basics) | Modes, generation controls, dimensions |
| [Prompting Guide](https://github.com/Mooshieblob1/MooshieUI/wiki/Prompting-Guide) | Autocomplete, presets, wildcards, interrogation |
| [Prompt Assistant](https://github.com/Mooshieblob1/MooshieUI/wiki/Prompt-Assistant) | LLM-assisted prompt building |
| [Models & the Model Hub](https://github.com/Mooshieblob1/MooshieUI/wiki/Models-and-the-Model-Hub) | Supported architectures, auto-detection, downloads |
| [Upscaling & Face Fix](https://github.com/Mooshieblob1/MooshieUI/wiki/Upscaling-and-Face-Fix) | Tiled diffusion, guidance nodes, face fix |
| [ControlNet & Style Transfer](https://github.com/Mooshieblob1/MooshieUI/wiki/ControlNet-and-Style-Transfer) | ControlNet and reference/style transfer |
| [Inpainting & the Canvas Editor](https://github.com/Mooshieblob1/MooshieUI/wiki/Inpainting-and-the-Canvas-Editor) | Mask painting and selective edits |
| [Compare Grid](https://github.com/Mooshieblob1/MooshieUI/wiki/Compare-Grid) | XYZ parameter sweeps |
| [Gallery & Metadata](https://github.com/Mooshieblob1/MooshieUI/wiki/Gallery-and-Metadata) | Persistent gallery, metadata import/remix |
| [Server, LAN & Multi-User](https://github.com/Mooshieblob1/MooshieUI/wiki/Server,-LAN-and-Multi-User) | Self-hosting, roles, auth, mobile |
| [Settings & Accessibility](https://github.com/Mooshieblob1/MooshieUI/wiki/Settings-and-Accessibility) | Persistence, i18n, accessibility |
| [FAQ](https://github.com/Mooshieblob1/MooshieUI/wiki/FAQ) | Common questions |

---

## ✨ Highlights

- **Three generation modes** - text to image, image to image, and inpainting with a built-in canvas/mask editor; settings carry over between modes.
- **Full generation controls** - searchable checkpoint/VAE/LoRA pickers with auto-download, all ComfyUI samplers and schedulers, steps/CFG/seed/batch, and smart dimension presets.
- **Smart model detection** - 20+ architectures (SD 1.5, SDXL, Illustrious/NoobAI, Pony, SD3/3.5, the Flux family, Chroma, Z-Image, Wan, Qwen, AuraFlow, PixArt, HunyuanDiT, Stable Cascade, Kolors, Anima, Mugen, Nanosaur) identified by SHA256 hash, each with auto-applied sampler/scheduler/CFG presets.
- **Tiled-diffusion upscaling** - MultiDiffusion/SpotDiffusion with anti-hallucination guidance nodes, one-click upscale, and YOLOv8 face fix.
- **Compare Grid (XYZ)** - per-cell parameter sweeps stitched into a single labelled image.
- **Real-time feedback** - live latent previews, progress phases, and cancel, streamed over WebSocket.
- **Gallery & metadata** - persistent SQLite-backed gallery; drag a PNG back in to restore its settings (SwarmUI/A1111 metadata + stealth alpha).
- **Self-hostable** - headless web server with roles, per-user galleries, auth, and a dedicated mobile layout.
- **11 languages** - 2,000+ translation keys with full locale parity, switchable without restart.

See the [Wiki](https://github.com/Mooshieblob1/MooshieUI/wiki) for the full feature reference.

---

## 📦 Quick Start

### Desktop (Windows/Linux)

1. Download a release from [Releases](https://github.com/Mooshieblob1/MooshieUI/releases).
2. Run the app. The setup wizard downloads uv, Python, ComfyUI, and PyTorch (GPU auto-detected) and installs MooshieUI's custom nodes - no Python or pip setup required.
3. Start generating; ComfyUI launches automatically.

> ~5–10 GB disk, 5–15 minutes on first launch. macOS, Docker, and remote/cloud ComfyUI setups are covered in [Installation](https://github.com/Mooshieblob1/MooshieUI/wiki/Installation).

### Self-host (Docker)

```bash
cp .env.example .env   # optional: credentials/ports
docker compose up -d --build
```

Open `http://localhost:3200` (or your configured `MOOSHIEUI_PORT`). Full server/LAN/multi-user setup: [Server, LAN & Multi-User](https://github.com/Mooshieblob1/MooshieUI/wiki/Server,-LAN-and-Multi-User).

### Build from source

```bash
git clone https://github.com/Mooshieblob1/MooshieUI.git
cd MooshieUI
npm install
npm run tauri dev      # hot-reload dev
npm run tauri build    # production build
```

---

## 🏗️ How it works

1. You adjust settings in the Svelte UI.
2. On Generate, settings go to the Rust backend via the IPC bridge (`ipcInvoke()` on desktop, HTTP/SSE in browser mode).
3. Rust builds a ComfyUI workflow JSON from templates - no node graph exposed.
4. The workflow is submitted to ComfyUI's `/prompt` API.
5. WebSocket streams progress and previews back to the UI in real time.

MooshieUI also ships custom ComfyUI nodes (tiled diffusion, soft/smart guidance, an SDXL↔Flux2 VAE adapter, Nanosaur DiT support, and face fix) that are auto-installed into ComfyUI. Details live in [Models & the Model Hub](https://github.com/Mooshieblob1/MooshieUI/wiki/Models-and-the-Model-Hub). The tiled diffusion node is also available as a standalone ComfyUI custom node: [ComfyUI-MooshieTiledDiffusion](https://github.com/Mooshieblob1/ComfyUI-MooshieTiledDiffusion).

---

## 🛠️ Tech Stack

| Layer | Technology |
|-------|------------|
| Frontend | Svelte 5, TypeScript 6, Tailwind CSS 4 |
| Runtime | Tauri desktop app + axum headless web server |
| State | Svelte 5 runes - class-based singleton stores |
| Persistence | Tauri Store (JSON) + SQLite (`rusqlite`) |
| ComfyUI transport | REST + WebSocket via Rust (reqwest, tokio-tungstenite) |
| Inference | ONNX Runtime (`ort`) for WD EVA02 image interrogation |
| Autocomplete | Danbooru + Anima tag databases (~140k tags) |
| i18n | 11 languages, 2,000+ keys, runtime switching |
| Build | Vite 6 + `@sveltejs/vite-plugin-svelte` |

---

## 🔒 Security

Automated **GlassWorm resistance checks** run on every push and pull request to catch supply-chain attacks that hide payloads in invisible Unicode variation selectors or tamper with git timestamps. The CI workflow (`.github/workflows/glassworm-scan.yml`) blocks merges on failure. Contributors should enable the same checks locally:

```bash
bash scripts/setup-hooks.sh
```

---

## 🤝 Contributing

Pull requests are welcome. `main` is protected: open a PR from a `chore/<topic>` branch after local validation and GlassWorm pre-commit checks. See **[push-instructions.md](push-instructions.md)** for the full workflow (branch naming, build gates, IPC/gallery conventions, and CI).

---

## 📋 Changelog

See [CHANGELOG.md](CHANGELOG.md) for the full version history.

---

## 📄 License

Licensed under the [GNU Affero General Public License v3.0](LICENSE).

---

## 🙏 Acknowledgments

Built on [ComfyUI](https://github.com/comfyanonymous/ComfyUI), [Tauri](https://tauri.app/), [Svelte](https://svelte.dev/), and [Tailwind CSS](https://tailwindcss.com/). Uses [CivitAI](https://civitai.com/) and [HuggingFace](https://huggingface.co/) for model data, [Danbooru](https://danbooru.donmai.us/) tags, [OmniSR](https://huggingface.co/Acly/Omni-SR) upscalers, [YOLOv8](https://docs.ultralytics.com/) face detection, and [Konva](https://konvajs.org/) for the canvas editor. Tiled diffusion implements MultiDiffusion (Bar-Tal et al., ICML 2023) and SpotDiffusion (Ding et al., 2024).
