# MooshieUI — Development Notes (Sessions 1–3)

These notes document all work done across three development sessions, covering features implemented, bugs fixed, CI setup, and known pending work. Written for anyone picking up the project.

---

## Version Info

- **Current tag**: `v0.2.0` (on commit `c176472`)
- **Version in config files**: Still `0.1.0` in `tauri.conf.json`, `package.json`, and `Cargo.toml` — bump to `0.2.0` before the next release build so artifact filenames match the tag
- **Draft release**: <https://github.com/Mooshieblob1/MooshieUI/releases> — artifacts are named `0.1.0` due to the above; functional but cosmetically mismatched

---

## Features Implemented

### Two-Column Layout + Resizable Panels
- Replaced the original three-panel layout with a two-column design (settings left, preview/gallery right)
- Panels are resizable via draggable dividers
- Prompt text areas are independently resizable in height

### LoRA Management
- Add unlimited LoRAs with searchable dropdown
- Per-LoRA model/CLIP strength sliders (0–2 range)
- Per-LoRA enable/disable toggle
- Active count badge on the LoRA section header
- Remove individual LoRAs

### Anima Preview 2 Support
- Auto-download of split model files (diffusion transformer, CLIP, VAE) from Hugging Face
- Download progress events emitted to frontend
- Quality prompt injection for Anima models
- Optimized defaults when Anima is selected
- Split model loading in workflow templates (uses `UNETLoader` + `DualCLIPLoader` + `VAELoader` instead of single `CheckpointLoaderSimple`)

### Collapsible Settings Sections + Search
- All settings panels (prompts, model, sampler, dimensions, upscale, LoRA) are collapsible
- Search bar filters settings sections by keyword

### Info Tooltips
- `InfoTip.svelte` component — hover `(?)` icons that explain technical settings in plain English
- Applied to: CFG scale, sampler, scheduler, steps, denoise, seed, batch size, tiled diffusion, upscale method

### CFG++ Auto-Detect
- When selecting a CFG++ sampler variant, CFG scale automatically soft-sets to 1.4
- Reverts when switching back to a non-CFG++ sampler

### Live Generation Preview
- WebSocket binary frames from ComfyUI's KSampler are parsed in `websocket.rs`
- Preview images (type 1 = PREVIEW_IMAGE, type 4 = WITH_METADATA) are emitted as Tauri events
- Frontend displays the latest preview in the progress area during generation

### Phase-Aware Progress
- Progress bar shows phase labels: "Generating...", "Upscaling...", "Preparing..."
- Step counter (e.g., "Step 15/30")
- Color-coded bar: indigo for generation, emerald for upscale pass

### Lightbox Improvements
- **Escape key** closes the lightbox
- **Click outside** the image closes the lightbox
- **Scroll-wheel zoom** at cursor position
- **Double-click** resets zoom to fit

### Clipboard Copy (Cross-Platform)
- Copies the actual file as a URI reference (preserves format + metadata)
- **Linux**: `xclip -selection clipboard -t text/uri-list` (X11), falls back to `wl-copy --type text/uri-list` (Wayland)
- **macOS**: `osascript` with `set the clipboard to (POSIX file "...")`
- **Windows**: PowerShell `Set-Clipboard -Path '...'` with `CREATE_NO_WINDOW` flag
- Implementation: `copy_image_to_clipboard` in `src-tauri/src/commands/api.rs`

### Gallery Persistence
- Images saved to `~/.local/share/com.mooshieui.desktop/gallery/` (or platform equivalent)
- Commands: `save_to_gallery`, `list_gallery_images`, `load_gallery_image`, `delete_gallery_image`, `get_gallery_image_path`
- Gallery survives app restarts

### Gallery Upload for Upscaling
- `upload_image_bytes` command accepts raw bytes + filename (no temp file needed)
- Used by gallery upscale flow: load gallery image → upload to ComfyUI → set as img2img input

### Settings Page
- New `SettingsPage.svelte` component
- Sections: Connection (server mode, URL, port), Paths (ComfyUI, venv), Extra Args, Defaults
- Backed by `get_config` / `update_config` Tauri commands (`src-tauri/src/commands/config.rs`)
- Config persisted to `~/.local/share/com.mooshieui.desktop/config.json`

### Image Upload (img2img / inpainting)
- File dialog via `@tauri-apps/plugin-dialog` (`open()` with image filters)
- Uploads to ComfyUI via existing `upload_image` command
- Preview thumbnail shown in the UI
- Clear button to reset
- Mask upload for inpainting mode
- `growMaskBy` slider for inpainting

### Tiled Diffusion Enhancements
- 5D latent support for COSMOS/Anima models (video-style latents with temporal dimension)
- Both MultiDiffusion and SpotDiffusion algorithms
- Cosine-feathered blending for seamless tile boundaries
- ControlNet support (proportional cropping/shifting per tile)

---

## Cross-Platform Support

### GPU Detection (`setup.rs`)
- **NVIDIA**: `nvidia-smi` (all platforms)
- **AMD Linux**: `rocm-smi`
- **AMD Windows**: WMI `Win32_VideoController` via PowerShell
- **Apple**: defaults to "apple" on macOS

### VRAM Detection (`setup.rs`)
- **NVIDIA**: `nvidia-smi --query-gpu=memory.total`
- **AMD Linux**: `/sys/class/drm/card*/device/mem_info_vram_total` sysfs
- **AMD Windows**: WMI `Win32_VideoController.AdapterRAM` via PowerShell
- **macOS**: `system_profiler SPDisplaysDataType` — parses VRAM/Memory lines

### CI / GitHub Actions (`.github/workflows/release.yml`)
- Triggers on `v*` tags or manual `workflow_dispatch`
- Matrix: `ubuntu-22.04`, `windows-latest`, `macos-latest` (aarch64), `macos-latest` (x86_64 cross-compile)
- Uses `tauri-apps/tauri-action@v0` for build + release artifact upload
- Linux deps: `libwebkit2gtk-4.1-dev`, `libjavascriptcoregtk-4.1-dev`, `libappindicator3-dev`, `librsvg2-dev`, `patchelf`, `libssl-dev`, `libgtk-3-dev`, `libsoup-3.0-dev`
- Creates a draft release with download table and feature list

---

## Bugs Fixed

| Bug | Cause | Fix |
|-----|-------|-----|
| Clipboard copies garbled image data | Browser Clipboard API denied in Tauri; raw RGBA bytes too large for IPC | Copy file as URI reference using platform-native clipboard tools |
| App freezes on clipboard copy | Sending full RGBA pixel data through Tauri IPC locked main thread | Switched to file path approach (no pixel data over IPC) |
| Vite infinite reload loop | `cargo doc` generated thousands of HTML files in `src-tauri/target/doc/`, triggering Tauri's file watcher | `rm -rf target/doc/` — avoid running `cargo doc` in the project root |
| macOS CI runner failure | `macos-13` runner deprecated by GitHub Actions | Changed to `macos-latest` (ARM), cross-compiles x86_64 target |
| Windows CI build failure | Missing `icons/icon.ico` required for Windows resource file | Generated `.ico` from `icon.png` using ImageMagick: `convert icon.png -define icon:auto-resize=256,128,64,48,32,16 icon.ico` |
| macOS bundle identifier conflict | `com.mooshieui.app` ends with `.app`, conflicts with macOS bundle extension | Changed to `com.mooshieui.desktop` |
| Invalid bundle category | Tauri doesn't accept `"Graphics"` | Changed to `"Graphics and Design"` (Tauri's expected value) |

---

## Known Issues / Pending Work

### Must Do Before Next Release
- **Bump version to 0.2.0** in `tauri.conf.json`, `package.json`, `Cargo.toml` (currently still says 0.1.0)

### Feature Work (Planned)
- **Inpainting canvas** — paint masks directly on images in the UI
- **Queue management page** — view, reorder, cancel queued generations
- **UpscaleSettings UI for Anima** — hide denoise/steps/tiling controls when Anima is selected (KSampler refine pass is skipped for split models)
- **Prompt history & favorites**
- **Model manager** — browse/download/organize models from within the app
- **ControlNet support**
- **Image metadata** — embed generation parameters in PNG metadata
- **Drag & drop** images into img2img/inpainting
- **Auto-update** in-app

### Known Rough Edges
- Gallery images are persisted to disk but the gallery grid doesn't auto-refresh on app start (need to generate or reload)
- Image upload preview uses blob URLs which don't survive page reload
- Settings page "Restart ComfyUI" button may not work if ComfyUI wasn't launched by the app (remote mode)

---

## Project Structure (Key Files)

```
comfyui-desktop/
├── src/                              # Svelte 5 frontend
│   ├── App.svelte                    # Main shell, gallery, lightbox, page routing
│   ├── app.css                       # Global styles, Tailwind v4
│   └── lib/
│       ├── components/
│       │   ├── generation/
│       │   │   ├── GenerationPage.svelte    # Main generation UI (two-column layout)
│       │   │   ├── ModelSelector.svelte     # Checkpoint, VAE, LoRA selection
│       │   │   ├── SamplerSettings.svelte   # Sampler, scheduler, steps, CFG, seed
│       │   │   ├── DimensionControls.svelte # Aspect ratio, resolution
│       │   │   └── UpscaleSettings.svelte   # Upscale method, tiling, denoise
│       │   ├── progress/
│       │   │   └── ProgressBar.svelte       # Phase-aware progress display
│       │   ├── settings/
│       │   │   └── SettingsPage.svelte      # App configuration page
│       │   └── ui/
│       │       └── InfoTip.svelte           # Tooltip component
│       ├── stores/                   # Svelte 5 rune-based state
│       │   ├── generation.svelte.ts  # Generation params ($state/$derived)
│       │   ├── models.svelte.ts      # Available models/samplers
│       │   ├── progress.svelte.ts    # Generation progress tracking
│       │   ├── gallery.svelte.ts     # Gallery image management
│       │   └── connection.svelte.ts  # ComfyUI connection state
│       ├── types/index.ts            # TypeScript interfaces
│       └── utils/api.ts              # Tauri invoke wrappers
│
├── src-tauri/                        # Rust/Tauri backend
│   ├── tauri.conf.json               # App config (name, version, bundle, CSP)
│   ├── Cargo.toml                    # Rust dependencies
│   ├── icons/
│   │   ├── icon.png                  # App icon (source)
│   │   └── icon.ico                  # Windows icon (generated from png)
│   └── src/
│       ├── lib.rs                    # Tauri plugin/command registration
│       ├── config.rs                 # AppConfig struct, load/save JSON
│       ├── setup.rs                  # One-click installer (uv, Python, ComfyUI, PyTorch, GPU detection)
│       ├── state.rs                  # AppState (shared state with RwLock)
│       ├── error.rs                  # AppError enum
│       ├── commands/
│       │   ├── mod.rs
│       │   ├── api.rs               # Model listing, image upload/download, clipboard, gallery
│       │   ├── config.rs            # get_config / update_config
│       │   ├── server.rs            # Start/stop ComfyUI process
│       │   ├── websocket.rs         # WebSocket connection management
│       │   └── workflow.rs          # Generate (build workflow + submit to ComfyUI)
│       ├── comfyui/
│       │   ├── client.rs            # HTTP client (reqwest) for ComfyUI REST API
│       │   ├── websocket.rs         # WebSocket connection, event parsing, preview frames
│       │   ├── process.rs           # Spawn/kill ComfyUI Python process
│       │   └── types.rs             # API response types (SamplerInfo, QueueInfo, etc.)
│       └── templates/
│           ├── mod.rs               # Shared workflow builder utilities
│           ├── txt2img.rs           # Text-to-image workflow JSON
│           ├── img2img.rs           # Image-to-image workflow JSON
│           ├── inpainting.rs        # Inpainting workflow JSON
│           └── upscale.rs           # Upscale chain (model/lanczos + tiled KSampler)
│
├── comfyui-nodes/
│   └── nodes_tiled_diffusion.py     # Custom ComfyUI node (MultiDiffusion + SpotDiffusion)
│
├── .github/workflows/
│   └── release.yml                  # CI: build all platforms on tag push
│
└── README.md                        # Project documentation
```

---

## Build Commands

```bash
cd comfyui-desktop/

# Development (hot-reload)
npm run tauri dev

# Frontend only (check for TS/Svelte errors)
npm run build

# Rust check (no full compile)
cd src-tauri && cargo check

# Production build
npm run tauri build

# Generate Windows icon from PNG (requires ImageMagick)
cd src-tauri/icons
convert icon.png -define icon:auto-resize=256,128,64,48,32,16 icon.ico
```

---

## CI Release Process

1. Bump version in `tauri.conf.json`, `package.json`, `Cargo.toml`
2. Commit and tag: `git tag v0.X.0 && git push origin v0.X.0`
3. GitHub Actions builds all 4 targets and creates a draft release
4. Review draft on GitHub, edit release notes if needed, publish

To retrigger a build on the same tag:
```bash
git tag -d v0.X.0
git push origin :refs/tags/v0.X.0
git tag v0.X.0
git push origin v0.X.0
```
