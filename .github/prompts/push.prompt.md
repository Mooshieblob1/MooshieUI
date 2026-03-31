---
name: push
description: Commit and push changes to main via PR — no version bump, no release, no tag
argument-hint: "Brief commit message describing the changes"
agent: agent
---

Push the current working-tree changes to `main` via a pull request. This is for non-release changes — CI fixes, docs, config, prompts, minor tweaks, or accumulating small changes toward a future release. Execute every step autonomously — do NOT pause for confirmation.

## Required Information

If NOT provided in the user's message, derive a commit message from `git diff --stat` and the changed file contents. Keep it under 72 characters, imperative mood (e.g. "fix CI toolchain", "update release prompt", "add push workflow").

If the user provides a message, use it as-is.

## Execution Plan

### 1. Pre-flight checks

```powershell
git diff --stat
git diff --cached --stat
```

If the working tree is clean (no changes), tell the user and stop.

### 2. Run pre-commit-check agent

Invoke the `pre-commit-check` agent. If it reports failures, fix them before continuing.

### 3. Create branch, commit, and push

Pick a branch name from the commit message: `chore/<slugified-summary>` (lowercase, hyphens, max 50 chars).

> **CRITICAL**: The pre-commit hook at `.githooks/pre-commit` is a bash script and will hang in PowerShell. Always use `git -c core.hooksPath=/dev/null` for all git commands.

```powershell
git checkout -b chore/<slug>
git add -A
git -c core.hooksPath=/dev/null commit -m "<commit message>"
git -c core.hooksPath=/dev/null push origin chore/<slug>
```

### 4. Create a PR

Use `mcp_github_create_pull_request`:
- **Title**: the commit message
- **Body**: output of `git diff --stat` against `main`, or a brief description of what changed
- **Base**: `main`
- **Head**: `chore/<slug>`

### 5. Wait for CI

Poll `github-pull-request_pullRequestStatusChecks` until the "GlassWorm Infection Audit" check shows `state: "success"`. Wait 30 seconds between polls. Timeout after 5 minutes.

If GlassWorm fails, read the check details and attempt to fix. If unfixable, report to the user.

### 6. Merge the PR

Use `mcp_github_merge_pull_request` with `merge_method: "squash"`.

### 7. Sync local main

```powershell
git checkout main
git fetch origin main
git reset --hard origin/main
```

### 8. Clean up

Delete the remote branch:
```powershell
git -c core.hooksPath=/dev/null push origin --delete chore/<slug>
```

## What this does NOT do

- **No version bump** — `package.json`, `Cargo.toml`, `tauri.conf.json` are left untouched
- **No release notes** — `RELEASE_NOTES.md` and `CHANGELOG.md` are not modified
- **No tag** — no `vX.Y.Z` tag is created or pushed
- **No release workflow** — the Build & Release CI is not triggered
- **No build validation** — cargo check / npm build are skipped (use `/release` for that)

## Common mistakes to avoid

1. **Pushing directly to main** — branch protection will reject it; always use a PR
2. **Using git without `-c core.hooksPath=/dev/null`** — the bash pre-commit hook hangs in PowerShell
3. **Accidentally including version bumps** — if version files are modified, warn the user and suggest using `/release` instead
