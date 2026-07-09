# Contributing to MooshieUI

Thanks for contributing. This guide covers how to set up, the checks your PR
must pass, and the three core guidelines every change follows: i18n, a11y, and
staying in scope.

## Setup

```bash
npm install
npm run tauri dev     # full desktop dev (Vite hot-reload)
npm run dev           # frontend only
```

Rust checks (if you touch `src-tauri/`): `cargo check --manifest-path src-tauri/Cargo.toml`, and `cargo fmt` + `cargo clippy` from `src-tauri/`.

## Before you open a PR

Run the local pre-flight:

```bash
npm run check
```

This runs, in order:
1. `npm run build` (must pass),
2. `npm run check:i18n` (i18n parity, must pass),
3. `npm run check:types` (svelte-check, informational).

Note on `check:types`: the project has a known baseline of pre-existing
svelte-check errors and warnings, so this pass still prints errors today. It does
not fail `npm run check` (it is a heads-up, not a hard gate locally). What
matters: do not add new type errors or a11y warnings in the files you change. CI
enforces exactly that (see below).

## The three core guidelines

### i18n (blocking)

All user-facing strings go through `locale.t('key')`. Never hardcode display
text. When you add or change a string:
- Add the key to `src/lib/locales/en.ts` (the source of truth).
- Add the same key to every other locale file in `src/lib/locales/`. Missing
  keys fall back to English silently, so the gate catches them for you.
- Keep every `{placeholder}` identical across locales for a given key.

`npm run check:i18n` (and CI) fails on any missing key, extra key, or placeholder
mismatch, and tells you exactly which file and key.

### a11y (advisory, becoming stricter)

Interactive elements must be keyboard- and screen-reader-accessible:
- Use real interactive elements (`<button>`, `<a>`) for click handlers, or add
  keyboard handlers and appropriate roles.
- Associate every `<label>` with a control.
- Provide `alt` text for meaningful images.

CI posts a11y warnings for the files you changed as a non-blocking job summary.
Please fix what you can; these will become blocking once the codebase baseline is
clean.

### Scope (human-reviewed)

Read `SCOPE.md`. Every PR states how it fits the charter, and every non-trivial
feature should start as an issue so scope is agreed before you build. Out-of-scope
changes are declined regardless of quality.

## Other conventions

- Backend calls go through `ipcInvoke()` / `ipcListen()` (`src/lib/utils/ipc.ts`),
  never raw `invoke()` / `listen()` from `@tauri-apps/api`.
- Styling is Tailwind only. No `<style>` blocks in `.svelte` files.
- Use `onclick`, not the legacy `on:click`.
- Rust `#[tauri::command]` functions return `Result<T, AppError>`.

## What CI checks and how to fix each

| Check | Blocking? | What it means | How to fix |
|---|---|---|---|
| GlassWorm Infection Audit | Yes | Security/steganography scan | Remove flagged content; see the check log |
| Guardrails: build | Yes | `npm run build` failed | Run `npm run build` locally and fix the error |
| Guardrails: i18n | Yes | Missing key/placeholder in a locale | Run `npm run check:i18n`; add the key to all locales |
| Guardrails: types | Yes | New svelte-check error in a file you changed | Run `npm run check:types`; fix errors in your changed files |
| a11y advisory | No | a11y warnings in changed files | See the job summary; fix what you can |

The types check is diff-scoped: it only fails on errors in the files your PR
changed, not the pre-existing baseline.
