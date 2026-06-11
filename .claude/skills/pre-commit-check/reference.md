# Pre-Commit Convention Reference

Apply only to **changed** files; check `+` lines in `git diff HEAD`.

## Svelte components (`src/lib/components/**/*.svelte`)

| Rule | Severity |
|------|----------|
| No `<style>` blocks | ERROR |
| No `on:click`, `on:input`, `on:change` (use `onclick`, etc.) | ERROR |
| No direct `invoke()` from `@tauri-apps/api` | WARN |
| Tailwind only; no inline `style=` except dynamic values | WARN |
| `installPipPackage` args must include `==` | ERROR |

## Stores (`src/lib/stores/**/*.svelte.ts`)

| Rule | Severity |
|------|----------|
| No `svelte/store` imports | ERROR |
| No `.push()` / `.splice()` on `$state` arrays (use spread) | WARN |
| `generation.svelte.ts`: `saveSettings()` after mutations | WARN |

## TypeScript utils (`src/lib/utils/**/*.ts`)

| Rule | Severity |
|------|----------|
| No duplicate exports (inline + barrel) | ERROR |
| No new `any` in changed lines | WARN |

## Rust commands (`src-tauri/src/commands/**/*.rs`)

| Rule | Severity |
|------|----------|
| `Result<T, AppError>` on commands | ERROR |
| No new `.unwrap()` / `.expect()` | WARN |
| Drop `RwLock` guards before `.await` on I/O | WARN |

## Workflow templates (`src-tauri/src/templates/**/*.rs`)

| Rule | Severity |
|------|----------|
| Complete `WorkflowResult` | ERROR |
| Node IDs via `next_id.to_string()` | WARN |

## Tauri config (`src-tauri/tauri.conf.json`)

| Rule | Severity |
|------|----------|
| `csp` or `capabilities` changes | WARN — manual review |

## Locales (`src/lib/locales/*.ts`)

| Rule | Severity |
|------|----------|
| All files share keys with `en.ts` | ERROR |
| `{var}` placeholders match `en.ts` per key | ERROR |
| New keys: `/^[a-z][a-z0-9]*(\.[a-z][a-z0-9_]*)+$/` | WARN |
| No empty string values | WARN |

## Hardcoded UI strings (svelte / stores)

New user-facing literals should use `locale.t()`. Ignore: class names, console logs, command names, paths, comparisons, comments, strings already inside `locale.t()`.

## Accepted patterns

- Component-local `listen()` in `onMount` for download/install progress is OK
- `.unwrap_or()` / `.unwrap_or_default()` / `.unwrap_or_else()` are OK
