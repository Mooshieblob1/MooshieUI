---
name: release
description: Cut a new MooshieUI release — bump versions, update release notes, validate, tag, and push
argument-hint: "Version number (e.g. 0.4.3) and a brief summary of changes"
agent: agent
---

Cut a new MooshieUI release. Execute every step autonomously — do NOT pause for confirmation between steps. Use the todo list to track progress.

## Required Information

If NOT provided in the user's message, ask for:
1. **Version number** (e.g. `0.4.3`) — must be semver, no `v` prefix
2. **Summary of changes** — what features/fixes to include in release notes

If the user says something like "release it" or "cut a release" without a version, read the current version from `package.json` and auto-increment the patch number. If no summary is given, derive it from `git log` since the last tag.

## Execution Plan

Create a todo list with these items and work through them sequentially:

### 1. Run pre-commit-check agent

Invoke the `pre-commit-check` agent before anything else. If it reports failures, fix them before continuing.

### 2. Bump version in 3 files

All three must have the **exact same version string**:

- **`package.json`** → `"version": "X.Y.Z"`
- **`src-tauri/Cargo.toml`** → `version = "X.Y.Z"` (under `[package]`)
- **`src-tauri/tauri.conf.json`** → `"version": "X.Y.Z"`

After bumping, run `Select-String -Pattern "X.Y.Z" package.json, src-tauri/Cargo.toml, src-tauri/tauri.conf.json` to verify all three match.

> `Cargo.lock` updates automatically on next `cargo check`.

### 3. Update RELEASE_NOTES.md and CHANGELOG.md

**RELEASE_NOTES.md** — prepend a new section **above** the existing content:

```markdown
## What's New in vX.Y.Z

### Feature/Fix Title
- Description

---

## What's New in vPREVIOUS
(existing content below)
```

**CHANGELOG.md** — prepend the same new section **below** the `# Changelog` heading and **above** the previous version:

```markdown
# Changelog

## What's New in vX.Y.Z

### Feature/Fix Title
- Description

---

## What's New in vPREVIOUS
(existing content below)
```

Format rules (apply to both files):
- `## What's New in vX.Y.Z` as the top-level heading
- `### Subsection` for each feature or fix group
- Bullet points for details
- `---` horizontal rule separating from the previous version

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

2. **Create a PR** from `release/vX.Y.Z` → `main` using `mcp_github_create_pull_request`:
   - Title: `vX.Y.Z: Short summary`
   - Body: bullet list of changes

3. **Wait for CI** — poll `github-pull-request_pullRequestStatusChecks` until the "GlassWorm Infection Audit" check shows `state: "success"`. Wait 30 seconds between polls. Timeout after 5 minutes.

4. **Triage and address bot review comments** — after CI passes, wait 60 seconds for automated reviewers (`gemini-code-assist[bot]`, `copilot-pull-request-reviewer[bot]`) to post, then fetch all review comments with `mcp_github_pull_request_read` (`method: "get_review_comments"`).

   **Assessment — do NOT blindly trust bot suggestions.** For each comment, classify it:

   | Category | Action | Examples |
   |----------|--------|---------|
   | **Correctness / Safety** | Fix it | Error propagation bugs, MIME type mismatches, missing integrity checks, silent data loss |
   | **Consistency** | Fix it | Title-case inconsistency in user-visible strings, naming convention violations |
   | **Defensive hardening** | Fix if trivial | `.trim().is_empty()` instead of `.is_empty()`, edge-case guards |
   | **Premature abstraction** | Skip | "Centralize X into a shared config" when only 2 call-sites exist |
   | **Stylistic / informational** | Skip | Praise, nitpicks, suggestions that don't improve correctness or safety |
   | **Factually wrong** | Skip and note why | Bot misunderstands the API contract, browser Clipboard API constraints, etc. |

   **Workflow:**
   - Read each comment, read the referenced file to verify the bot's claim is accurate
   - Present a summary table to the user: comment, file, verdict (fix / skip), and one-line rationale
   - Apply only the fixes classified as Fix
   - If any fixes were made, commit and push:
     ```powershell
     git add -A
     git -c core.hooksPath=/dev/null commit -m "address bot review feedback"
     git -c core.hooksPath=/dev/null push origin release/vX.Y.Z
     ```
   - Wait for CI to pass again (same polling as step 3) before proceeding to merge
   - If there are no actionable comments, proceed directly to merge

5. **Merge the PR** using `mcp_github_merge_pull_request` with `merge_method: "squash"`.

6. **Sync local main:**
   ```powershell
   git checkout main
   git fetch origin main
   git reset --hard origin/main
   ```

### 7. Tag and push

> **CRITICAL**: Tag protection rules prevent deleting or force-updating tags once pushed. Only push the tag after the PR is merged and main is synced.

```powershell
git tag vX.Y.Z
git -c core.hooksPath=/dev/null push origin vX.Y.Z
```

The `v*` tag triggers the **Build & Release** GitHub Actions workflow which:
1. Builds Linux (`.deb`, `.AppImage`) and Windows (`.exe`) installers
2. Generates `latest.json` updater manifest with signatures
3. Creates a **GitHub Release** with download table + only the current version's section from `RELEASE_NOTES.md`

### 8. Verify CI started

After pushing the tag, confirm the release workflow started by checking with the GitHub API or telling the user to check `https://github.com/Mooshieblob1/MooshieUI/actions`.

### 9. Fallback: workflow_dispatch

If the tag push is rejected (e.g. tag already exists due to a previous attempt), or if the tag-triggered workflow fails, use **workflow_dispatch** as a fallback:

```powershell
# Extract git credential token
$cred = "protocol=https`nhost=github.com" | git credential fill 2>$null
$token = ($cred | Select-String "password=").Line -replace "password=",""
$headers = @{ Authorization = "Bearer $token"; Accept = "application/vnd.github+json" }
$body = @{ ref = "main"; inputs = @{ tag = "vX.Y.Z" } } | ConvertTo-Json
Invoke-RestMethod -Uri "https://api.github.com/repos/Mooshieblob1/MooshieUI/actions/workflows/release.yml/dispatches" -Method POST -Headers $headers -Body $body -ContentType "application/json"
```

This triggers the same Build & Release workflow from the current `main` HEAD.

### 10. Clean up

Delete the release branch (remote only):
```powershell
git -c core.hooksPath=/dev/null push origin --delete release/vX.Y.Z
```

## How the About section works

No manual edit needed. The About section in Settings auto-populates:
- **Version**: `v{appVersion}` where `appVersion` comes from `package.json` → Vite define → `__APP_VERSION__`
- **Release notes**: Fetched at runtime from the GitHub Releases API via `fetchReleaseNotes()` in `SettingsPage.svelte`

## Common mistakes to avoid

1. **Forgetting one of the 3 version files** — always grep to verify all three match
2. **Not running cargo check** — the Cargo.lock won't update and the build will fail in CI
3. **Pushing directly to main** — branch protection will reject it; always use a PR
4. **Using git without `-c core.hooksPath=/dev/null`** — the bash pre-commit hook hangs in PowerShell
5. **Pushing a tag before the PR is merged** — the tag must point at the final merge commit on main
6. **Trying to force-update or delete a tag** — tag protection rules prevent this; use workflow_dispatch instead
7. **Tag before commit** — the tag must point at the release commit, not the previous one
