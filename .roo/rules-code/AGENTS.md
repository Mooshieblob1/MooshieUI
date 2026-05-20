# Code Mode Rules (Non-Obvious Only)

## IPC Layer — ALWAYS use, never bypass

- **Use `ipcInvoke()`/`ipcListen()` from [`src/lib/utils/ipc.ts`](src/lib/utils/ipc.ts)** for ALL backend communication. Never import or call `invoke()`/`listen()` directly from `@tauri-apps/api`.
- `ipcInvoke()` routes to Tauri IPC in desktop mode and HTTP POST in browser mode. Direct `invoke()` calls silently fail in browser mode.
- Frontend API wrappers live in [`src/lib/utils/api.ts`](src/lib/utils/api.ts) — add new wrappers there, not inline in components.

## State Management — Svelte 5 Runes ONLY

- **No legacy Svelte stores**: Do NOT import `writable`/`readable`/`derived` from `svelte/store`.
- Stores are **classes with `$state` rune fields**, exported as **singleton instances**.
- Store files MUST use `.svelte.ts` extension — this is required for Svelte 5 rune compilation.
- **Array mutations**: Reassign with spread (`this.arr = [...this.arr, item]`) — do NOT use `.push()`, `.splice()`, etc. on `$state` arrays.
- **`saveSettings()` is manual**: Called explicitly after mutations. Does NOT auto-save on field change.
- Guard nullable persisted fields with `!== undefined`, not truthiness — `0`, `false`, `""` are valid saved values.

## Frontend → Backend Data Flow

- The `toParams()` pattern in stores converts camelCase TypeScript fields to snake_case for the Rust backend. When adding params: add `$state` field → add to `loadSettings()`/`saveSettings()` → add snake_case mapping in `toParams()` → add to Rust `GenerationParams` struct.
- Cross-store coordination happens in `App.svelte`. Stores do NOT import each other directly.

## Rust Patterns

- **Drop `RwLock` guards before `.await`** in Rust. Holding `config.read().await` across an I/O await point can deadlock.
- Workflow template IDs are **string numbers** (`"1"`, `"2"`, ...). Use `next_id.to_string()` counter pattern. Track connection sources as `(String, u32)` tuples.
- New Tauri commands: add handler in `commands/`, register in `lib.rs`'s `generate_handler![...]`, add wrapper in `src/lib/utils/api.ts`.
- **Use `AppError` enum** for all command returns. Never panic, never return raw strings.

## i18n / Locales

- **Key parity required**: Every key in [`en.ts`](src/lib/locales/en.ts) must be present in every other locale file or `locale.t()` silently falls back to English.
- Interpolation variables (`{varName}`) must match exactly between all locale files.
- **No hardcoded user-visible strings** in components — use `locale.t('key')` from `$lib/stores/locale.svelte.js`.

## Styling

- **Tailwind CSS utility classes only** — no `<style>` blocks in `.svelte` files.
- Dark theme only: `neutral-950` through `neutral-50` backgrounds/text, `indigo` accent.
- Dynamic inline styles allowed only for computed values (progress bar width, etc.).

## Critical Gotchas

- **`installPipPackage()` must pin versions**: Always include `==` (e.g., `"ultralytics==8.4.34"`, not `"ultralytics"`). Unpinned versions will be rejected.
- **Svelte 5 event syntax**: Use `onclick`, `oninput`, `onchange` — NOT the legacy `on:click` directive syntax.
- Gallery images are JXL format — use `loadGalleryImageDisplay()`/`loadGalleryImagePng()` to read them. Never read gallery files directly from disk.
