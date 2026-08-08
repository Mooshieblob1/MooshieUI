---
trigger: always_on
description: MooshieUI core build, dual-mode IPC, release/git gotchas — always apply
---

# MooshieUI Core

Mirrors essentials from root `AGENTS.md`. Deeper mode rules: sibling `.md` files in `.agents/rules/`.

## Build & run

```bash
npm install && npm run tauri dev    # dev (port 1420)
npm run tauri build                 # production
cargo check   # in src-tauri/
```

No frontend tests (no vitest/jest). Rust has ~128 `#[test]` fns over pure logic: `cargo test --manifest-path src-tauri/Cargo.toml`. The suite is green; any failure is a real regression.

## Dual-mode (non-negotiable)

- Desktop: Tauri WebView. Browser: axum serves same UI (`webserver.rs`); `window.__MOOSHIE_BROWSER_MODE__`.
- **All backend I/O:** `ipcInvoke()` / `ipcListen()` in `src/lib/utils/ipc.ts` — never raw `invoke()`/`listen()` (browser mode breaks silently).

## Images

- Gallery on disk: **JXL**. UI: `loadGalleryImageDisplay()` / `loadGalleryImagePng()` — not raw file reads.
- URI schemes: `thumbnail://`, `gallery://`.

## No co-authoring

Never add `Co-Authored-By` trailers to any commit, PR body, or comment. Do not attribute AI assistance anywhere in git or GitHub output.

## Git / release (Windows)

- Pre-commit hook is bash → hangs in PowerShell: `git -c core.hooksPath=/dev/null ...`
- Version must match in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`
- Workflows: `.agents/skills/push`, `.agents/skills/release`

## Diagnostics

- `error-logs/` for large logs (on-demand read)
- Ring buffers: `log_buffer.rs`, `log-buffer.ts` → `exportLogs()`
