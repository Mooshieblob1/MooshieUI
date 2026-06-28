---
name: quickrelease
description: Cut a fast MooshieUI release — skips local checks and bot triage (pre-verified changes only)
argument-hint: "Version number (e.g. 0.4.3) and a brief summary of changes"
agent: agent
---


# Quick Release (MooshieUI)

Fast-path release: **checkout main → version bump → changelog → push branch → PR → auto-merge → tag → CI**.

This workflow skips all developer hygiene checks, linting, pre-commit hooks, and local build verification. Use it only when the changes are trivial and already verified.

## Inputs

| Field | Default if omitted |
|-------|-------------------|
| Version `X.Y.Z` | Read `package.json`, patch+1 (no `v` prefix in files) |
| Summary | `git log` since last `v*` tag |

## Windows git (required)

```text
git -c core.hooksPath=/dev/null ...
```

Pre-commit hook hangs in PowerShell without this.

## Workflow

### 1. Sync main branch

Ensure you are on a fresh, updated `main` branch before bumping versions:

```powershell
git checkout main
git pull origin main
```

### 2. Version bump (all three must match)

Update version in:

| File | Field |
|------|-------|
| `package.json` | `"version": "X.Y.Z"` |
| `src-tauri/Cargo.toml` | `version = "X.Y.Z"` under `[package]` |
| `src-tauri/tauri.conf.json` | `"version": "X.Y.Z"` |

### 3. Changelog files

Prepend to **both** `RELEASE_NOTES.md` and `CHANGELOG.md`:

```markdown
## What's New in vX.Y.Z

### Fixes and maintenance
- Detail

---

## What's New in vPREVIOUS
```

`CHANGELOG.md`: new section goes directly under `# Changelog`.

### 4. Create and push release branch

```powershell
git checkout -b release/vX.Y.Z
git add -A
git -c core.hooksPath=/dev/null commit -m "vX.Y.Z: Quick release"
git -c core.hooksPath=/dev/null push -u origin release/vX.Y.Z
```

### 5. Create Pull Request

```powershell
gh pr create --base main --head release/vX.Y.Z --title "vX.Y.Z: Quick release" --body "<bullet list of changes>"
```

### 6. Merge PR

If branch checks are green or can be bypassed:
```powershell
gh pr merge <PR_NUMBER> --squash --delete-branch
```
If branch policy blocks the merge due to pending checks or admin settings, add the `--admin` flag:
```powershell
gh pr merge <PR_NUMBER> --squash --delete-branch --admin
```

### 7. Sync main & Tag

```powershell
git checkout main
git fetch origin main
git reset --hard origin/main
git tag vX.Y.Z
git -c core.hooksPath=/dev/null push origin vX.Y.Z
```

This triggers the **Build & Release** workflow on GitHub Actions.

### 8. Cleanup

```powershell
git fetch --prune origin
```

Prune any stale local branches.

## Checklist

```
- [ ] Stale release/* branches cleaned up
- [ ] Three version files match X.Y.Z
- [ ] RELEASE_NOTES.md + CHANGELOG.md updated
- [ ] Release PR merged to main
- [ ] Tag vX.Y.Z pushed
- [ ] Release workflow running on GitHub
```

## Mistakes to avoid

1. Adding `Co-Authored-By` trailers to commits, PR bodies, or comments — never attribute AI assistance in any git or GitHub output
2. Missing one of the three version files
3. `git` without `core.hooksPath=/dev/null` on Windows
4. Tag before PR merge
5. Force-updating tags — use `workflow_dispatch` instead
