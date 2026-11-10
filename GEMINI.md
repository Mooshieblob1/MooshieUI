### Project Overview

MooshieUI is a user-friendly interface for image, video, music and experimental image-to-3D generation through ComfyUI, with an optional NovelAI image backend. It is built with Svelte 5 for the frontend and Rust for the backend, and runs two ways from one codebase: as a Tauri v2 desktop app, and as a browser app served by an embedded axum web server (LAN and Docker friendly).

The goal of MooshieUI is to abstract away the complexity of ComfyUI's graph-based workflow. The Rust backend builds workflow JSON from templates, so users never see a node graph.

**This file is a short overview.** The maintained agent entry point is [AGENTS.md](AGENTS.md), with full conventions in [.github/instructions/mooshieui.instructions.md](.github/instructions/mooshieui.instructions.md) and the canonical skills and rules in [.agents/](.agents/README.md). Where this file and those differ, they win.

**Key Technologies:**
- **Frontend:** Svelte 5, TypeScript 6, Tailwind CSS 4, Vite 6
- **Backend & Desktop:** Tauri v2 (Rust), plus an axum web server for browser mode
- **State Management:** Svelte 5 runes in class-based singleton stores (`*.svelte.ts`), not `svelte/store`
- **API Communication:** The frontend calls the backend only through `ipcInvoke()` / `ipcListen()` in `src/lib/utils/ipc.ts` (Tauri IPC on desktop, REST POST + SSE in browser mode), with typed wrappers in `src/lib/utils/api.ts`. Never call raw `invoke()` / `listen()` from `@tauri-apps/api`: it works on desktop and breaks silently in browser mode. The Rust backend then sends workflow requests to ComfyUI through its REST and WebSocket APIs, or image requests to the NovelAI API.

**Architecture:**
- `src/`: The Svelte 5 frontend.
    - `lib/components/`: UI components, organized by feature.
    - `lib/stores/`: Rune-based stores for global state.
    - `lib/utils/ipc.ts`, `lib/utils/api.ts`: The IPC abstraction and its typed wrappers.
    - `lib/locales/`: 12 locale files. Every key in `en.ts` must exist in all of them.
- `src-tauri/`: The Rust backend.
    - `src/commands/`: Command handlers exposed to the frontend. They return `Result<T, AppError>`.
    - `src/comfyui/`: The ComfyUI client, process management and WebSocket bridge.
    - `src/templates/`: Builders for ComfyUI workflow JSON.
    - `src/webserver.rs`: The embedded web server for browser mode.
    - `src/novelai/`, `src/mcp/`, `src/cloud/`: The NovelAI backend, the opt-in MCP server and the cloud voice and video providers.
- `comfyui-nodes/`: Custom Python nodes installed into ComfyUI at setup.

### Building and Running

**Prerequisites:**
- Node.js 22.12+ or 24+
- Rust (latest stable)
- Tauri v2 prerequisites (see [Tauri docs](https://v2.tauri.app/start/prerequisites/))

**Development:**
1. Install frontend dependencies: `npm install`
2. Run the development environment: `npm run tauri dev`

The application has a one-time setup wizard that downloads and configures Python, ComfyUI and the required dependencies on first launch.

**Validation:**
- `npm run check` runs the frontend gates: build, i18n parity, portal check and type check.
- `cargo check --manifest-path src-tauri/Cargo.toml` and the same with `--no-default-features --features server`. The crate has two builds (desktop and server), and Tauri references outside a `#[cfg(feature = "desktop")]` gate break the server build.
- `cargo test --manifest-path src-tauri/Cargo.toml` runs the Rust unit tests. The suite is green; treat a failure as a regression.

**Production Build:** `npm run tauri build`

### Development Conventions

- **State:** Stores are class singletons using `$state`. Call `saveSettings()` explicitly after mutations.
- **i18n:** All user-facing strings go through `locale.t('key')`. No hardcoded display text.
- **Styling:** Tailwind CSS only, no `<style>` blocks in `.svelte` files. Use `onclick`, not `on:click`.
- **Overlays:** Every `fixed` modal or popup uses `use:portal` on its outermost element.
- **Gallery images:** Stored as PNG, JPEG XL or WebP. Load them through `loadGalleryImageDisplay()` / `loadGalleryImagePng()`, never by reading files directly.
- **Git:** `main` is protected. Land changes through a PR from a `chore/<slug>` branch. See [docs/push-instructions.md](docs/push-instructions.md).
