---
trigger: glob
description: MooshieUI frontend — Svelte 5 runes, ipcInvoke, i18n, Tailwind
globs: src/**/*.svelte,src/**/*.svelte.ts,src/**/*.ts
---

# Code — Frontend (MooshieUI)

## IPC

- `ipcInvoke()` / `ipcListen()` only — wrappers in `src/lib/utils/api.ts`.
- Never `invoke()`/`listen()` from `@tauri-apps/api` in components.

## Svelte 5 state & Stores

- No `svelte/store`. Class singletons with `$state`; files **must** be `*.svelte.ts`.
- **Computed values inside stores**: Use native `get` accessors (e.g. `get percentage() { ... }`), do NOT use `$derived`.
- **Computed values inside components**: Use `$derived` for component-local computed properties.
- **Component state**: Use `$state` for component-local state only. Read shared state directly from stores.
- **Props passing**: Do NOT pass props between components for shared state; read directly from store singletons.
- **Mutate arrays**: Reassign with spread (e.g. `this.arr = [...this.arr, item]`), not `.push()`/`.splice()`.
- **`saveSettings()` is explicit**: Invoke manually after mutations.
- **Robustness**: Wrap `loadSettings()` and `saveSettings()` in try/catch. Never throw from store methods; swallow and log errors to keep UI functional.
- **Loading states**: Use `loading = $state(false)` and reset in a `finally` block. Use `Promise.all()` for parallel fetches.
- **Persisted fields**: Guard with `!== undefined` (0/false/"" are valid).

## Data flow

- `toParams()` maps camelCase → snake_case for Rust.
- Cross-store logic in `App.svelte` only.

## Prompt inline tags

- Parsers in `src/lib/utils/`: `promptSchedule.ts` (`<from/to/range>`, `<fromto[..]>`), `promptSegmentDetail.ts` (`<segment:target,creativity,threshold>`), `promptInertRanges.ts` (shared inert ranges for autocomplete/weighting).
- **Conventions**: invalid tags stay literal text; reset shared regex `lastIndex` before exec loops; opening-tag regexes use `SYNTAX_ANGLE_LOOKBEHIND`.
- In `toParams()`, `<segment>` parsing runs **before** system fragments (styles, quality tags, preset appends) merge into the prompt — trailing-form segments would otherwise swallow them.
- Textarea highlight pills must mirror parser output exactly — render from the parser's `ranges`, never a separate regex.

## i18n

- `locale.t('key')` — no hardcoded user strings.
- Key + `{placeholder}` parity across **all** locale files vs `en.ts`.

## UI & Design System

- Tailwind only; no `<style>` in `.svelte`.
- **Color Palette (Dark theme only)**:
  - `neutral-950` (#0a0a0a) → Main BG
  - `neutral-900` (#171717) → Card BG
  - `neutral-800` (#262626) → Input BG, borders
  - `neutral-700` (#404040) → Active borders, dividers
  - `neutral-500` (#737373) → Placeholder, muted text
  - `neutral-300` (#d4d4d4) → Primary text
- **Accent colors**: Remapped in `app.css` to `--theme-accent-*`. Default is gold; alternativesNord/Solarized set via `data-palette`.
- **Hover Reveal**: Use Tailwind `group` + `group-hover:` utility class patterns.
- **Mobile / Touch Targets**:
  - Minimum hit area of 44px (`touch-target` class).
  - Notch safe area: `safe-top` and `pb-[max(env(safe-area-inset-bottom),1rem)]`.
  - Optimization: Use `tap-highlight-none` and `overscroll-contain` where appropriate.
- Events: `onclick` not `on:click`.
- `installPipPackage("pkg==x.y.z")` — `==` required.

## Images

- Use `loadGalleryImageDisplay()` / `loadGalleryImagePng()` for gallery assets.
