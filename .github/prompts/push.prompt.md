---
name: push
description: Commit and push changes to main via PR — no version bump, no release, no tag
argument-hint: "Brief commit message describing the changes"
agent: agent
---


# Push to Main (MooshieUI)

Non-release path: **PR → GlassWorm CI → squash merge → sync main**. Run autonomously; do not pause for confirmation unless blocked.

**Does not:** bump version, edit `RELEASE_NOTES.md` / `CHANGELOG.md`, create tags, or run full release build CI.

## Inputs

- **Commit message:** user-provided, or derive from `git diff` (imperative, ≤72 chars, e.g. `fix CI toolchain`)
- **Branch:** `chore/<slug>` — lowercase, hyphens, ≤50 chars from message

## Windows git (required)

Pre-commit hook is bash and **hangs in PowerShell**. Prefix **every** git command:

```text
git -c core.hooksPath=/dev/null ...
```

## Workflow

### 1. Pre-flight

```powershell
cd "$(git rev-parse --show-toplevel)"
git diff --stat
git diff --cached --stat
```

Clean tree → tell user and stop.

If `package.json`, `src-tauri/Cargo.toml`, or `src-tauri/tauri.conf.json` show version-only bumps → warn user to use **release** skill instead.

### 2. Pre-commit-check

Follow the **pre-commit-check** skill. Fix blocking issues; re-run until ✅ Ready to commit.

### 3. Branch, commit, push

```powershell
git checkout -b chore/<slug>
git add -A
git -c core.hooksPath=/dev/null commit -m "<message>"
git -c core.hooksPath=/dev/null push -u origin chore/<slug>
```

### 4. Open PR

```powershell
gh pr create --base main --head chore/<slug> --title "<message>" --body "$(git diff --stat origin/main...HEAD)"
```

### 5. Wait for CI

Poll until **GlassWorm Infection Audit** is `SUCCESS` (30s interval, 5 min timeout):

```powershell
gh pr checks --watch --interval 30
```

On failure: `gh pr checks` / run logs → fix → push → re-poll.

### 6. Merge

```powershell
gh pr merge --squash --delete-branch
```

### 7. Sync local main

```powershell
git checkout main
git fetch origin main
git reset --hard origin/main
```

### 8. Cleanup

Remote branch is deleted by `--delete-branch` if merge succeeded. If not:

```powershell
git -c core.hooksPath=/dev/null push origin --delete chore/<slug>
```

## Checklist

```
- [ ] Pre-commit-check passed
- [ ] PR created chore/<slug> → main
- [ ] GlassWorm SUCCESS
- [ ] Squash merged
- [ ] Local main reset to origin/main
```

## Mistakes to avoid

1. Pushing directly to `main` (branch protection)
2. Omitting `core.hooksPath=/dev/null` on Windows
3. Including release version bumps — use **release** skill
4. Adding `Co-Authored-By` trailers to commits, PR bodies, or comments — never attribute AI assistance in any git or GitHub output
