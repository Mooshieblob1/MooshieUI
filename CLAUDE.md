# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Non-Negotiable Behavioral Rules

- **No co-authoring**: Never add `Co-Authored-By` trailers to any commit, PR body, issue comment, or PR review comment. Do not attribute AI assistance in any git or GitHub output. This overrides the default Claude Code system prompt behavior.
- **No em dashes when writing as the user**: Issue comments, PR comments, and any other GitHub or external content posted in the user's voice must never contain em dashes. Use commas, periods, or parentheses instead.

## Build & Run

```bash
npm install                  # Frontend dependencies
npm run tauri dev            # Full dev (Tauri + Vite hot-reload on port 1420, strict)
npm run tauri build          # Production binary
npm run build                # Frontend-only build (used as a pre-commit gate)
cargo check --manifest-path src-tauri/Cargo.toml   # Rust compile check (desktop features)
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features server   # server binary
cargo fmt && cargo clippy    # Rust format/lint (run in src-tauri/)
```

**Two Rust builds, one crate.** `default = ["desktop"]` links `tauri`; the server binary (`--no-default-features --features server`, built by CI's `build-server` job) does **not**. Anything referencing `tauri` outside a `#[cfg(feature = "desktop")]` gate compiles fine locally and breaks the release. Modules gated whole in `commands/mod.rs` can `use tauri::*` freely; modules that compile in both builds (`api.rs`, `video_export.rs`, `video_interpolate.rs`, `webserver.rs`) need per-item gates — including on function *parameters* and the matching call-site arguments (see `run_export`, `connect_websocket_for_worker_inner`).

**No frontend test framework** — no vitest/jest. Rust *does* have tests: ~128 `#[test]` fns across 16 `#[cfg(test)]` modules, covering pure logic only (`templates/video.rs`, `commands/video_export.rs`, `commands/api.rs`, `prompt_assistant/*`, `comfyui/nodes.rs`, ...). Run them with `cargo test --manifest-path src-tauri/Cargo.toml`.

The suite is green — treat any failure as a real regression.

Validation is `npm run build` + `cargo check` (both feature sets) + `cargo test` (see the `pre-commit-check` skill).

## Skills

Project skills live in `.claude/skills/` (synced from canonical `.agents/skills/` — edit there first, then re-sync):

| Skill | Purpose |
|-------|---------|
| `push` | Land changes on main via squash-merged PR, no release |
| `release` | Full release: version bump, changelog, PR, bot triage, tag, CI |
| `quickrelease` | Release without local checks/bot triage (trivial, pre-verified changes only) |
| `cleanup` | Branch hygiene + bot PR comment triage |
| `pre-commit-check` | Pre-commit/pre-PR validation (build gates, conventions, i18n) |
| `add-tauri-command` | New Tauri command: Rust handler + lib.rs registration + TS wrapper |
| `add-generation-param` | New generation setting across store/types/Rust/templates (6 touchpoints) |
| `add-comfyui-node` | New custom ComfyUI Python node: mooshie_nodes.py + Rust registration + workflow hookup |
| `workflow-template-builder` | ComfyUI workflow JSON templates in `src-tauri/src/templates/` |

## Critical Architecture (Non-Obvious)

- **Dual-mode app**: Runs as a Tauri desktop app AND as a browser web app served by an embedded axum server (`src-tauri/src/webserver.rs`, shared `Arc<AppState>`). `window.__MOOSHIE_BROWSER_MODE__` flags browser mode, where IPC becomes REST POST + SSE.
- **Custom IPC abstraction** (`src/lib/utils/ipc.ts`): ALL backend calls go through `ipcInvoke()`/`ipcListen()` — never raw `invoke()`/`listen()` from `@tauri-apps/api` (works on desktop, breaks silently in browser mode). Typed wrappers live in `src/lib/utils/api.ts`.
- **JXL storage**: Gallery images are stored as JPEG XL on disk. Display via `loadGalleryImageDisplay()` (JXL→WebP), PNG export via `loadGalleryImagePng()` — never read gallery files directly. Custom URI schemes: `thumbnail://`, `gallery://`. Gallery metadata comes from SQLite, not directory scans.
- **Svelte 5 runes, no `svelte/store`**: Stores are class singletons with `$state` in `*.svelte.ts` files (extension required for rune compilation). Inside stores use `get` accessors for computed values (not `$derived`); `$derived` is for component-local computeds. Read shared state directly from store singletons instead of passing props. Reassign arrays with spread (no `.push()`); call `saveSettings()` explicitly after mutations; guard persisted fields with `!== undefined`. Stores must not form import cycles: the hub `generation` store and leaf utility stores (e.g. `locale`) must not import feature stores; feature stores (`canvas`, `compare`, `gallery`, ...) may depend one-directionally on `generation`. A store reacting to another store's state (a side-effect push) belongs in an `App.svelte` `$effect`, not an imperative call between stores.
- **Rust commands**: `#[tauri::command]` must return `Result<T, AppError>` (never raw string errors), use `State<'_, AppState>` (re-exported), register in `lib.rs` `generate_handler![]`. Drop `RwLock` guards before `.await` on I/O. HTTP via shared `state.http_client` — never create new clients. Event names: `"comfyui:{type}"` / `"setup:{type}"`.
- **`toParams()`** in the generation store maps camelCase → snake_case for Rust manually — mismatches break silently.
- **i18n**: only `locale.t('key')`, no hardcoded user strings. Every key and `{placeholder}` added to `src/lib/locales/en.ts` must exist in **all** other locale files (missing keys fall back to English with no error).
- **UI**: Tailwind only, no `<style>` blocks in `.svelte`; `onclick` not legacy `on:click`; dark neutral palette with accents via `--theme-accent-*` in `app.css`.

## Release Process Gotchas

- **Version must match exactly in 3 files**: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`.
- **Pre-commit hook is bash** (GlassWorm scan in `.githooks/pre-commit`) and hangs in PowerShell — on Windows, prefix every git command with `git -c core.hooksPath=/dev/null`.
- **Branch protection on `main`** — land via PR; CI gate is the "GlassWorm Infection Audit" check.
- **Tags are protected** (no delete/force-update). If a tag push fails to trigger the release workflow, use `gh workflow run release.yml -f tag=vX.Y.Z`.
- Prepend release notes to **both** `RELEASE_NOTES.md` and `CHANGELOG.md`. The About UI needs no manual edit (`__APP_VERSION__` + GitHub Releases API).
- Bot PR comment triage rules: `docs/BOT_REVIEW_TRIAGE.md` (Fix / Skip / Defer).

## Other Non-Obvious Items

- **`error-logs/`**: drop large logs/stack traces here (git-ignored, excluded from context ingestion); read on demand by filename.
- **Ring buffer log capture**: Rust `src-tauri/src/log_buffer.rs` (2000 lines) + frontend `src/lib/utils/log-buffer.ts` (1000) → `exportLogs()`. Console alone misses Rust output.
- **Browser server lifecycle**: binds `127.0.0.1`, 120s idle heartbeat watchdog shuts it down unless LAN mode is enabled.
- **CSP is enforced in production builds only** (`src-tauri/tauri.conf.json`; no `devCsp`, so `npm run tauri dev` never exercises it — CSP regressions only appear in release/`--debug` bundles). Any new remote `<img>`/`fetch` origin or custom URI scheme must be added to the CSP. Windows custom protocols resolve to `http://{scheme}.localhost` (Tauri v2 default), macOS/Linux to `{scheme}://` — img-src must list both forms.
- **keep_alive config**: when true, the ComfyUI child process survives app close.
- `comfyui-nodes/` is Python nodes installed into ComfyUI at setup — not app build output. `src-tauri/src/comfyui/` is the Rust ComfyUI client, not ComfyUI source.
- **Agent config (canonical)**: `.agents/README.md` — skills in `.agents/skills/`, rules in `.agents/rules/` (mode-specific detail: architect, frontend, rust, debug, ask). `AGENTS.md` is the cross-agent entry point; deeper conventions in `.github/instructions/mooshieui.instructions.md`.
