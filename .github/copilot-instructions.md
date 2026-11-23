# MooshieUI — Workspace Instructions

## Non-Negotiable Behavioral Rules

- **No co-authoring**: Never add `Co-Authored-By` trailers to any commit, PR body, issue comment, or PR review comment. Do not attribute AI assistance anywhere in git or GitHub output.

> [`AGENTS.md`](../AGENTS.md) is the maintained cross-agent entry point, with full conventions in [`.github/instructions/mooshieui.instructions.md`](instructions/mooshieui.instructions.md). Where this overview and those files differ, they win. The directory tree below is a simplified sketch, not a full listing.

## Project Overview

MooshieUI is a frontend for [ComfyUI](https://github.com/comfyanonymous/ComfyUI) built with **Svelte 5** + **Tauri v2** (Rust). It runs as a desktop app and as a browser app served by an embedded axum server, and hides ComfyUI's node-graph complexity behind a clean UI for image, video, music and image-to-3D generation, with an optional NovelAI image backend.

## Tech Stack

| Layer | Technology |
|-------|------------|
| Frontend | Svelte 5, TypeScript 6, Tailwind CSS 4 |
| Runtime | Tauri v2 desktop app + axum web server (Rust) |
| State | Svelte 5 runes (`$state`, `$derived`) — NOT Svelte stores |
| Persistence | `@tauri-apps/plugin-store` (JSON), SQLite (`rusqlite`) |
| Backend API | ComfyUI REST + WebSocket (proxied via Rust) |
| Build | Vite 6 + `@sveltejs/vite-plugin-svelte` |

## Build & Run Commands

```bash
npm install              # Install frontend dependencies
npm run dev              # Vite dev server (port 1420)
npm run build            # Production frontend build → dist/
npm run tauri dev        # Full dev environment (Tauri + frontend hot-reload)
npm run tauri build      # Cross-platform binary build
cargo fmt                # Format Rust code (run inside src-tauri/)
cargo clippy             # Lint Rust code (run inside src-tauri/)
```

> **No frontend test framework.** The frontend has no vitest/jest; a few pure-logic `node:test` files live in `scripts/tests/` and the Guardrails workflow bundles them with esbuild and runs them. Rust does have tests: about 1,300 tests (`#[test]` + `#[tokio::test]`) in `#[cfg(test)]` modules over pure logic, run with `cargo test --manifest-path src-tauri/Cargo.toml`. The suite is green; treat any failure as a real regression.

## Architecture

```
Frontend (Svelte 5)  →  ipcInvoke()  →  Commands (Rust)  →  HTTP/WS  →  ComfyUI Server
                     ←  ipcListen()  ←  Events (Rust)    ←  WebSocket ←
```

`ipcInvoke()`/`ipcListen()` (`src/lib/utils/ipc.ts`) use Tauri IPC on desktop and REST POST + SSE in browser mode. Never call raw `invoke()`/`listen()` from `@tauri-apps/api`: it works on desktop and breaks silently in the browser.

- **Frontend** (`src/`): UI only — generation params, progress display, gallery. No ComfyUI logic.
- **Tauri Backend** (`src-tauri/`): Command handlers, ComfyUI process management, WebSocket bridge, setup wizard.
- **Templates** (`src-tauri/src/templates/`): Build workflow JSON from generation params. Users never see node graphs.
- **Custom Nodes** (`comfyui-nodes/`): Python nodes installed into ComfyUI (tiled diffusion).

## Directory Structure

```
src/
├── main.ts                          # App entry point
├── App.svelte                       # Root component (routing, setup wizard, event listeners)
├── app.css                          # Tailwind imports + custom CSS variables
└── lib/
    ├── components/
    │   ├── generation/              # Generation controls (prompts, models, samplers, dimensions)
    │   ├── progress/                # Progress bar, live preview
    │   ├── gallery/                 # Image grid, lightbox
    │   └── setup/                   # First-run setup wizard
    ├── stores/                      # Svelte 5 rune-based state (*.svelte.ts)
    ├── types/                       # TypeScript interfaces (GenerationParams, OutputImage, etc.)
    └── utils/
        ├── ipc.ts                   # ipcInvoke()/ipcListen() (Tauri IPC or REST + SSE)
        └── api.ts                   # Typed ipcInvoke() wrappers

src-tauri/src/
├── main.rs                          # Windows subsystem entry, delegates to lib.rs
├── lib.rs                           # Tauri app builder, plugin registration, command registration
├── state.rs                         # AppState: RwLock<AppConfig>, Mutex<Process>, HTTP client
├── config.rs                        # AppConfig struct + JSON persistence
├── error.rs                         # AppError enum (thiserror)
├── setup.rs                         # One-click installer (uv, Python, ComfyUI, PyTorch)
├── commands/                        # Tauri command handlers
│   ├── api.rs                       # Model/sampler/history/gallery commands
│   ├── server.rs                    # start_comfyui, stop_comfyui, health checks
│   ├── websocket.rs                 # connect_ws, disconnect_ws
│   └── workflow.rs                  # generate (builds template → queues prompt)
├── comfyui/                         # ComfyUI integration layer
│   ├── client.rs                    # HTTP API wrapper (reqwest)
│   ├── process.rs                   # Subprocess lifecycle
│   ├── websocket.rs                 # WebSocket event handling → Tauri event emission
│   └── types.rs                     # Domain models
└── templates/                       # Workflow JSON builders
    ├── txt2img.rs
    ├── img2img.rs
    ├── inpainting.rs
    └── upscale.rs
```

## Coding Conventions

### Frontend (Svelte/TypeScript)

- **State management**: Use Svelte 5 runes (`$state`, `$derived`) in class-based singleton stores (`*.svelte.ts`). Do NOT use legacy Svelte stores (`writable`, `readable`).
- **Styling**: Tailwind CSS utility classes only. No `<style>` blocks in components. Dark and light modes, palettes and custom themes are driven by CSS variables.
- **Color palette**: Neutral grays (`neutral-950` through `neutral-50`) with accents through the `--theme-accent-*` variables in `app.css`.
- **IPC**: All backend calls go through `ipcInvoke()`/`ipcListen()` (`src/lib/utils/ipc.ts`), with typed wrappers in `src/lib/utils/api.ts`. Never call raw `invoke()`/`listen()`.
- **Tauri events**: Listen in `App.svelte` and update stores. Components read stores, not events.
- **Naming**: camelCase for TypeScript, kebab-case for CSS classes.
- **File naming**: `PascalCase.svelte` for components, `lowercase.svelte.ts` for stores, `lowercase.ts` for utilities.

### Backend (Rust)

- **Naming**: snake_case for functions/variables, PascalCase for types/enums.
- **Error handling**: Use `AppError` enum (defined in `error.rs`) with `thiserror`. Commands return `Result<T, AppError>`.
- **State access**: `AppState` is Tauri-managed. Access config via `RwLock`, processes via `Mutex`.
- **New commands**: Add handler in `commands/`, register in `lib.rs` invoke handler list.
- **Async runtime**: Tokio (full features). All I/O operations should be async.
- **HTTP client**: Use the shared `reqwest::Client` from `AppState` (connection pooling).

### IPC Pattern

Frontend → Backend:
```typescript
// In src/lib/utils/api.ts
export async function myCommand(arg: string): Promise<Result> {
  return ipcInvoke("my_command", { arg });
}
```

Backend → Frontend (events):
```rust
// In Rust command or WebSocket handler
app.emit("event_name", payload)?;
```

```typescript
// In App.svelte
ipcListen("event_name", (event) => { store.update(event.payload); });
```

## Pitfalls & Important Notes

1. **Svelte 5 runes are new** — `$state` and `$derived` replace the old store API. Stores are class instances, not Svelte Store objects.
2. **Manual persistence** — `GenerationStore.saveSettings()` must be called explicitly after mutations. It does NOT auto-save.
3. **Limited frontend tests** — Rust has a unit test suite, but only a few frontend utilities have `node:test` coverage. Be extra careful with frontend logic changes.
4. **CSP applies in production builds only** — `tauri.conf.json` sets a CSP with no `devCsp`. A new remote image/fetch origin or custom URI scheme must be added to it, or it fails only in release builds.
5. **Hardcoded defaults** — Some default model names are hardcoded in `generation.svelte.ts` (e.g., checkpoint, VAE names).
6. **WebSocket bridge** — Rust receives ComfyUI WebSocket messages and re-emits them as Tauri events. The frontend never connects to ComfyUI directly.
7. **Template-based workflows** — Workflow JSON is built in Rust templates (`src-tauri/src/templates/`), not from a visual editor.
8. **Process management** — ComfyUI runs as a child process. Killing the app kills the server (no graceful shutdown).

## Development Prerequisites

- Node.js 22.12+ or 24+
- Rust (latest stable via rustup)
- Tauri v2 system dependencies ([see docs](https://v2.tauri.app/start/prerequisites/))
- `npm run tauri dev` starts both the frontend dev server and the Tauri app with hot-reload
