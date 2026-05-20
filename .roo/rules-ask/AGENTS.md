# Ask Mode Rules (Non-Obvious Only)

## Project Identity

- The npm package is named `comfyui-desktop` but the product/app name is `MooshieUI`. The repo is `MooshieUI`.
- The Tauri app identifier is `com.mooshieui.desktop` — not comfyui or anything else.

## Counterintuitive Organization

- `comfyui-nodes/` contains **Python nodes installed into ComfyUI** at setup time, not part of the main app build. These are custom ComfyUI extensions for tiled diffusion and nanosaur support.
- `src-tauri/src/comfyui/` is Rust code for interacting with ComfyUI's API — not the ComfyUI source itself.
- `src/main.ts` is the Svelte entry point; `src-tauri/src/main.rs` is a thin Windows subsystem wrapper that delegates to `lib.rs`.
- `src-tauri/src/server_main.rs` is the server-mode binary entry point (compiled with `--features server`).

## Two Separate i18n Systems

- Frontend locales are flat `Record<string, string>` files in [`src/lib/locales/`](src/lib/locales/) with dot-separated keys (e.g. `"gallery.toast.copied"`).
- `locale.t('key')` from `$lib/stores/locale.svelte.js` is the only way to get translated strings — no other i18n library is in use.
- All locale files must maintain **exact key parity** with [`en.ts`](src/lib/locales/en.ts) — missing keys silently fall back to English with no warning.

## Primary Reference Documentation

- [`GEMINI.md`](GEMINI.md) and [`.github/copilot-instructions.md`](.github/copilot-instructions.md) are the primary AI assistant documentation.
- [`.github/instructions/`](.github/instructions/) has per-layer conventions: `svelte-components.instructions.md`, `svelte-stores.instructions.md`, `tauri-backend.instructions.md`.
- [`.github/prompts/release.prompt.md`](.github/prompts/release.prompt.md) and [`.github/prompts/add-generation-param.prompt.md`](.github/prompts/add-generation-param.prompt.md) document specific workflows.
- [`.github/agents/pre-commit-check.agent.md`](.github/agents/pre-commit-check.agent.md) defines the automated pre-commit validation rules.

## Configuration Files

- `svelte.config.js` suppresses one specific a11y warning: `a11y_label_has_associated_control`.
- `vite.config.ts` injects `__APP_VERSION__` global at build time from `package.json` version field.
- The Vite dev server enforces `strictPort: true` on port 1420 and expects `TAURI_DEV_HOST` env var for HMR configuration.

## Storage

- Gallery images use **JXL (JPEG XL)** format on disk with `.jxl` extension. The `jxl-oxide` crate handles decoding, `jxl-encoder` handles encoding.
- Thumbnails are auto-generated WebP images served through the custom `thumbnail://` URI scheme (not stored as separate files).
