---
trigger: manual
description: MooshieUI project map, naming, docs index. Use when explaining the codebase or answering where-things-live questions.
---

# Ask / Reference (MooshieUI)

## Identity

- Product: **MooshieUI**; npm package `comfyui-desktop`; Tauri id `com.mooshieui.desktop`.

## Layout

- `comfyui-nodes/`: Python nodes installed into ComfyUI at setup — not app build output.
- `src-tauri/src/comfyui/`: Rust ComfyUI client — not ComfyUI source.
- `src/main.ts`: Svelte entry; `server_main.rs`: `--features server` binary.

## i18n

- Flat `Record<string, string>` in `src/lib/locales/`; keys like `"gallery.toast.copied"`.
- Only `locale.t()` from `$lib/stores/locale.svelte.js`.

## Docs index

| Topic | Path |
|-------|------|
| Index | `docs/README.md` |
| Overview | `GEMINI.md`, `.github/copilot-instructions.md` |
| Full conventions | `.github/instructions/mooshieui.instructions.md` |
| Layer rules | `.github/instructions/` (svelte-*, tauri-backend) |
| Workflows / skills | `.agents/skills/` (mirrored in `.claude/skills/`) |
| Release / push | `push`, `release` skills |
| Pre-commit | `pre-commit-check` skill |
| Bot PR triage | `docs/BOT_REVIEW_TRIAGE.md` |
| Feature research | `docs/FEATURE_RESEARCH.md` |

## Config notes

- `svelte.config.js`: suppresses `a11y_label_has_associated_control`.
- `vite.config.ts`: `__APP_VERSION__` from `package.json`; dev port 1420 strict.

## Storage

- Gallery: `.jxl` on disk (`jxl-oxide` / `jxl-encoder`).
- Thumbnails: generated WebP, not separate files.
