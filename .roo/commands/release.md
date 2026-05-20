---
name: release
description: Cut a new MooshieUI release — bump versions, update release notes, validate, tag, and push
argument-hint: "Version number (e.g. 0.4.3) and a brief summary of changes"
---

Cut a new MooshieUI release. Execute every step autonomously — do NOT pause for confirmation between steps.

## Required Information

If NOT provided in the user's message, ask for:
1. **Version number** (e.g. `0.4.3`) — must be semver, no `v` prefix
2. **Summary of changes** — what features/fixes to include in release notes

If the user says "release it" or "cut a release" without a version, read the current version from `package.json` and auto-increment the patch number. If no summary is given, derive it from `git log` since the last tag.

## Execution Plan

### 1. Run pre-commit-check agent

Invoke the `pre-commit-check` agent before anything else. If it reports failures, fix them before continuing.

### 2. Bump version in 3 files

All three must have the **exact same version string**:

- **`package.json`** → `"version": "X.Y.Z"`
- **`src-tauri/Cargo.toml`** → `version = "X.Y.Z"` (under `[package]`)
- **`src-tauri/tauri.conf.json`** → `"version": "X.Y.Z"`

After bumping, verify all three match. `Cargo.lock` updates automatically on next `cargo check`.

### 3. Update RELEASE_NOTES.md and CHANGELOG.md

**RELEASE_NOTES.md** — prepend a new section above existing content:

```markdown
## What's New in vX.Y.Z

### Feature/Fix Title
- Description

---

## What's New in vPREVIOUS
(existing content below)
```

**CHANGELOG.md** — prepend same section below `# Changelog` heading, above previous version.

Format rules (apply to both files):
- `## What's New in vX.Y.Z` as the top-level heading
- `### Subsection` for each feature or fix group
- Bullet points for details
- `---` horizontal rule separating from previous version

### 4. Build validation

Run both and confirm they succeed with no errors:
```powershell
cargo check --manifest-path src-tauri/Cargo.toml
npm run build
```

### 5. Commit, branch, and PR

> **CRITICAL**: Branch protection on `main` requires the "GlassWorm Infection Audit" status check to pass. Direct pushes to `main` will be rejected.

> **CRITICAL**: The pre-commit hook at `.githooks/pre-commit` is a bash script and will hang in PowerShell. Always use `git -c core.hooksPath=/dev/null` for all git commands.

1. **Create a release branch and commit:**
   ```powershell
   git checkout -b release/vX.Y.Z
   git add -A
   git -c core.hooksPath=/dev/null commit -m "vX.Y.Z: Short summary of major changes"
   git -c core.hooksPath=/dev/null push origin release/vX.Y.Z
   ```

2. **Create a PR** from `release/vX.Y.Z` → `main`:
   - Title: `vX.Y.Z: Short summary`
   - Body: bullet list of changes

3. **Wait for CI** — poll until "GlassWorm Infection Audit" shows `state: "success"`. Wait 30 seconds between polls. Timeout after 5 minutes.

4. **Triage bot review comments** — after CI passes, wait 60 seconds for automated reviewers, then assess each comment:
   - **Fix it**: correctness/safety bugs, consistency violations
   - **Skip**: premature abstractions, stylistic nits, factually wrong suggestions
   - Apply only fix-worthy changes, commit and push if changes were made, re-wait for CI

5. **Merge the PR** with `merge_method: "squash"`.

6. **Sync local main:**
   ```powershell
   git checkout main
   git fetch origin main
   git reset --hard origin/main
   ```

### 6. Tag and push

> **CRITICAL**: Tag protection rules prevent deleting or force-updating tags. Only push the tag after the PR is merged and main is synced.

```powershell
git tag vX.Y.Z
git -c core.hooksPath=/dev/null push origin vX.Y.Z
```

### 7. Verify CI started

Confirm the release workflow started at `https://github.com/Mooshieblob1/MooshieUI/actions`.

### 8. Fallback: workflow_dispatch

If the tag push is rejected or the tag-triggered workflow fails, use **workflow_dispatch** as fallback:
```powershell
gh workflow run release.yml -f tag="vX.Y.Z"
```

### 9. Clean up

Delete the release branch (remote only):
```powershell
git -c core.hooksPath=/dev/null push origin --delete release/vX.Y.Z
```

## How the About section works

No manual edit needed. The About section in Settings auto-populates:
- **Version**: `v{appVersion}` from `package.json` → Vite define → `__APP_VERSION__`
- **Release notes**: Fetched at runtime via `fetchReleaseNotes()` from GitHub Releases API

## Common mistakes to avoid

1. **Forgetting one of the 3 version files** — always grep to verify all three match
2. **Not running cargo check** — Cargo.lock won't update and CI build will fail
3. **Pushing directly to main** — branch protection will reject it; always use a PR
4. **Using git without `-c core.hooksPath=/dev/null`** — the bash pre-commit hook hangs in PowerShell
5. **Pushing a tag before the PR is merged** — tag must point at the final merge commit on main
6. **Trying to force-update or delete a tag** — tag protection prevents this; use workflow_dispatch instead
