# AGENTS.md

This file provides guidance to agents when working with code in this repository.

## Non-Negotiable Behavioral Rules

- **No co-authoring**: Never add `Co-Authored-By` trailers to any commit, PR body, issue comment, or PR review comment. Do not attribute AI assistance anywhere in git or GitHub output.
- **No em dashes when writing as the owner**: Issue comments, PR comments, and external posts in the owner's voice must never contain em dashes. Use commas, periods, or parentheses instead.
- **No `claude/` branches**: Never create or push a branch named `claude/...` (or after any AI tool), even when the session assigns one. Use `chore/<slug>` (the push skill's convention) or another descriptive prefix.
- **Commit as the owner**: Author and commit as `Mooshieblob <blob@mooshieblob.com>`. Set the repo-local `git config user.name` and `user.email` before the first commit; an AI author on a branch commit becomes a `Co-authored-by` line when GitHub squash-merges the PR.

## Error Logs

- **`error-logs/` directory**: Drop large error logs, stack traces, or debug output here. Files in this directory are excluded from automatic context ingestion, so they won't consume live context tokens. Reference the filename when you need me to read a log on demand. This directory is git-ignored.

## Build & Run

```bash
npm install                  # Frontend dependencies
npm run check                # Frontend gates: build, i18n parity, portal check, svelte-check
npm run tauri dev            # Full dev (Tauri + Vite hot-reload on port 1420)
npm run tauri build          # Production binary
cargo check                  # Rust compile check, desktop features (run in src-tauri/)
cargo check --no-default-features --features server   # server binary build (run in src-tauri/)
cargo fmt                    # Rust format (run in src-tauri/)
cargo clippy                 # Rust lint (run in src-tauri/)
```

**Two Rust builds, one crate.** `default = ["desktop"]` links `tauri`; the server binary (`--no-default-features --features server`, built by CI's `build-server` job) does not. A `tauri` reference outside a `#[cfg(feature = "desktop")]` gate compiles locally and breaks the release build. Modules gated whole in `commands/mod.rs` can use `tauri` freely; modules present in both builds (`api.rs`, `video_export.rs`, `video_interpolate.rs`, `webserver.rs`) need per-item gates, including on function parameters and the matching call-site arguments.

**No frontend test framework.** The frontend has no vitest/jest; a few pure-logic `node:test` files live in `scripts/tests/` and the Guardrails workflow bundles them with esbuild and runs them. Rust does have tests: about 1,300 tests (`#[test]` + `#[tokio::test]`) in `#[cfg(test)]` modules over pure logic. Run `cargo test --manifest-path src-tauri/Cargo.toml`. The suite is green; treat any failure as a real regression.

## Critical Architecture (Non-Obvious)

- **Dual-mode app**: Runs as Tauri desktop app AND as a browser-mode web app via embedded axum server (`src-tauri/src/webserver.rs`). The flag `window.__MOOSHIE_BROWSER_MODE__` determines which mode is active.
- **Custom IPC abstraction** (`src/lib/utils/ipc.ts`): ALL backend calls go through `ipcInvoke()`/`ipcListen()` — NEVER use `invoke()` or `listen()` directly. These route to Tauri IPC OR HTTP/SSE depending on the mode.
- **Gallery storage**: Gallery images are stored as PNG (the default), JPEG XL or WebP, per the output format setting. Display reads use `loadGalleryImageDisplay()` (transcodes to WebP), PNG export uses `loadGalleryImagePng()`. Never read gallery files directly.
- **Custom URI schemes**: Tauri registers `thumbnail://` and `gallery://` protocols for loading images from the gallery directory.

- **MCP server** (`src-tauri/src/mcp/`, [`docs/MCP.md`](docs/MCP.md)): opt-in, localhost-only, token-protected (with optional Cloudflare quick tunnel in `mcp/tunnel.rs` or owner `public_url`). It reuses the owner's last request per model (local txt2img, NovelAI txt2img, local Image Edit) and applies bounded agent overrides. When adding or changing a feature, state the MCP decision in the PR: clear new per-image `GenerationParams` inputs in `setups::snapshot`, add assistant-controllable fields to `setups::Overrides` / `apply()` and the `generate_image` schema in `mcp/tools.rs`, consider a tool for new modes or gallery actions, keep the `comfyui:*` events and `save_to_gallery_in_dir` (`mcp/jobs.rs`) intact, update `mcp/music.rs` (`MusicParams`), and mirror changes across prompt expansion/library (`mcp/expand.rs`, `mcp/library.rs`) and comic layouts (`mcp/comic.rs`). Keep the tool table in [`docs/MCP.md`](docs/MCP.md) current.
- **Model architectures**: new model families are declared once as an `ArchitectureSpec` in `src-tauri/src/architectures/<name>.rs`; the frontend reads `utils/architectures.ts` via `list_model_architectures`. Do not add new per-family match arms for a registered family (see [`docs/ARCHITECTURES.md`](docs/ARCHITECTURES.md)).
- **Modals and popups (`use:portal`)**: put `use:portal` (`src/lib/utils/portal.ts`) on the outermost element of every `fixed` overlay or popup and keep it the sole top-level node of its `{#if}` branch (`node scripts/check-portals.mjs` in Guardrails CI enforces this; deliberate exceptions need `<!-- portal-exempt: reason -->`).
- **External HTTP error bodies**: pass error bodies from external services through `src-tauri/src/http_error.rs` (`readable_body` / `detail_suffix`) and frontend `responseErrorText` (`src/lib/utils/ipc.ts`) so Cloudflare/proxy HTML never reaches the UI.
- **CivitAI links**: route all "Open on CivitAI" model page links through `civitaiModelPageUrl()` (`src/lib/utils/civitaiUrl.ts`) so mature pages use `civitai.red` (API stays `civitai.com`).

## Release Process Gotchas

- **Version in 3 files must match exactly**: [`package.json`](package.json:5), [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml:3), [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json:4)
- **Pre-commit hook is bash**: hangs in PowerShell. Always use `git -c core.hooksPath=/dev/null` for all git commands on Windows.
- **Tag protection**: tags cannot be deleted or force-updated. Use `workflow_dispatch` as fallback.
- Full release procedure at [`.github/prompts/release.prompt.md`](.github/prompts/release.prompt.md)

## Other Non-Obvious Items

- **CSP**: A Content Security Policy is set in `src-tauri/tauri.conf.json` and enforced in production builds only (there is no `devCsp`, so `npm run tauri dev` never exercises it). Any new remote `<img>`/`fetch` origin or custom URI scheme must be added to it.
- **Ring buffer log capture**: Both Rust (`src-tauri/src/log_buffer.rs`) and frontend (`src/lib/utils/log-buffer.ts`) capture console output for `exportLogs()` diagnostics.
- **keep_alive config**: When true, ComfyUI process survives app close. App kills ComfyUI on exit otherwise.
- **Store files use `.svelte.ts` extension** — required for Svelte 5 rune compilation.
- **Agent config (canonical):** [`.agents/README.md`](.agents/README.md)
  - **Skills:** `push`, `release`, `quickrelease`, `cleanup`, `pre-commit-check`, `add-tauri-command`, `add-generation-param`, `add-comfyui-node`, `workflow-template-builder` — [`.agents/skills/`](.agents/skills/)
  - **Rules:** glob-scoped + manual — [`.agents/rules/`](.agents/rules/)
  - **Claude Code mirror:** [`.claude/skills/`](.claude/skills/), [`.claude/commands/`](.claude/commands/) (synced from `.agents/`)
- **Existing AI rules**: [`GEMINI.md`](GEMINI.md), [`.github/copilot-instructions.md`](.github/copilot-instructions.md), [`.github/instructions/`](.github/instructions/) (including [`mooshieui.instructions.md`](.github/instructions/mooshieui.instructions.md)), [`.github/agents/`](.github/agents/)
- **Project docs**: [`docs/README.md`](docs/README.md) — bot triage, feature research, superpowers plans/specs
