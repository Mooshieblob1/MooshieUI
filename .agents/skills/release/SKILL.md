---
name: release
description: >-
  Cuts a MooshieUI semver release — bumps three version files, updates changelog
  and release notes, validates build, opens release PR, merges, tags, and triggers
  Build & Release CI. Use when the user says /release, cut a release, ship vX.Y.Z,
  or version bump.
---

# Release (MooshieUI)

Full release path: **repo hygiene → pre-commit → version bump → changelog → build validation → PR → GlassWorm → bot triage (all relevant PRs) → merge → tag → CI**.

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

### 1. Repo hygiene — branches and open PRs [BLOCKING before new release branch]

Refresh remotes, inventory branches/PRs, and resolve conflicts **before** creating `release/vX.Y.Z`.

```powershell
git fetch --prune origin
git branch -vv
git branch -r
gh pr list --state open --base main --json number,title,headRefName,updatedAt,isDraft
gh api repos/Mooshieblob1/MooshieUI/branches?per_page=100 --jq '.[].name'
```

**Release / stale branch handling**

| Situation | Action |
|-----------|--------|
| Remote `release/vX.Y.Z` already exists for **this** release | If PR is merged or abandoned: delete remote branch. If PR is still open and current: check out and continue on it (do not create a duplicate). |
| Remote `release/v*` for an **older** version, PR merged | Delete remote branch: `git -c core.hooksPath=/dev/null push origin --delete release/vOLD` |
| Remote `release/v*` with **closed** PR and no further work | Delete remote branch |
| Local tracking branches for deleted remotes | `git fetch --prune` then delete stale locals |
| Open PR targeting `main` that is superseded by this release | Close with a short comment, or merge first if it must ship in this release |
| Open PR with failing **GlassWorm** or blocking CI | Fix on that branch/PR or close before cutting the release |

**Do not** force-push `main` or protected tags. Do not delete branches that still have unmerged work the user intends to ship.

Report a short table: branch/PR, status, action taken.

### 2. Bot review sweep — open PRs and recent merges

Before version bump, triage bot feedback on PRs that could block or duplicate this release.

**Bots to read** (non-exhaustive): `gemini-code-assist[bot]`, `copilot-pull-request-reviewer[bot]`, `github-actions[bot]` (only when comment contains a concrete fix).

```powershell
# Open PRs targeting main
gh pr list --state open --base main --json number,title,headRefName

# Per PR (replace N):
gh pr view N --json reviews,comments,state,mergedAt
gh api repos/Mooshieblob1/MooshieUI/pulls/N/comments
gh api repos/Mooshieblob1/MooshieUI/pulls/N/reviews
```

Also check **recently merged** PRs since the last `v*` tag (bots sometimes comment late or on squash-merge commits):

```powershell
$lastTag = git describe --tags --abbrev=0
gh pr list --state merged --base main --limit 20 --json number,title,mergedAt
```

Classify every bot comment using [docs/BOT_REVIEW_TRIAGE.md](../../../docs/BOT_REVIEW_TRIAGE.md):

| Verdict | Action |
|---------|--------|
| **Fix** | Correctness, safety, MIME mismatches, consistency — fix on that PR's branch if still open, or on `main`/release branch if merged and release-blocking |
| **Skip** | Premature abstraction, nits, factually wrong bot claims — note rationale in release PR body or user summary |
| **Defer** | Valid but not release-blocking — document for follow-up; do not block tag |

**Release-blocking** bot findings on open PRs must be resolved (fix or explicit skip with verification) before creating the release PR. If a fix belongs on another open PR, land it there first or cherry-pick into the release branch.

### 3. Pre-commit-check

Run **pre-commit-check** on current tree. Fix blockers; re-run until ✅.

### 4. Version bump (all three must match)

| File | Field |
|------|-------|
| `package.json` | `"version": "X.Y.Z"` |
| `src-tauri/Cargo.toml` | `version = "X.Y.Z"` under `[package]` |
| `src-tauri/tauri.conf.json` | `"version": "X.Y.Z"` |

Verify:

```powershell
Select-String -Pattern '"X.Y.Z"|version = "X.Y.Z"' package.json, src-tauri/Cargo.toml, src-tauri/tauri.conf.json
```

### 5. Changelog files

Prepend to **both** `RELEASE_NOTES.md` and `CHANGELOG.md`:

```markdown
## What's New in vX.Y.Z

### Group title
- Detail

---

## What's New in vPREVIOUS
```

`CHANGELOG.md`: new section goes directly under `# Changelog`.

### 6. Build validation [BLOCKING]

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features server
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
```

### 7. Release branch and PR

Confirm step 1: no conflicting `release/vX.Y.Z` on remote (unless intentionally continuing that PR).

```powershell
git checkout main
git pull origin main
git checkout -b release/vX.Y.Z
git add -A
git -c core.hooksPath=/dev/null commit -m "vX.Y.Z: Short summary"
git -c core.hooksPath=/dev/null push -u origin release/vX.Y.Z
gh pr create --base main --head release/vX.Y.Z --title "vX.Y.Z: Short summary" --body "<bullet list of changes; note any bot triage from steps 1-2>"
```

### 8. CI

```powershell
gh pr checks <PR_NUMBER> --watch --interval 30
```

Wait for **GlassWorm Infection Audit** SUCCESS (5 min timeout).

### 9. Bot review triage — release PR (and late comments)

Wait ~60s after CI green. Re-fetch **all** bot threads on the release PR:

```powershell
gh pr view <PR_NUMBER> --json reviews,comments,state
gh api repos/Mooshieblob1/MooshieUI/pulls/<PR_NUMBER>/comments
gh api repos/Mooshieblob1/MooshieUI/pulls/<PR_NUMBER>/reviews
```

Repeat classification (Fix / Skip / Defer). Present a summary table: bot, file, verdict, one-line rationale.

Apply fixes only for **Fix**; then:

```powershell
git add -A
git -c core.hooksPath=/dev/null commit -m "address bot review feedback"
git -c core.hooksPath=/dev/null push
gh pr checks <PR_NUMBER> --watch --interval 30
```

If bots post **after** a green run (common on squash-merge), re-poll comments once more before merge. Do not merge with unaddressed **Fix** items on the release PR.

### 10. Merge and sync main

```powershell
gh pr merge <PR_NUMBER> --squash --delete-branch
git checkout main
git fetch origin main
git reset --hard origin/main
```

### 11. Post-merge bot pass (before tag)

On the merged release PR, fetch comments again in case bots reviewed the squash commit:

```powershell
gh pr view <PR_NUMBER> --json comments,reviews,mergedAt
gh api repos/Mooshieblob1/MooshieUI/pulls/<PR_NUMBER>/comments
```

- **Fix** on `main`: commit directly on `main` only if policy allows; otherwise open a tiny follow-up PR. For release workflow, prefer a fast follow-up commit on `main` before tagging when the fix is trivial and release-blocking.
- If a **Fix** requires re-tagging content, complete the fix, then tag (never move/delete an existing tag).

### 12. Tag (after merge only)

Tags are protected — no force/delete. Tag must point at merged `main`:

```powershell
git tag vX.Y.Z
git -c core.hooksPath=/dev/null push origin vX.Y.Z
```

`v*` tag triggers **Build & Release** ([`.github/workflows/release.yml`](../../../.github/workflows/release.yml)).

### 13. Verify CI

```powershell
gh run list --workflow=release.yml --limit 3
```

Tell user: https://github.com/Mooshieblob1/MooshieUI/actions

### 14. Fallback

If tag push fails or workflow does not start:

```powershell
gh workflow run release.yml -f tag=vX.Y.Z
```

### 15. Cleanup

```powershell
git fetch --prune origin
git -c core.hooksPath=/dev/null push origin --delete release/vX.Y.Z
```

Confirm no duplicate `release/v*` branches remain for this version. Prune stale local branches.

### 16. Wiki pass [user-facing changes only]

The wiki is a **separate git repo** (`https://github.com/Mooshieblob1/MooshieUI.wiki.git`, default branch `master`, pages are top-level `*.md`, nav is `_Sidebar.md`). It documents user-facing behavior only — skip this step if the release is purely internal (CI, refactors, deps) with no new/changed/removed user-facing feature or setting.

For every user-facing item in this release's `## What's New in vX.Y.Z` block:

1. Clone the wiki **outside** the main repo (a temp/scratch dir), never nest it inside the working tree:
   ```powershell
   git clone https://github.com/Mooshieblob1/MooshieUI.wiki.git <scratch>/MooshieUI.wiki
   ```
2. Find the page(s) the feature belongs on (`Upscaling-and-Face-Fix`, `Models-and-the-Model-Hub`, `Prompt-Assistant`, `Settings-and-Accessibility`, `Prompting-Guide`, `Generation-Basics`, `FAQ`, ...). A brand-new area may need a new page plus a `_Sidebar.md` entry.
3. **Verify exact UI labels against current code before writing** — features get renamed between changelog and ship. Grep `src/lib/locales/en.ts` for the label string and confirm the component. Do not copy changelog prose verbatim; match the wiki's concise, present-tense voice.
4. Fix anything the release makes stale (counts, removed toggles, renamed settings).
5. Commit and push to the wiki `master` (no `Co-Authored-By`, no em dashes):
   ```powershell
   git -c core.hooksPath=/dev/null commit -am "Document vX.Y.Z: <short summary>"
   git -c core.hooksPath=/dev/null push origin master
   ```

Report a short list of wiki pages touched (or "no user-facing changes — wiki pass skipped").

## About UI

No manual About edit — version from `__APP_VERSION__`; release notes from GitHub Releases API at runtime.

## Checklist

```
- [ ] Stale/existing branches and open PRs reviewed; release/* cleaned up or continued intentionally
- [ ] Bot comments triaged on open PRs + recent merges (Fix/Skip/Defer documented)
- [ ] Pre-commit-check passed
- [ ] Three version files match X.Y.Z
- [ ] RELEASE_NOTES.md + CHANGELOG.md updated
- [ ] cargo check + npm run build passed
- [ ] Release PR merged to main
- [ ] Post-merge bot pass completed (no unaddressed Fix items)
- [ ] Tag vX.Y.Z pushed
- [ ] Release workflow running
- [ ] Remote release/vX.Y.Z branch deleted
- [ ] Wiki pass done for user-facing changes (or explicitly skipped as internal-only)
```

## Mistakes to avoid

1. Missing one of the three version files
2. Skipping `cargo check` (stale `Cargo.lock` breaks CI)
3. Tag before PR merge
4. `git` without `core.hooksPath=/dev/null` on Windows
5. Force-updating tags — use `workflow_dispatch` instead
6. Creating a second `release/vX.Y.Z` while an open PR already exists for the same version
7. Merging the release PR while another open PR has release-blocking bot **Fix** items still unresolved
8. Ignoring late bot comments posted after CI goes green
9. Adding `Co-Authored-By` trailers to commits, PR bodies, or comments — never attribute AI assistance in any git or GitHub output
10. Shipping user-facing features without a wiki pass, or documenting a changelog label that no longer matches the shipped UI string (verify against `en.ts`)
