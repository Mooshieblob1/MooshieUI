# Architect Mode Rules (Non-Obvious Only)

## Dual-Mode Architecture

This app runs in **two distinct modes** — any architectural change must work in both:

- **Desktop mode** (Tauri): Svelte UI loaded in a native WebView. IPC via Tauri's `invoke()`/`listen()`. ComfyUI as a child process.
- **Browser mode**: Same Svelte UI served via embedded axum HTTP server. IPC as REST POST + SSE. ComfyUI runs server-side.

The IPC abstraction layer ([`src/lib/utils/ipc.ts`](src/lib/utils/ipc.ts)) routes calls based on `isTauri`/`isBrowserMode` flags. `AppState` is shared between Tauri and axum via `Arc`.

## State Sharing & Concurrency

- `AppState` is `Arc<AppState>`, managed by Tauri and passed to axum handlers. Both Tauri commands and HTTP handlers share the same state.
- Config uses `RwLock` — hold `.read()` guards briefly. **Drop before `.await`** on any I/O to prevent deadlocks.
- ComfyUI process handle uses `Mutex<Option<Child>>` — only one instance per app.
- Browser mode auth uses `AuthState` with SHA256-hashed password + argon2. Tokens stored in an in-memory `HashSet`.

## Server Lifecycle (Browser Mode)

- The axum server binds to `127.0.0.1:ui_server_port` (configurable). If the port is in use, it falls back to an OS-assigned port.
- **Heartbeat watchdog**: In single-user browser mode, the server shuts down after 120s without a heartbeat. In LAN mode (`lan_enabled`), the watchdog is disabled because multiple users may connect.
- The frontend sends heartbeats every 3s via `setInterval` + on `visibilitychange` + `beforeunload` beacon.
- `spawn_prompt_cleanup_reactor` and `spawn_stuck_worker_watchdog` run unconditionally (both desktop and browser modes) to release workers after each prompt.

## Image Storage & Delivery

- **Gallery**: Images stored as JXL on disk. Read via `loadGalleryImageDisplay()` (JXL→WebP for display) or `loadGalleryImagePng()` (JXL→PNG for export). Never read raw `.jxl` files for UI display.
- **Thumbnails**: Generated on-the-fly as WebP via the `thumbnail://` custom URI scheme in Tauri, or `/internal-api/_thumbnail/` endpoint in browser mode. Not stored as separate files.
- **Gallery URI scheme** (`gallery://`): Serves original gallery images (PNG, JPG, WebP — NOT raw JXL). Used for full-resolution viewing.
- **Temp images**: Short-lived directory for intermediate generation outputs. Cleaned on app startup via `temp_images::init()`.

## Workflow Architecture

- Workflow templates in [`src-tauri/src/templates/`](src-tauri/src/templates/) build `serde_json::Map<String, Value>` (not visual/graph-based).
- Node IDs are **stringified incrementing integers** (`"1"`, `"2"`, `"3"`) assigned via `next_id.to_string()` counter.
- Node connections use `(node_id_string, output_port_index)` tuples as JSON arrays.
- The `SaveImage` terminal node is appended by `templates/mod.rs`, not individual templates.
- LoRA chaining pattern: `model_source`/`clip_source` tuples are threaded through sequential `LoraLoader` nodes.

## Hidden Dependencies & Constraints

- **Svelte stores must NOT import each other** directly. Cross-store coordination is centralized in `App.svelte`.
- **`toParams()` pattern**: TypeScript camelCase fields must be manually mapped to Rust snake_case. This is a brittle coupling — changing a field name in one place without updating the mapping breaks the API silently.
- **i18n key parity**: Missing locale keys silently fallback to English. If you add a key to `en.ts`, you MUST add it to every other locale file in [`src/lib/locales/`](src/lib/locales/).
- **`setup.rs` is self-contained**: The one-click installer has its own error handling (`Result<(), String>`, not `AppError`) and is gated behind `#[cfg(feature = "desktop")]`.

## Performance Considerations

- Use `state.http_client` (shared `reqwest::Client`) for all HTTP calls — connection pooling. Never create a new client per request.
- The gallery index uses SQLite (`rusqlite` with `bundled` feature) for metadata queries, not filesystem scanning.
- JXL transcoding (for thumbnails) is CPU-intensive — thumbnails are only generated on first access, not at import time.
