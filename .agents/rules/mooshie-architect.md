---
trigger: manual
description: MooshieUI architecture — dual-mode, AppState, workflows, storage. Use when designing features or reviewing system design.
---

# Architect (MooshieUI)

## Dual-mode

- **Desktop:** Tauri + ComfyUI child process.
- **Browser:** axum HTTP; IPC = REST POST + SSE. Shared `Arc<AppState>` for Tauri commands and HTTP handlers.
- Route all frontend backend calls through `src/lib/utils/ipc.ts`.

## State & concurrency

- Config: `RwLock` — **drop guard before `.await`** on I/O.
- ComfyUI process: `Mutex<Option<Child>>` (one instance).
- Browser auth: `AuthState`; tokens in memory.

## Browser server lifecycle

- Bind `127.0.0.1:ui_server_port` (fallback port if busy).
- Heartbeat watchdog: 120s idle shutdown unless `lan_enabled`.
- Frontend heartbeats every 3s + visibility/beacon.
- `spawn_prompt_cleanup_reactor` + `spawn_stuck_worker_watchdog` always on.

## Images

- Gallery: JXL on disk; display via transcoding helpers; thumbnails WebP on demand (`thumbnail://` or `/internal-api/_thumbnail/`).
- `gallery://` for full-res non-JXL view. Temp dir cleaned at startup.

## Workflow templates

- `src-tauri/src/templates/`: JSON maps, string node IDs `"1"`,`"2"`, connections `[id, port]`.
- `finish_workflow` (mod.rs) appends post-process chains in fixed order: upscale → facefix → segment refinement → `MooshieSaveImage`. LoRA chain threads `model_source`/`clip_source`.
- Custom ComfyUI nodes live in `comfyui/mooshie_nodes.py`, deployed at startup, verified via `/object_info` (`REQUIRED_MOOSHIE_NODE_CLASSES`).
- Prompt inline tags (`<from/to/range>`, `<segment:...>`) are parsed frontend-side in `toParams()`; Rust receives structured params (`positive_segments`, `detail_segments`), never raw tags.

## Constraints

- Stores must not form import cycles. The hub `generation` store and leaf utility stores (e.g. `locale`) must not import feature stores; feature stores (`canvas`, `compare`, `gallery`, ...) may depend one-directionally on `generation`. A store reacting to another store's state (a side-effect push) belongs in an `App.svelte` `$effect`, not an imperative call between stores.
- `toParams()`: manual camelCase → snake_case (silent breakage if mismatched).
- i18n: new `en.ts` keys → **all** `src/lib/locales/*.ts`.
- `setup.rs`: `#[cfg(feature = "desktop")]`, `Result<(), String>`.

## Performance

- Shared `state.http_client` (reqwest pool).
- Gallery metadata: SQLite, not directory scan.
- JXL thumbnail transcode on first access only.
