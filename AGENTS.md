# AGENTS.md

This file provides guidance to agents when working with code in this repository.

## Error Logs

- **`error-logs/` directory**: Drop large error logs, stack traces, or debug output here. Files in this directory are excluded from automatic context ingestion (via `.rooignore`), so they won't consume live context tokens. Reference the filename when you need me to read a log on demand. This directory is git-ignored.

## Build & Run

```bash
npm install                  # Frontend dependencies
npm run tauri dev            # Full dev (Tauri + Vite hot-reload on port 1420)
npm run tauri build          # Production binary
cargo check                  # Rust compile check (run in src-tauri/)
cargo fmt                    # Rust format (run in src-tauri/)
cargo clippy                 # Rust lint (run in src-tauri/)
```

**No test framework exists.** No vitest/jest on frontend, no `#[test]` modules in Rust.

## Critical Architecture (Non-Obvious)

- **Dual-mode app**: Runs as Tauri desktop app AND as a browser-mode web app via embedded axum server (`src-tauri/src/webserver.rs`). The flag `window.__MOOSHIE_BROWSER_MODE__` determines which mode is active.
- **Custom IPC abstraction** (`src/lib/utils/ipc.ts`): ALL backend calls go through `ipcInvoke()`/`ipcListen()` — NEVER use `invoke()` or `listen()` directly. These route to Tauri IPC OR HTTP/SSE depending on the mode.
- **JXL storage**: Gallery images are stored as JPEG XL format. Display reads use `loadGalleryImageDisplay()` (transcodes JXL→WebP), PNG export uses `loadGalleryImagePng()` (JXL→PNG). Never read gallery files directly.
- **Custom URI schemes**: Tauri registers `thumbnail://` and `gallery://` protocols for loading images from the gallery directory.

## Release Process Gotchas

- **Version in 3 files must match exactly**: [`package.json`](package.json:5), [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml:3), [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json:4)
- **Pre-commit hook is bash**: hangs in PowerShell. Always use `git -c core.hooksPath=/dev/null` for all git commands on Windows.
- **Tag protection**: tags cannot be deleted or force-updated. Use `workflow_dispatch` as fallback.
- Full release procedure at [`.github/prompts/release.prompt.md`](.github/prompts/release.prompt.md)

## Other Non-Obvious Items

- **CSP is null** in [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json:25) — no Content Security Policy restrictions.
- **Ring buffer log capture**: Both Rust (`src-tauri/src/log_buffer.rs`) and frontend (`src/lib/utils/log-buffer.ts`) capture console output for `exportLogs()` diagnostics.
- **keep_alive config**: When true, ComfyUI process survives app close. App kills ComfyUI on exit otherwise.
- **Store files use `.svelte.ts` extension** — required for Svelte 5 rune compilation.
- **Cursor config**: [`.cursor/README.md`](.cursor/README.md)
  - **Skills:** `push`, `release`, `pre-commit-check`, `add-tauri-command`, `add-generation-param`, `workflow-template-builder` — [`.cursor/skills/`](.cursor/skills/)
  - **Rules:** always-on + file-scoped (mirrors `.roo/rules-*`) — [`.cursor/rules/`](.cursor/rules/)
- **Existing AI rules**: [`GEMINI.md`](GEMINI.md), [`.github/copilot-instructions.md`](.github/copilot-instructions.md), [`.github/instructions/`](.github/instructions/), [`.github/agents/`](.github/agents/), [`.roo/commands/`](.roo/commands/)
