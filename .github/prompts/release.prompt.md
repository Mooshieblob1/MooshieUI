---
name: release
description: Cut a new MooshieUI release: bump versions, update release notes, gate on CI, tag, and watch the release build
argument-hint: "Version number (e.g. 0.4.3) and a brief summary of changes"
agent: agent
---

# Release (MooshieUI)

Full release path: **preflight → version bump → notes → release PR → full CI gate → merge → tag merge commit → watch Build & Release → wiki pass**.

The release PR's CI is the build validation. It touches `package.json` and `src-tauri/Cargo.toml`, so it runs every check: Guardrails (`npm run build`, i18n, types), Rust desktop (fmt, clippy, `cargo test --locked`), Rust server, Dependency audit (`scripts/audit-deps.sh`) and the Apple Silicon build. Don't repeat those locally; wait for them.

Use **quickrelease** when the code on `main` is already verified and you only need the bump, notes and tag.

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

Pushing a `v*` tag returns 403 from a cloud session, and `gh` may be unavailable there. In the cloud, do steps 1 to 7 with the GitHub tools, then stop and give the user the one-line tag command from step 8 (tag the merge SHA, push it). Don't look for another route.

## Workflow

### 1. Preflight [BLOCKING]

```powershell
git fetch --prune --tags origin
git describe --tags --abbrev=0 origin/main          # last release tag
gh pr list --state open --base main --json number,title,headRefName,isDraft
gh run list --workflow macos-native.yml --branch main --limit 1
gh run list --workflow release.yml --limit 1
```

- **Open PRs**: if one the user expects in this release is still open or in CI, merge it first (or ask). A release cut while a PR is mid-CI ships without it.
- **Existing `release/vX.Y.Z`** on the remote: continue that PR if it is open and current; never open a second one for the same version.
- **Version already used**: if tag `vX.Y.Z` exists (even with no published release, as with v2.3.50), pick the next patch. Tags are protected and can't be moved or deleted.
- **Last Build & Release run failed?** Find out why before cutting another (see step 9).
- **Bot review comments**: review bots don't currently comment on this repo. If any PR merged since the last tag does have bot comments (`gh api repos/Mooshieblob1/MooshieUI/pulls/N/comments`), triage them with [docs/BOT_REVIEW_TRIAGE.md](../../../docs/BOT_REVIEW_TRIAGE.md) (Fix / Skip / Defer). Otherwise skip this.

### 2. Uncommitted code changes

A release cut from clean `main` has none, so skip this. If the user wants uncommitted changes in the release, land them through a normal PR first (**push** skill, which runs **pre-commit-check**), then come back.

### 3. Version bump

```powershell
git checkout main
git pull origin main
git checkout -b release/vX.Y.Z
npm version X.Y.Z --no-git-tag-version --ignore-scripts   # package.json + package-lock.json
```

Then edit by hand:

| File | Field |
|------|-------|
| `src-tauri/Cargo.toml` | `version = "X.Y.Z"` under `[package]` |
| `src-tauri/tauri.conf.json` | `"version": "X.Y.Z"` |

Refresh `Cargo.lock` (CI's Rust jobs run `--locked` and fail on a stale lock), then verify with the same script the release build runs:

```powershell
cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1 > $null
git diff --stat   # expect package.json, package-lock.json, Cargo.toml, Cargo.lock, tauri.conf.json
node scripts/verify-release-version.mjs vX.Y.Z
```

`cargo metadata` only rewrites the `comfyui-desktop` entry in `Cargo.lock`. If the diff shows other crates changing, revert the lock and investigate.

### 4. Release notes

Prepend the same section to **both** `docs/RELEASE_NOTES.md` (top of file) and `CHANGELOG.md` (directly under `# Changelog`):

```markdown
## What's New in vX.Y.Z

### Group title
- **Feature name**: what changed, in user terms.

---

## What's New in vPREVIOUS
```

Build & Release copies this section into the GitHub release body with `sed`, from the `## What's New in vX.Y.Z` line to the first `---` line. So:

- The heading must be exactly `## What's New in vX.Y.Z` (with the `v`), or the release body has no notes.
- Never put a `---` line inside the section; it ends the extract early.
- Use the shipped UI labels (grep `src/lib/locales/en.ts`), not branch names or PR titles.

### 5. Commit, push, open the PR

```powershell
git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json docs/RELEASE_NOTES.md CHANGELOG.md
git -c core.hooksPath=/dev/null commit -m "vX.Y.Z: Short summary"
git -c core.hooksPath=/dev/null push -u origin release/vX.Y.Z
gh pr create --base main --head release/vX.Y.Z --title "vX.Y.Z: Short summary" --body "<bullet list of changes, with PR numbers>"
```

### 6. Full CI gate [BLOCKING]

```powershell
gh pr checks <PR_NUMBER> --watch --interval 30
```

Wait for **every** check, including **Apple Silicon build and runtime** (about 25 to 30 minutes). That job is not required by branch protection, but Build & Release only publishes when the Mac build passes, and a tag whose build fails burns the version (v2.3.50: its release PR's Apple Silicon check was red, it was tagged anyway, and it never published).

- Any failure: fix it on a normal PR, merge that, then update this branch. Don't merge the release PR red.
- **Branch behind main** (merge refused with "required status checks are expected", or `mergeStateStatus` is `BEHIND`): `main` requires up-to-date branches. Run `gh pr update-branch <PR_NUMBER>`, add whatever merged meanwhile to the notes, and wait for the checks again.

### 7. Merge

```powershell
gh pr merge <PR_NUMBER> --squash --delete-branch
```

`--admin` does not help: admins are included in branch protection. The repo doesn't delete merged branches on its own, so keep `--delete-branch`.

### 8. Tag the merge commit

Tag the release PR's merge commit, not local `main` (another PR may have merged right after it, and its changes aren't in the notes):

```powershell
$sha = gh pr view <PR_NUMBER> --json mergeCommit --jq .mergeCommit.oid
git fetch origin main
git tag vX.Y.Z $sha
git -c core.hooksPath=/dev/null push origin vX.Y.Z
```

The `v*` tag starts **Build & Release** ([`.github/workflows/release.yml`](../../../.github/workflows/release.yml)). Only if the push succeeded but no run started within a minute:

```powershell
gh workflow run release.yml -f tag=vX.Y.Z
```

### 9. Watch Build & Release until the release is published

```powershell
$run = gh run list --workflow release.yml --branch vX.Y.Z --limit 1 --json databaseId --jq '.[0].databaseId'
gh run watch $run --interval 60
gh release view vX.Y.Z --json url,assets --jq '{url, assets: [.assets[].name]}'
```

A run takes about 35 to 50 minutes. The GitHub release is created only after the Windows, Linux and Mac builds and updater signing pass. Then:

| What failed | Release published? | Do this |
|-------------|--------------------|---------|
| `build`, `build-macos` or `sign-updaters` | No | Flaky (runner, network): `gh run rerun $run --failed`. Code problem: fix on a normal PR, then release the next patch. |
| `release` (Create or update GitHub Release) after it was already published | Yes, and immutable | Never re-run to add or replace assets (v2.3.48 and v2.3.49 re-runs failed this way). Ship the next patch instead. |
| `linux-repo`, `publish-aur`, Docker or server jobs | Yes | Doesn't block the release. Report it (for example a `REPO_GPG_PRIVATE_KEY` passphrase error) so it gets fixed before the next one. |

Check the release body contains this version's notes (if it shows only the installer blurb, the heading didn't match).

### 10. Cleanup

```powershell
git checkout main
git pull origin main
git fetch --prune origin
git branch -D release/vX.Y.Z
```

`--delete-branch` already removed the remote branch. Stale feature branches are the **cleanup** skill's job.

### 11. Wiki pass [user-facing changes only]

Skip this if the release is purely internal (CI, refactors, deps). Otherwise update the GitHub wiki, a **separate repo** (`https://github.com/Mooshieblob1/MooshieUI.wiki.git`, branch `master`, top-level `*.md`, nav `_Sidebar.md`). mooshieui.com/wiki mirrors it every 6 hours (the Sync wiki workflow in `Mooshieblob1/mooshieui-site`; run it by hand to publish sooner), so a page that only exists on the site is deleted at the next sync.

1. Clone **outside** the main repo: `git clone https://github.com/Mooshieblob1/MooshieUI.wiki.git <scratch>/MooshieUI.wiki`
2. Find the page for each user-facing item in the new `## What's New in vX.Y.Z` section (`Upscaling-and-Face-Fix`, `Models-and-the-Model-Hub`, `Prompt-Assistant`, `Settings-and-Accessibility`, `Prompting-Guide`, `Generation-Basics`, `FAQ`, ...). A new area may need a new page plus a `_Sidebar.md` entry.
3. **Verify exact UI labels against current code** (`src/lib/locales/en.ts` + the component). Match the wiki's concise, present-tense voice; don't paste changelog prose.
4. Fix anything the release makes stale (counts, removed toggles, renamed settings).
5. Commit and push (no `Co-Authored-By`, no em dashes):
   ```powershell
   git -c core.hooksPath=/dev/null commit -am "Document vX.Y.Z: <short summary>"
   git -c core.hooksPath=/dev/null push origin master
   ```

Report the pages touched, or that the pass was skipped as internal-only.

## About UI

No manual About edit: the version comes from `__APP_VERSION__` and release notes from the GitHub Releases API at runtime.

## Checklist

```
- [ ] Preflight: expected PRs merged, version unused, last release run understood
- [ ] package.json, package-lock.json, Cargo.toml, Cargo.lock, tauri.conf.json at X.Y.Z; verify-release-version.mjs passes
- [ ] docs/RELEASE_NOTES.md + CHANGELOG.md have "## What's New in vX.Y.Z"
- [ ] Every check on the release PR green, Apple Silicon included
- [ ] Release PR merged; tag vX.Y.Z on its merge commit pushed
- [ ] Build & Release finished; GitHub release published with notes and assets
- [ ] Local release branch deleted
- [ ] Wiki pass done for user-facing changes (or skipped as internal-only)
```

## Mistakes to avoid

1. Tagging while the release PR's Apple Silicon check is failing (the release never publishes, and the version is burned)
2. Leaving `Cargo.lock` at the old version (`--locked` CI jobs fail)
3. A notes heading that isn't exactly `## What's New in vX.Y.Z`, or a `---` inside the section
4. Tagging local `main` instead of the release PR's merge commit
5. Re-running a release job after the release published (immutable; ship the next patch)
6. `gh pr merge --admin` (admins are enforced; it can't bypass checks)
7. `git` without `core.hooksPath=/dev/null` on Windows
8. Force-updating or deleting tags (protected; use the next patch)
9. A second `release/vX.Y.Z` while one is already open
10. Adding `Co-Authored-By` trailers to commits, PR bodies, or comments; never attribute AI assistance in git or GitHub output
11. Shipping user-facing features without a wiki pass, or documenting a label that no longer matches `en.ts`
