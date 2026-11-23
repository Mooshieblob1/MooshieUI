---
name: quickrelease
description: >-
  Cuts a MooshieUI semver release quickly, skipping bot reviews, pre-commit-checks,
  and local build validation. Bumps the version files and lockfiles, prepends
  release notes, opens a release PR, merges it once the required checks and the
  dependency audit pass, tags the merge commit, and checks the release builds. Use
  when the user says /quickrelease or wants a checkless release.
---

# Quick Release (MooshieUI)

Only run when the user explicitly asks to cut a release; do not bump versions, open release PRs, or push tags unprompted.

Fast path: **sync main → version bump → notes → release PR → required checks + dependency audit → merge → tag merge commit → check the release run**.

No local builds, lint or bot triage. The release PR's own CI covers the cheap gates; the tagged Build & Release run rebuilds everything. Use it when the code on `main` is already verified. When it isn't, or when the last release failed, use **release**.

## Inputs

| Field | Default if omitted |
|-------|-------------------|
| Version `X.Y.Z` | Read `package.json`, patch+1 (no `v` prefix in files) |
| Summary | `git log` since the last `v*` tag |

## Windows git (required)

```text
git -c core.hooksPath=/dev/null ...
```

The pre-commit hook is bash and hangs in PowerShell without this.

## Cloud sessions

Pushing a `v*` tag returns 403 from a cloud session. In the cloud, do steps 1 to 6 with the GitHub tools, then stop and give the user the one-line tag command from step 7.

## Workflow

### 1. Sync main and look before you cut

```powershell
git checkout main
git pull origin main
git fetch --tags origin
gh pr list --state open --base main --json number,title,isDraft
gh run list --workflow macos-native.yml --branch main --limit 1
```

- If a PR the user expects in this release is still open or in CI, wait for it to merge. Otherwise it misses the release.
- If tag `vX.Y.Z` already exists, use the next patch (tags are protected).
- If the latest Apple Silicon run for `main` code failed, stop and use **release**: Build & Release won't publish without a passing Mac build.

### 2. Version bump

```powershell
git checkout -b release/vX.Y.Z
npm version X.Y.Z --no-git-tag-version --ignore-scripts   # package.json + package-lock.json
```

Edit `src-tauri/Cargo.toml` (`version = "X.Y.Z"` under `[package]`) and `src-tauri/tauri.conf.json` (`"version": "X.Y.Z"`). Then refresh `Cargo.lock` (CI's Rust jobs run `--locked`) and verify with the script the release build runs:

```powershell
cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1 > $null
node scripts/verify-release-version.mjs vX.Y.Z
```

Expect exactly five changed files: `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json`.

### 3. Release notes

Prepend the same section to **both** `docs/RELEASE_NOTES.md` (top of file) and `CHANGELOG.md` (directly under `# Changelog`):

```markdown
## What's New in vX.Y.Z

### Fixes and maintenance
- **Thing**: what changed, in user terms.

---

## What's New in vPREVIOUS
```

The release body is cut from this file between `## What's New in vX.Y.Z` and the first `---`, so the heading must match exactly and the section must not contain a `---` line.

### 4. Commit, push, open the PR

```powershell
git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json docs/RELEASE_NOTES.md CHANGELOG.md
git -c core.hooksPath=/dev/null commit -m "vX.Y.Z: Quick release"
git -c core.hooksPath=/dev/null push -u origin release/vX.Y.Z
gh pr create --base main --head release/vX.Y.Z --title "vX.Y.Z: Quick release" --body "<bullet list of changes>"
```

### 5. Wait for the required checks and the dependency audit

```powershell
gh pr checks <PR_NUMBER> --watch --required --interval 15
gh pr checks <PR_NUMBER>
```

The first command waits for GlassWorm Infection Audit and Guardrails (about one minute). Then look once at the full list:

- **Dependency audit (npm + cargo)** must have passed. It's the same `scripts/audit-deps.sh` that blocks the tagged build, so a new advisory fails here in a minute instead of 15 minutes into Build & Release. If it fails, fix the crate on a normal PR first.
- Anything already **failing** (Rust, Apple Silicon): stop. Don't tag a build that will fail.
- Rust and Apple Silicon still **pending** is fine; the tagged build runs them again.

### 6. Merge

```powershell
gh pr merge <PR_NUMBER> --squash --delete-branch
```

If it's refused because the branch is behind `main` (`main` requires up-to-date branches): `gh pr update-branch <PR_NUMBER>`, add what merged meanwhile to the notes if it's user-facing, and repeat step 5. `--admin` doesn't help; admins are included in branch protection.

### 7. Tag the merge commit

```powershell
$sha = gh pr view <PR_NUMBER> --json mergeCommit --jq .mergeCommit.oid
git fetch origin main
git tag vX.Y.Z $sha
git -c core.hooksPath=/dev/null push origin vX.Y.Z
```

This starts **Build & Release**. Only if no run started within a minute: `gh workflow run release.yml -f tag=vX.Y.Z`.

### 8. Check the release run

```powershell
gh run list --workflow release.yml --branch vX.Y.Z --limit 1
```

The run takes about 35 to 50 minutes and publishes the GitHub release only after the Windows, Linux and Mac builds and signing pass. Tell the user the run link. If you stay for it (`gh run watch <id>`):

- A build or signing job failed: nothing is published. Re-run failed jobs if it was flaky (`gh run rerun <id> --failed`); otherwise fix on a normal PR and release the next patch.
- Once published, the release is immutable. Never re-run it to add or replace assets; ship the next patch.
- `linux-repo`, AUR or Docker failures don't block the release, but report them.

### 9. Cleanup

```powershell
git checkout main
git pull origin main
git fetch --prune origin
git branch -D release/vX.Y.Z
```

### 10. Wiki pass [user-facing changes only]

Quick releases are usually fixes that need no docs; skip this then. If this release adds, renames, or removes a user-facing feature or setting, update the GitHub wiki (a **separate repo**: `https://github.com/Mooshieblob1/MooshieUI.wiki.git`, branch `master`, top-level `*.md`, nav `_Sidebar.md`). mooshieui.com/wiki mirrors it every 6 hours.

```powershell
git clone -c core.autocrlf=false https://github.com/Mooshieblob1/MooshieUI.wiki.git <scratch>/MooshieUI.wiki   # outside the main repo
```

Edit the page(s) for each user-facing item in the new section, **verifying exact UI labels against current code** (`src/lib/locales/en.ts` + the component). Match the wiki's concise, present-tense voice. Then:

```powershell
git -c core.hooksPath=/dev/null commit -am "Document vX.Y.Z: <short summary>"
git -c core.hooksPath=/dev/null push origin master
```

No `Co-Authored-By`, no em dashes. Report the pages touched, or that the pass was skipped.

## Checklist

```
- [ ] Expected PRs merged; version unused; main's Apple Silicon run not failing
- [ ] Five version files at X.Y.Z; verify-release-version.mjs passes
- [ ] docs/RELEASE_NOTES.md + CHANGELOG.md have "## What's New in vX.Y.Z"
- [ ] Required checks + Dependency audit green, nothing failing
- [ ] Release PR merged; tag vX.Y.Z on its merge commit pushed
- [ ] Build & Release running (link given to the user)
- [ ] Wiki pass done for user-facing changes (or skipped)
```

## Mistakes to avoid

1. Adding `Co-Authored-By` trailers to commits, PR bodies, or comments; never attribute AI assistance in git or GitHub output
2. Leaving `Cargo.lock` at the old version (`--locked` CI jobs fail)
3. Tagging while any check on the release PR is failing
4. Tagging local `main` instead of the release PR's merge commit
5. `gh pr merge --admin` (admins are enforced)
6. A notes heading that isn't exactly `## What's New in vX.Y.Z`
7. `git` without `core.hooksPath=/dev/null` on Windows
8. Force-updating tags or re-running a published release; use the next patch
