# Debug Mode Rules (Non-Obvious Only)

## Log Capture

- **Logs are NOT just in console**. Both Rust (`src-tauri/src/log_buffer.rs`) and frontend (`src/lib/utils/log-buffer.ts`) maintain bounded ring buffers (2000 lines Rust, 1000 lines frontend). Use `exportLogs()` to get the full combined picture — `console.log` alone misses Rust-side output.
- The frontend ring buffer intercepts `console.{log,info,warn,error,debug}` + uncaught errors + unhandled promise rejections. It's always on.

## Browser Mode Debugging

- In browser mode, IPC calls go to `/internal-api/{command}` as HTTP POST. SSE events stream from `/internal-api/_events`. Check these endpoints when `ipcInvoke()` fails silently.
- The heartbeat endpoint at `/internal-api/_heartbeat` keeps the server alive. If the server dies after 120s of inactivity, the heartbeat watchdog killed it.
- Check `window.__MOOSHIE_BROWSER_MODE__` to confirm you're in browser vs Vite dev mode.

## Rust/Tauri Debugging

- **CSP is null** — you won't see CSP violation errors in devtools, but this means all content origins are trusted.
- `WEBKIT_DISABLE_DMABUF_RENDERER=1` is auto-set on Linux to fix NVIDIA+Wayland rendering glitches.
- On Linux AppImage + Wayland: the app may re-exec itself with `LD_PRELOAD` for `libwayland-client.so` (sentinel env var `_MOOSHIEUI_WAYLAND_FIXED` prevents loops).
- `cargo check` is fast compile validation; `cargo clippy` catches common Rust pitfalls.

## Silent Failures

- Direct `invoke()` calls from `@tauri-apps/api` work in Tauri but **fail silently in browser mode**. Always use `ipcInvoke()`.
- If a locale key is missing from a non-English locale file, `locale.t()` silently falls back to English — no error thrown.
- `saveSettings()` is never called automatically. If settings seem to "not persist", check that `saveSettings()` is explicitly invoked after the mutation.

## Gallery/Image Issues

- Gallery images are stored as JXL format, not PNG. Direct file reads will get JXL data, not displayable images. Use `loadGalleryImageDisplay()` (→WebP) or `loadGalleryImagePng()` (→PNG).
- Custom Tauri URI schemes `thumbnail://` and `gallery://` have platform-specific URL formats (macOS/Linux vs Windows use different prefixes).
