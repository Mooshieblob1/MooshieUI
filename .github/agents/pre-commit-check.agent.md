---
name: pre-commit-check
description: >-
  Validates MooshieUI uncommitted changes before commit or PR — build gates,
  Rust fmt/clippy, conventions, i18n. Use before committing, or when push/release
  skills invoke pre-commit validation.
---

# Pre-Commit Check (MooshieUI)

Run **before** any commit in the push or release workflows. Execute steps in order; **stop on blocking failures**. Only audit **changed** files (unstaged + staged).

## Setup

```powershell
cd "$(git rev-parse --show-toplevel)"
git diff --name-only HEAD
git diff --staged --name-only
```

Combine lists. Classify: `rust` (`src-tauri/**/*.rs`), `config`, `rust-deps`, `svelte`, `store`, `typescript`, `locale`, `python-nodes`, `frontend-deps`.

If empty → report "Nothing to check" and stop.

## Build gates [BLOCKING]

| Step | When | Command |
|------|------|---------|
| Frontend | svelte / store / ts / package.json changed | `npm run build` — PASS if output ends with `✓ built in` |
| Rust compile | rust / rust-deps / config changed | `cargo check --manifest-path src-tauri/Cargo.toml` |
| Rust server build | rust / rust-deps / config changed | `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features server` — the server binary does not link `tauri`, so the desktop check above cannot catch a `tauri` reference that escaped a `#[cfg(feature = "desktop")]` gate |
| Rust fmt | any `.rs` changed | `cd src-tauri; cargo fmt --check` — only **blocking** if diffs overlap **your** changed lines |

If a build gate fails → report full error and **STOP** (skip convention audit).

## Lint [NON-BLOCKING]

If `.rs` changed: `cd src-tauri; cargo clippy` — warn only on **new** issues in changed files.

## Convention audit [NON-BLOCKING unless noted]

Inspect only `+` lines in `git diff HEAD`. Full rule tables: [reference.md](reference.md).

**Blocking convention errors:**
- Svelte: `<style>` blocks; legacy `on:click` / `on:input`; unpinned `installPipPackage("pkg")` (must contain `==`)
- Stores: imports from `svelte/store`
- Utils: duplicate exports
- Rust commands: `#[tauri::command]` must return `Result<T, AppError>`
- Templates: new builders must return complete `WorkflowResult`
- Locales: key parity and interpolation parity (see i18n below)

## i18n [BLOCKING if locale files changed]

- **Canonical:** `src/lib/locales/en.ts`
- **Parity:** every other `src/lib/locales/*.ts` must have the same keys and matching `{placeholder}` names as `en.ts`
- **Export:** each file ends with `export default <name>;` matching filename

## Cross-file [NON-BLOCKING]

If `commands/*.rs` or `lib.rs` changed: matching wrapper in `src/lib/utils/api.ts` using `ipcInvoke()` (not raw `invoke()`).

## Report format

```markdown
## Pre-Commit Check Report

### Files Changed
- path (category)

### Build Gates
- [ ] Frontend build: PASS/FAIL
- [ ] Rust compile: PASS/FAIL
- [ ] Rust formatting: PASS/FAIL

### Lint
- [ ] Clippy: PASS/WARN

### Convention / i18n
- (list findings with file:line)

### Summary
✅ Ready to commit
— or —
❌ N blocking issue(s)
⚠️ N warning(s)
```

## Rules

- Diff-aware; ignore pre-existing issues in untouched lines
- Do **not** auto-fix unless the parent workflow (push/release) is fixing blockers
- Push/release: fix blocking issues, re-run this skill, then continue


---

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
