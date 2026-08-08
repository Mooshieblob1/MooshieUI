# Contributing to MooshieUI

This guide describes how to land changes on `main` via pull request. `main` is branch-protected: do not push directly to it.

For architecture and agent-specific conventions, see [AGENTS.md](AGENTS.md). For automated bot review triage during releases, see [docs/BOT_REVIEW_TRIAGE.md](docs/BOT_REVIEW_TRIAGE.md).

---

## 1. Fork, clone, and develop

Follow [Development Setup](README.md#development-setup-all-platforms) in the README:

```bash
git clone https://github.com/<your-user>/MooshieUI.git
cd MooshieUI
npm install
npm run tauri dev
```

Work on a topic branch from up-to-date `main`:

```bash
git fetch origin
git checkout main
git pull origin main
git checkout -b chore/<short-description>
```

Use a lowercase slug with hyphens (for example `chore/fix-gallery-thumbnail`).

---

## 2. Security pre-commit hook (required)

Every PR runs **GlassWorm Infection Audit** CI. Install the same checks locally so commits are not blocked unexpectedly:

```bash
bash scripts/setup-hooks.sh
```

This enables `.githooks/pre-commit` (Unicode steganography, tampered git dates, suspicious `eval()` patterns, and related checks). See [Security](README.md#-security) in the README for details.

---

## 3. Validate before you open a PR

There is no frontend test suite (no Vitest/Jest). Rust has ~128 `#[test]` fns covering pure logic. Run the checks that match your changes:

| If you changed… | Run |
|-----------------|-----|
| Svelte, stores, TypeScript, `package.json` | `npm run build` (expect `✓ built in` at the end) |
| Rust (`src-tauri/`) | `cargo check --manifest-path src-tauri/Cargo.toml` |
| Rust logic with test coverage | `cargo test --manifest-path src-tauri/Cargo.toml` (see note below) |
| Rust formatting | `cd src-tauri && cargo fmt --check` |
| Locale files (`src/lib/locales/*.ts`) | Ensure every locale has the same keys and `{placeholders}` as `en.ts` |

The Rust suite is green on `main`. If `cargo test` fails, treat it as a regression from your change, not a known issue.

### Conventions (high-signal)

- **Dual-mode IPC:** All backend calls use `ipcInvoke()` / `ipcListen()` in `src/lib/utils/ipc.ts` — never raw Tauri `invoke()` / `listen()` (browser mode breaks silently).
- **Gallery images:** Stored as JXL on disk; use `loadGalleryImageDisplay()` / `loadGalleryImagePng()` — do not read gallery files directly in the UI.
- **Svelte 5:** No `<style>` blocks; use `onclick` not `on:click`; state stores use `.svelte.ts` and runes, not `svelte/store`.
- **New Tauri commands:** Return `Result<T, AppError>` and add a matching `ipcInvoke` wrapper in `src/lib/utils/api.ts`.

More rule tables live in [`.agents/skills/pre-commit-check/reference.md`](.agents/skills/pre-commit-check/reference.md) (used by maintainers and coding agents).

---

## 4. Commit and open a pull request

```bash
git add -A
git commit -m "fix: short imperative summary"
git push -u origin chore/<short-description>
```

Open a PR against `main` on GitHub. Use a clear title and describe what changed and how you tested it (manual steps are fine).

### Windows (PowerShell)

The repo’s pre-commit hook is bash. In PowerShell, `git commit` can hang unless hooks are disabled for that invocation:

```powershell
git -c core.hooksPath=/dev/null commit -m "your message"
```

Prefer running `bash scripts/setup-hooks.sh` from Git Bash or WSL so normal `git commit` works with hooks enabled.

---

## 5. CI and merge

All PRs must pass **GlassWorm Infection Audit** before merge.

Maintainers (or you, on your fork) can watch checks:

```bash
gh pr checks --watch --interval 30
```

After approval, changes are **squash-merged** into `main` and the PR branch is deleted. Pull latest `main` locally:

```bash
git checkout main
git pull origin main
```

---

## 6. Releases (maintainers only)

Version bumps, `CHANGELOG.md`, `RELEASE_NOTES.md`, git tags, and GitHub Releases are **not** part of regular contribution PRs.

Releases follow a separate workflow: bump `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json` together; update changelog files; open `release/vX.Y.Z`; merge; then tag `vX.Y.Z` on `main` to trigger Build & Release CI.

Maintainers use the release skill/prompt (`.agents/skills/release`, plus `.github/prompts/release.prompt.md`). Contributors should **not** include version-only bumps in `chore/*` PRs unless asked.

---

## Quick checklist

- [ ] Branch from current `main` (`chore/<slug>`)
- [ ] Local GlassWorm hook installed (`scripts/setup-hooks.sh`)
- [ ] `npm run build` and/or `cargo check` as appropriate
- [ ] IPC, gallery, and Svelte conventions respected
- [ ] PR targets `main`; GlassWorm CI green
- [ ] No version bump / changelog edits unless doing a release

---

## Questions

Open a [GitHub issue](https://github.com/Mooshieblob1/MooshieUI/issues) or discuss on an existing PR. For security-sensitive findings, avoid posting exploit details in public issues until maintainers can respond.
