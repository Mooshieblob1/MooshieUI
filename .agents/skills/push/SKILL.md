---
name: push
description: >-
  Commits MooshieUI working-tree changes and lands them on main via a squash-merged
  PR without version bump or release tag. Use when the user says /push, push to main,
  chore PR, or wants docs/CI/fixes merged without a release.
---

# Push to Main (MooshieUI)

Non-release path: **PR → GlassWorm + Guardrails CI → squash merge → sync main**. Run autonomously; do not pause for confirmation unless blocked.

**Does not:** bump version, edit `docs/RELEASE_NOTES.md` / `CHANGELOG.md`, create tags, or run full release build CI.

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

For feature changes, include the MCP decision in the PR body (or state `MCP: no change` if unaffected).

```powershell
gh pr create --base main --head chore/<slug> --title "<message>" --body "$(git diff --stat origin/main...HEAD)"
```

### 5. Wait for CI

Poll both required checks (**GlassWorm Infection Audit** and **Guardrails (build, i18n, types)**):

```powershell
gh pr checks --watch --required --interval 15
gh pr checks
```

On failure: `gh pr checks` / run logs → fix → push → re-poll.

### 6. Merge

`main` requires branches to be up to date. If the PR is behind `main` (`mergeStateStatus` is `BEHIND`, or merge fails with `required status checks are expected`), update the branch with `main`, wait for **GlassWorm Infection Audit** and **Guardrails** to pass on the new head, then merge right away. Merge sibling PRs one at a time.

```powershell
gh pr merge --squash --delete-branch
```

Report the squash merge commit SHA and timestamp (GMT+8).

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

### 9. Wiki pass [only if user-facing behavior changed]

Most pushes (docs, CI, refactors, internal fixes) need **nothing here** — skip. Only if this push adds, renames, or removes a user-facing feature or setting, sync the wiki (a **separate repo**: `https://github.com/Mooshieblob1/MooshieUI.wiki.git`, branch `master`, top-level `*.md`, nav `_Sidebar.md`).

```powershell
git clone -c core.autocrlf=false https://github.com/Mooshieblob1/MooshieUI.wiki.git <scratch>/MooshieUI.wiki   # outside the main repo
```

Edit the relevant page(s), **verifying exact UI labels against current code** (`src/lib/locales/en.ts` + the component) rather than trusting the commit message — labels get renamed before ship. Match the wiki's concise, present-tense voice. Then:

```powershell
git -c core.hooksPath=/dev/null commit -am "Document <feature>"
git -c core.hooksPath=/dev/null push origin master
```

Commit as `Mooshieblob <blob@mooshieblob.com>`. No `Co-Authored-By`, no em dashes. Report pages touched (or that no wiki change was needed).

## Checklist

```
- [ ] Pre-commit-check passed
- [ ] PR created chore/<slug> → main
- [ ] GlassWorm + Guardrails SUCCESS (branch up to date with main)
- [ ] Squash merged
- [ ] Local main reset to origin/main
- [ ] Wiki pass done if the push changed user-facing behavior (else skipped)
```

## Mistakes to avoid

1. Pushing directly to `main` (branch protection)
2. Omitting `core.hooksPath=/dev/null` on Windows
3. Including release version bumps — use **release** skill
4. Adding `Co-Authored-By` trailers to commits, PR bodies, or comments — never attribute AI assistance in any git or GitHub output
