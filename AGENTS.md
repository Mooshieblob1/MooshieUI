# AGENTS.md

This file provides guidance to agents when working with code in this repository.

## Non-Negotiable Behavioral Rules

- **No co-authoring**: Never add `Co-Authored-By` trailers to any commit, PR body, issue comment, or PR review comment. Do not attribute AI assistance anywhere in git or GitHub output.

## Error Logs

- **`error-logs/` directory**: Drop large error logs, stack traces, or debug output here. Files in this directory are excluded from automatic context ingestion, so they won't consume live context tokens. Reference the filename when you need me to read a log on demand. This directory is git-ignored.

## Build & Run

```bash
npm install                  # Frontend dependencies
npm run tauri dev            # Full dev (Tauri + Vite hot-reload on port 1420)
npm run tauri build          # Production binary
cargo check                  # Rust compile check, desktop features (run in src-tauri/)
cargo check --no-default-features --features server   # server binary build (run in src-tauri/)
cargo fmt                    # Rust format (run in src-tauri/)
cargo clippy                 # Rust lint (run in src-tauri/)
```

**Two Rust builds, one crate.** `default = ["desktop"]` links `tauri`; the server binary (`--no-default-features --features server`, built by CI's `build-server` job) does not. A `tauri` reference outside a `#[cfg(feature = "desktop")]` gate compiles locally and breaks the release build. Modules gated whole in `commands/mod.rs` can use `tauri` freely; modules present in both builds (`api.rs`, `video_export.rs`, `video_interpolate.rs`, `webserver.rs`) need per-item gates, including on function parameters and the matching call-site arguments.

**No frontend test framework.** No vitest/jest. Rust does have tests: ~128 `#[test]` fns in `#[cfg(test)]` modules over pure logic. Run `cargo test --manifest-path src-tauri/Cargo.toml`. The suite is green; treat any failure as a real regression.

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
- **Agent config (canonical):** [`.agents/README.md`](.agents/README.md)
  - **Skills:** `push`, `release`, `cleanup`, `pre-commit-check`, `add-tauri-command`, `add-generation-param`, `add-comfyui-node`, `workflow-template-builder` — [`.agents/skills/`](.agents/skills/)
  - **Rules:** always-on + file-scoped — [`.agents/rules/`](.agents/rules/)
  - **Claude Code mirror:** [`.claude/skills/`](.claude/skills/), [`.claude/commands/`](.claude/commands/) (synced from `.agents/`)
- **Existing AI rules**: [`GEMINI.md`](GEMINI.md), [`.github/copilot-instructions.md`](.github/copilot-instructions.md), [`.github/instructions/`](.github/instructions/) (including [`mooshieui.instructions.md`](.github/instructions/mooshieui.instructions.md)), [`.github/agents/`](.github/agents/)
- **Project docs**: [`docs/README.md`](docs/README.md) — bot triage, feature research, superpowers plans/specs
