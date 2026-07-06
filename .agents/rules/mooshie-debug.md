---
trigger: manual
description: MooshieUI debugging — log buffers, browser /internal-api, silent failures. Use when fixing bugs or investigating errors.
---

# Debug (MooshieUI)

## Logs

- Rust `log_buffer.rs` (2000) + frontend `log-buffer.ts` (1000). Use **`exportLogs()`** — console alone misses Rust output.
- Frontend intercepts all `console.*` + unhandled errors/rejections.

## Browser mode

- IPC: `POST /internal-api/{command}`; events: SSE `/internal-api/_events`.
- Heartbeat `/internal-api/_heartbeat` — 120s idle kills server (unless LAN mode).
- Confirm `window.__MOOSHIE_BROWSER_MODE__`.

## Rust / platform

- CSP is `null` in `tauri.conf.json`.
- Linux: `WEBKIT_DISABLE_DMABUF_RENDERER=1`; AppImage Wayland may re-exec with `LD_PRELOAD`.

## Silent failures

- Raw `invoke()` works desktop-only.
- Missing locale keys → English fallback, no error.
- Settings don't persist → `saveSettings()` not called.

## Gallery bugs

- On-disk JXL is not displayable directly — use transcoding helpers.
- `thumbnail://` / `gallery://` URLs differ by OS.
