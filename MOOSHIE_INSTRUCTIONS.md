# MooshieUI Development Instructions

> **Static context file** — place at the very top of every API payload. Do not inject invisible/ changing cursor data. This content is intentionally stable to maximize cache hits.

---

## Project Identity

MooshieUI is a desktop frontend for ComfyUI — **Svelte 5 + Tauri v2 (Rust)**. Dual-mode architecture: runs as a Tauri native desktop app AND as a browser-based web app via embedded axum server. npm package name is `comfyui-desktop`; product name is `MooshieUI`.

| Layer | Technology |
|-------|------------|
| Frontend | Svelte 5, TypeScript 6, Tailwind CSS 4 |
| Desktop | Tauri v2 (Rust) |
| State | Svelte 5 runes (`$state`, `$derived`) — NOT Svelte stores |
| Persistence | `@tauri-apps/plugin-store` (JSON) in desktop; localStorage in browser |
| Backend API | ComfyUI REST + WebSocket (proxied via Rust) |
| Build | Vite 6 + `@sveltejs/vite-plugin-svelte` |

## Build Commands

```bash
npm install                  # Frontend dependencies
npm run dev                  # Vite dev server (port 1420)
npm run build                # Production frontend → dist/
npm run tauri dev            # Full dev (Tauri + frontend hot-reload)
npm run tauri build          # Cross-platform binary
cargo check                  # Rust compile check (run in src-tauri/)
cargo fmt                    # Rust format (run in src-tauri/)
cargo clippy                 # Rust lint (run in src-tauri/)
```

**No test framework exists.** No vitest/jest on frontend, no `#[test]` modules in Rust.

## Architecture (Non-Obvious)

- **Dual-mode**: Tauri webview OR browser via axum. The flag `window.__MOOSHIE_BROWSER_MODE__` determines mode. IPC routes through custom [`src/lib/utils/ipc.ts`](src/lib/utils/ipc.ts) — `ipcInvoke()`/`ipcListen()` — NEVER use `invoke()`/`listen()` directly from `@tauri-apps/api`.
- **JXL storage**: Gallery images stored as JPEG XL format. Display via `loadGalleryImageDisplay()` (JXL→WebP), export via `loadGalleryImagePng()` (JXL→PNG). Never read gallery files directly.
- **Custom URI schemes**: `thumbnail://` and `gallery://` protocols registered in Tauri for gallery image loading.
- **CSP is null** in `tauri.conf.json` — no Content Security Policy restrictions.
- **Ring buffer log capture**: Both Rust and frontend maintain bounded ring buffers for `exportLogs()` diagnostics.
- **`keep_alive` config**: When true, ComfyUI process survives app close. Otherwise the app kills ComfyUI on exit.

---

## MooshieUI Design System

### Color Palette

**Dark theme only** — no light mode. Colors use Tailwind neutral scale (50–950) with a theme-accent system.

```css
/* Neutral (backgrounds, borders, text) */
neutral-950  →  #0a0a0a  (main bg)
neutral-900  →  #171717  (card bg)
neutral-800  →  #262626  (input bg, borders)
neutral-700  →  #404040  (active borders, dividers)
neutral-600  →  #525252  (subtle borders)
neutral-500  →  #737373  (placeholder, muted text)
neutral-400  →  #a3a3a3  (secondary text)
neutral-300  →  #d4d4d4  (primary text on dark)
neutral-200  →  #e5e5e5  (highlight text)
neutral-100  →  #f5f5f5  (bright text)
neutral-50   →  #fafafa  (white text)

/* Accent (indigo → theme-accent remapped in app.css) */
indigo-500/600  →  primary actions, fill bars, active states
indigo-400       →  hover states
indigo-300       →  active text
```

**Accent remapping**: CSS variables in `app.css` remap `indigo-*` and `purple-*` to `--theme-accent-*`. Default palette is gold; `data-palette="nord"` and `data-palette="solarized"` are alternatives.

### Typography

| Usage | Class | Size |
|-------|-------|------|
| Page title | `text-base font-semibold` | 16px |
| Section heading | `text-sm font-semibold` | 14px |
| Body text | `text-sm` | 14px |
| Labels | `text-xs font-medium` | 12px |
| Fine print | `text-[11px]` / `text-[10px]` | 11px / 10px |
| Numeric displays | `tabular-nums` | monospaced digits |
| Text overflow | `truncate` or `line-clamp-2`/`line-clamp-3` | |

### Spacing

- **Cards/Sections**: `rounded-xl border border-neutral-800 bg-neutral-900/60 p-4 space-y-3`
- **Button padding**: `px-3 py-2` (standard), `px-4 py-2` (wider), `px-3 py-1.5` (compact)
- **Input/Select**: `rounded-lg px-3 py-2` (standard), `py-2.5` (touch-target), `text-sm`
- **Gaps**: `gap-1` (tight), `gap-2` (standard), `gap-3` (relaxed), `space-y-2`/`space-y-3` (vertical lists)
- **Dividers**: `w-px h-6 bg-neutral-700 mx-2` (vertical), `h-px bg-neutral-700 my-1` (horizontal)

### Borders

| Element | Class |
|---------|-------|
| Card border | `border-neutral-800` |
| Interactive border | `border-neutral-700` |
| Active/focus border | `border-indigo-500` |
| Error border | `border-red-700/50` |
| Bottom separator | `border-b border-neutral-800` |

### Rounded Corners

| Element | Class |
|---------|-------|
| Cards, modals | `rounded-xl` |
| Buttons, inputs, selects | `rounded-lg` |
| Small buttons, badges | `rounded-md` or `rounded` |
| Pills, chips, badges | `rounded-full` |
| Progress bars | `rounded-full` |

---

## UI Component Rules

### Buttons

**Primary Action**:
```svelte
<button class="bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg px-3 py-2 text-sm font-medium transition-colors">
```

**Secondary / Outline**:
```svelte
<button class="border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 rounded-lg px-3 py-2 text-sm transition-colors">
```

**Ghost / Icon**:
```svelte
<button class="text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800 rounded p-1 transition-colors">
```

**Disabled**:
```
disabled:opacity-50 disabled:cursor-not-allowed
```

**Toggle (active/inactive ternary)**:
```svelte
class="{active
  ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300'
  : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}"
```

**Touch target for mobile**: `touch-target` class (minimum 44px hit area).

### Inputs & Selects

```svelte
<!-- Text input -->
<input class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm
              text-neutral-100 placeholder-neutral-500
              focus:outline-none focus:border-indigo-500 transition-colors" />

<!-- Select/Dropdown -->
<select class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2.5
               text-sm text-neutral-100">

<!-- Range slider -->
<input type="range" min="0" max="100" class="w-full accent-indigo-500" />

<!-- Checkbox -->
<input type="checkbox" class="accent-indigo-500" />
```

### Section Cards

```svelte
<section class="rounded-xl border border-neutral-800 bg-neutral-900/60 p-4 space-y-3">
  <h2 class="text-xs font-semibold uppercase tracking-wide text-neutral-400">
    Section Title
  </h2>
  <!-- content -->
</section>
```

### Progress Bars

```svelte
<div class="w-full h-2 bg-neutral-800 rounded-full overflow-hidden">
  <div class="h-full rounded-full transition-[width] duration-200
              {upscaled ? 'bg-emerald-500' : 'bg-indigo-500'}"
       style="width: {percent}%">
  </div>
</div>
```

**Color coding**: `bg-indigo-500` (generation), `bg-emerald-500` (upscale), `bg-violet-500` (server-side progress).

### Loading Spinners

```svelte
<div class="w-4 h-4 border-2 border-neutral-600 border-t-indigo-500 rounded-full animate-spin"></div>
```

### Separators

```svelte
<!-- Vertical divider in toolbar -->
<div class="w-px h-6 bg-neutral-700 mx-2"></div>

<!-- Horizontal divider -->
<div class="h-px bg-neutral-700 my-1"></div>
```

### Tooltips (InfoTip)

```svelte
<span class="inline-flex items-center ml-1">
  <button type="button"
    class="w-3.5 h-3.5 rounded-full border border-neutral-600 text-neutral-500
           hover:border-neutral-400 hover:text-neutral-300 transition-colors
           inline-flex items-center justify-center text-[9px] leading-none font-medium cursor-help"
    title="Explanation text">
    ?
  </button>
</span>
```

### SVG Icons

All icons are inline SVGs with these standard attributes:
```svg
xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24"
fill="none" stroke="currentColor" stroke-width="2"
stroke-linecap="round" stroke-linejoin="round"
```
Common sizes: `w-3 h-3` (tiny), `w-4 h-4` (small), `w-5 h-5` (standard), `w-6 h-6` (large).

### Hover Reveal Pattern

```svelte
<div class="relative group">
  <div class="absolute top-3 right-3 flex gap-2
              opacity-0 group-hover:opacity-100 transition-opacity">
    <!-- overlay controls -->
  </div>
  <!-- content -->
</div>
```

### Toast Notifications

```
fixed bottom-4 right-4 z-50 px-4 py-2.5 rounded-lg border text-sm shadow-lg
  success: bg-emerald-900/90 border-emerald-700 text-emerald-200
  error:   bg-red-900/90 border-red-700 text-red-200
  info:    bg-neutral-800 border-neutral-700 text-neutral-200
```

### Mobile-Specific Patterns

- **Mobile bottom sheet**: `fixed bottom-0 z-50 bg-neutral-900 border-t border-neutral-800 rounded-t-2xl`
- **Mobile tab bar**: `bg-neutral-950/95 backdrop-blur border-t border-neutral-800`
- **Mobile top bar**: `bg-neutral-950/95 backdrop-blur safe-top` with `h-12`
- **Mobile section toggles**: Collapsible sections with `border-neutral-800 bg-neutral-900/60 rounded-xl`
- **safe-area**: `pb-[max(env(safe-area-inset-bottom),1rem)]` and `safe-top` for iOS/mobile notches
- **Touch optimization**: `tap-highlight-none`, `touch-target`, `overscroll-contain`, `no-scroll-chain`

---

## Svelte 5 Conventions (Non-Obvious)

- **Stores**: Class-based singletons with `$state` rune fields. File extension MUST be `.svelte.ts`. Export as `export const store = new StoreClass()`.
- **No legacy stores**: Do NOT import `writable`/`readable`/`derived` from `svelte/store`.
- **Array mutations**: Always reassign with spread — `this.arr = [...this.arr, item]`. Never use `.push()`, `.splice()` on `$state` arrays.
- **`saveSettings()` is manual**: Called explicitly after mutations. Does not auto-save.
- **Event syntax**: `onclick`, `oninput`, `onchange` — NOT legacy `on:click` directive.
- **No `<style>` blocks**: Tailwind CSS utility classes only. Dynamic inline styles allowed only for computed values (progress width, transform, etc.).
- **IPC**: All backend calls through `src/lib/utils/api.ts` wrappers calling `ipcInvoke()`. Components never import `invoke()` directly.

## Rust Conventions (Non-Obvious)

- **Drop `RwLock` guards before `.await`**: Holding `config.read().await` across I/O can deadlock.
- **`AppError` enum**: All Tauri commands return `Result<T, AppError>`. Never panic, never return raw strings.
- **Workflow templates**: Node IDs are string numbers (`"1"`, `"2"`, ...) using `next_id.to_string()` counter. Connections are `(String, u32)` tuples.
- **New commands**: Add handler in `commands/`, register in `lib.rs` `generate_handler![...]`, add TS wrapper in `api.ts`.
- **Use `state.http_client`**: Shared reqwest client with connection pooling. Never create new clients per request.

## i18n Rules

- **Key parity**: Every key in `en.ts` must exist in all locale files. Missing keys silently fall back to English.
- **Interpolation variables** (`{varName}`) must match exactly between locales.
- **No hardcoded user-visible strings** in components — use `locale.t('key')`.
- Locale keys use dot-separated lowercase: `gallery.toast.copied`, `settings.display`.

## Critical Gotchas

- **`installPipPackage()` must pin versions**: Always include `==` (e.g., `"ultralytics==8.4.34"`, not `"ultralytics"`). Unpinned rejected.
- **Pre-commit hook is bash**: Hangs in PowerShell. Use `git -c core.hooksPath=/dev/null` on Windows.
- **Version in 3 files must match**: [`package.json`](package.json:5), [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml:3), [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json:4).
- **Tag protection**: Tags cannot be deleted or force-updated. Use `workflow_dispatch` fallback.
- **Stores do NOT import each other**: Cross-store coordination happens in `App.svelte`.
- **`toParams()` pattern**: camelCase store fields → snake_case Rust params. Manual mapping in `generation.svelte.ts`.
