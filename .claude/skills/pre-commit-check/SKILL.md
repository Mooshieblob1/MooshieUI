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
