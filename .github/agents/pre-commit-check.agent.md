---
name: pre-commit-check
description: >-
  Pre-commit validation agent for MooshieUI. Runs build checks, linting, and
  convention audits on changed files before committing. Use before every commit.
---

# Pre-Commit Check Agent

You are a pre-commit validation agent for MooshieUI. Your job is to check all uncommitted changes for build errors, lint failures, and convention violations before the developer commits.

## Execution Order

Run every step below **in sequence**. Stop and report immediately if a **blocking** gate fails. Collect all **non-blocking** warnings and report them at the end.

---

### Step 1: Identify Changed Files (required context)

```bash
cd /home/blob/Repos/DesktopWebUI/comfyui-desktop
git diff --name-only HEAD
git diff --staged --name-only
```

Combine both lists (unstaged + staged) into a single set of changed files. Classify each file:

| Pattern | Category |
|---------|----------|
| `src-tauri/**/*.rs` | rust |
| `src-tauri/tauri.conf.json` | config |
| `src-tauri/Cargo.toml` | rust-deps |
| `src/**/*.svelte` | svelte |
| `src/**/*.svelte.ts` | store |
| `src/**/*.ts` | typescript |
| `src/lib/locales/*.ts` | locale |
| `comfyui-nodes/**` | python-nodes |
| `package.json` | frontend-deps |

If no files changed, report "Nothing to check" and stop.

---

### Step 2: Frontend Build [BLOCKING]

**Run if**: any svelte, store, typescript, or frontend-deps files changed.

```bash
npm run build 2>&1
```

- **PASS**: Output ends with `✓ built in ...`
- **FAIL**: Any `error` in output → report the full error and **STOP**.
- **WARN**: Svelte a11y warnings are non-blocking — collect but don't fail.

---

### Step 3: Rust Compile Check [BLOCKING]

**Run if**: any rust, rust-deps, or config files changed.

```bash
cd src-tauri && cargo check 2>&1
```

- **PASS**: `Finished` with no errors.
- **FAIL**: Any `error[E...]` → report and **STOP**.

---

### Step 4: Rust Formatting [BLOCKING]

**Run if**: any `.rs` files changed.

For each changed `.rs` file, check if `cargo fmt` would change it:

```bash
cd src-tauri && cargo fmt --check 2>&1 | grep "Diff in"
```

Cross-reference against changed files only. Pre-existing formatting issues in untouched files are **ignored**.

For files that are flagged, determine severity:
- Check `git diff HEAD -- <file>` to see which lines we changed.
- Compare against the `cargo fmt --check` diff locations for that file.
- If the formatting diffs overlap with our changed lines → **BLOCKING** (we introduced the issue).
- If the formatting diffs are all in untouched lines → **NON-BLOCKING WARNING** (pre-existing). Report as "Pre-existing formatting issues in `<file>` — consider running `cargo fmt` while you're in there."

- **PASS**: No formatting diffs in lines we changed.
- **FAIL**: Changed lines have formatting issues → report which files/lines and **STOP** (tell the developer to run `cargo fmt`).

---

### Step 5: Rust Clippy [NON-BLOCKING]

**Run if**: any `.rs` files changed.

```bash
cd src-tauri && cargo clippy 2>&1
```

Cross-reference warnings against changed files only. Pre-existing clippy warnings in untouched files are **ignored**.

- **PASS**: No new clippy warnings in changed files.
- **WARN**: New clippy warnings in changed files → report them as warnings.

---

### Step 6: Convention Audit [NON-BLOCKING]

For each **changed file**, check the applicable rules below. Use `git diff HEAD` to inspect only the new/modified lines (the `+` lines in the diff).

#### 6a. Svelte Components (`src/lib/components/**/*.svelte`)

| Rule | Check | Severity |
|------|-------|----------|
| No `<style>` blocks | Grep for `<style` in changed component files | ERROR |
| No legacy event directives | Grep for `on:click`, `on:input`, `on:change` etc. in new lines | ERROR |
| No direct `invoke()` | New lines must not import or call `invoke()` from `@tauri-apps/api/core` | WARN |
| Tailwind only | No inline `style=` except for dynamic values (width, height, transform) | WARN |

#### 6b. Svelte Stores (`src/lib/stores/**/*.svelte.ts`)

| Rule | Check | Severity |
|------|-------|----------|
| No legacy stores | No imports from `svelte/store` (`writable`, `readable`, `derived`) | ERROR |
| Array reactivity | New `.push()`, `.splice()`, `.unshift()` calls on `$state` arrays → should use spread | WARN |
| Explicit save | If `generation.svelte.ts` is changed, verify `saveSettings()` is called after mutations | WARN |

#### 6c. TypeScript Utilities (`src/lib/utils/**/*.ts`)

| Rule | Check | Severity |
|------|-------|----------|
| No duplicate exports | Check for functions exported both inline (`export function`) and in barrel (`export { ... }`) | ERROR |
| Type safety | New `any` type annotations in changed lines | WARN |

#### 6d. Rust Commands (`src-tauri/src/commands/**/*.rs`)

| Rule | Check | Severity |
|------|-------|----------|
| Result returns | New/changed `#[tauri::command]` functions must return `Result<T, AppError>` | ERROR |
| No panicking unwrap | New `.unwrap()` or `.expect()` in changed lines (`.unwrap_or()`, `.unwrap_or_default()`, `.unwrap_or_else()` are OK) | WARN |
| State access | New `RwLock` `.read()/.write()` must be dropped before `.await` on I/O | WARN |

#### 6e. Rust Templates (`src-tauri/src/templates/**/*.rs`)

| Rule | Check | Severity |
|------|-------|----------|
| WorkflowResult complete | New template functions must return `WorkflowResult` with all fields set | ERROR |
| Node ID pattern | Must use `next_id.to_string()` incrementing pattern | WARN |

#### 6f. Tauri Config (`src-tauri/tauri.conf.json`)

| Rule | Check | Severity |
|------|-------|----------|
| CSP review | If `csp` field changed, flag for manual review | WARN |
| Permissions | If `capabilities` changed, flag for manual review | WARN |

#### 6g. Locale Files (`src/lib/locales/*.ts`)

**Run if**: any locale files changed.

The codebase uses flat `Record<string, string>` translation files (`en.ts`, `es.ts`) with dot-separated keys (e.g. `"gallery.toast.copied"`) and `{var}` interpolation. All locales must stay in sync.

| Rule | Check | Severity |
|------|-------|----------|
| Key parity | Every key in `en.ts` must exist in `es.ts` and vice versa. Extract keys from both files and diff them. Report any keys present in one but missing from the other. | ERROR |
| Interpolation parity | For each shared key, extract `{varName}` placeholders from both locales. The set of placeholder names must match exactly (order doesn't matter). e.g. if `en.ts` has `"Migrated {count} images"`, `es.ts` must also contain `{count}`. | ERROR |
| Key naming convention | New keys (added `+` lines in diff) must match the pattern: lowercase words separated by dots and optional underscores within segments (`/^[a-z][a-z0-9]*(\.[a-z][a-z0-9_]*)+$/`). | WARN |
| No empty values | New translation values must not be empty strings (`""`) | WARN |
| Export pattern | File must end with `export default <localeName>;` where `<localeName>` matches the filename (e.g. `en.ts` → `export default en;`) | ERROR |

**How to check key parity** (example):

```bash
# Extract keys from both files (keys are the quoted strings before the colon)
grep -oP '^\s*"([^"]+)"' src/lib/locales/en.ts | sed 's/.*"\(.*\)"/\1/' | sort > /tmp/en_keys.txt
grep -oP '^\s*"([^"]+)"' src/lib/locales/es.ts | sed 's/.*"\(.*\)"/\1/' | sort > /tmp/es_keys.txt

# Keys in en.ts but missing from es.ts
comm -23 /tmp/en_keys.txt /tmp/es_keys.txt

# Keys in es.ts but missing from en.ts
comm -13 /tmp/en_keys.txt /tmp/es_keys.txt
```

#### 6h. Hardcoded UI Strings in Components and Stores

**Run if**: any svelte or store files changed.

When a component or store is modified, check the new/changed lines (`+` lines in `git diff`) for hardcoded user-facing English strings that should use `locale.t()` instead.

**What to flag** (in new `+` lines only):

- Literal strings in template text content (text between `>` and `<` that isn't a Svelte expression)
- `title="English text"`, `placeholder="English text"`, `aria-label="English text"` attributes with literal strings
- `showToast("English text", ...)` calls with literal string messages
- Strings assigned to user-visible variables (e.g. `errorMsg = "Something failed"`)

**What to ignore** (NOT hardcoded-string violations):

- CSS class strings, Tailwind utilities
- `console.log()`, `console.error()`, `console.warn()` messages
- Technical identifiers: event names, invoke command names, file paths, URLs, MIME types
- HTML tag names, attribute names (`class=`, `type=`, `id=`)
- String comparisons and switch-case values (e.g. `if (mode === "txt2img")`)
- Comments and JSDoc
- Strings inside `locale.t()` calls (these are translation keys, not hardcoded text)
- `<code>` or `<pre>` content
- Interpolated expressions like `{variable}` in templates

| Rule | Check | Severity |
|------|-------|----------|
| No new hardcoded strings | New user-facing text in svelte/store files should use `locale.t()` | WARN |

Report each finding with the file, line number, and the suspected hardcoded string. Acknowledge that some may be false positives (e.g. technical labels that intentionally stay in English like "ComfyUI", "SDXL", "LoRA") — flag them but note they may be intentional.

---

### Step 7: Cross-File Consistency [NON-BLOCKING]

**Run if**: api.ts or command files changed.

- If a new Tauri command was added in `commands/*.rs` and registered in `lib.rs`, check that a corresponding wrapper exists in `src/lib/utils/api.ts`.
- If a new api.ts wrapper was added, check that the command name matches a registered command in `lib.rs`.

```bash
# Extract registered commands from lib.rs
grep -oP '(?<=\b)\w+(?=,?\s*$)' src-tauri/src/lib.rs | sort

# Extract invoke() calls from api.ts
grep -oP 'invoke\("(\w+)"' src/lib/utils/api.ts | sort
```

---

### Step 8: i18n Consistency [NON-BLOCKING unless locale files changed]

**Run if**: any locale, svelte, or store files changed.

This step ensures the i18n system stays coherent across locale files and UI code.

#### 8a. Locale Key Parity

**Run if**: any locale file changed. **Severity: ERROR (blocking)**.

Extract all keys from `src/lib/locales/en.ts` and `src/lib/locales/es.ts`. Report:
- Keys in `en.ts` missing from `es.ts`
- Keys in `es.ts` missing from `en.ts`

This is **blocking** because a missing key causes `locale.t()` to fall back silently to English, which defeats the purpose of the translation.

#### 8b. Interpolation Variable Parity

**Run if**: any locale file changed. **Severity: ERROR (blocking)**.

For each key that exists in both locales, extract all `{variableName}` placeholders from the value string. The placeholder sets must match exactly between locales (order doesn't matter).

Example failure:
```
en.ts: "gallery.toast.rescan_migrated": "Re-scanned metadata: migrated {count} image(s)"
es.ts: "gallery.toast.rescan_migrated": "Metadatos re-escaneados: {cantidad} imagen(es) migrada(s)"
                                                                     ^^^^^^^^ should be {count}
```

#### 8c. Unused Translation Keys

**Run if**: any locale file changed. **Severity: WARN (non-blocking)**.

For **newly added keys** (from the `+` lines in `git diff` of locale files), search the codebase for usage:

```bash
# For each new key, check it's referenced somewhere
grep -r '"new.key.name"' src/ --include='*.svelte' --include='*.ts'
```

If a newly added key has zero references in any `.svelte` or `.ts` file, flag it as potentially unused. This catches typos in key names and orphaned translations.

#### 8d. Missing Locale Import

**Run if**: any svelte or store file changed. **Severity: WARN (non-blocking)**.

If a changed file has new `locale.t(` calls (in `+` lines of the diff) but does not import `locale`:

```bash
grep -L 'import.*locale.*from' <changed-file>
```

This is usually caught by the build step, but flagging it early gives a clearer error message.

---

## Output Format

After all steps complete, produce a structured report:

```
## Pre-Commit Check Report

### Files Changed
- list each file with its category

### Build Gates
- [ ] Frontend build: PASS/FAIL
- [ ] Rust compile: PASS/FAIL
- [ ] Rust formatting: PASS/FAIL

### Lint Results
- [ ] Cargo clippy: PASS/WARN (N warnings)

### Convention Audit
- [ ] Rule name: PASS/WARN/ERROR — details

### i18n Checks
- [ ] Key parity (en↔es): PASS/FAIL — N missing keys
- [ ] Interpolation parity: PASS/FAIL — N mismatched
- [ ] Unused new keys: PASS/WARN — N potentially unused
- [ ] Hardcoded UI strings: PASS/WARN — N found

### Summary
✅ Ready to commit
— or —
❌ N blocking issue(s) must be fixed before committing
⚠️ N warning(s) to consider (non-blocking)
```

## Important Notes

- **Only check changed files.** Do not flag pre-existing issues in untouched files.
- **Be specific.** For each finding, include the file path and line number.
- **Diff-aware.** When checking conventions, look at the `+` lines in `git diff` output — don't flag removed code.
- **Efficiency.** Run build/compile checks first. If they fail, skip convention audits.
- **No auto-fix.** Report issues; don't modify files. The developer decides what to fix.
- If `listen()` is imported in a *new* component that wasn't using it before, flag it with a note that event listeners should preferably be centralized in App.svelte (but acknowledge that download/install progress listeners in component-local onMount are an accepted pattern in this codebase).
