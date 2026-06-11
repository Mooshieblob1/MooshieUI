---
name: cleanup
description: Branch hygiene and bot PR comment triage maintenance workflow
argument-hint: "Optional scope hint, e.g. 'release branches only' or 'all open PRs'"
agent: agent
---


# Cleanup (MooshieUI)

Standalone maintenance path: **branch hygiene → PR/bot triage → targeted cleanup actions → summary**.

This workflow does **not** bump versions, edit release notes/changelog, create release tags, or trigger release CI.

## Windows git (required)

Prefix git commands with:

```text
git -c core.hooksPath=/dev/null ...
```

Pre-commit hook is bash and hangs in PowerShell.

## Workflow

### 1. Branch inventory and stale detection

```powershell
git fetch --prune origin
git branch -vv
git branch -r
gh pr list --state open --base main --json number,title,headRefName,updatedAt,isDraft
gh api repos/Mooshieblob1/MooshieUI/branches?per_page=100 --jq '.[].name'
```

Identify:
- stale local branches tracking deleted remotes
- stale remote `release/v*` branches with merged/closed PRs
- duplicate release branches for the same version
- open PR branches with no recent activity and no merge intent

### 2. Bot comment triage scope

Review bot comments on:
1) open PRs to `main`
2) recently merged PRs (default: last 20, or since last tag)

```powershell
gh pr list --state open --base main --json number,title,headRefName
gh pr list --state merged --base main --limit 20 --json number,title,mergedAt
# Per PR N:
gh pr view N --json reviews,comments,state,mergedAt
gh api repos/Mooshieblob1/MooshieUI/pulls/N/comments
gh api repos/Mooshieblob1/MooshieUI/pulls/N/reviews
```

Primary bots: `gemini-code-assist[bot]`, `copilot-pull-request-reviewer[bot]`, and actionable `github-actions[bot]` comments.

Classify with `docs/BOT_REVIEW_TRIAGE.md`:
- **Fix**: correctness/safety/consistency
- **Skip**: stylistic nits, premature abstraction, factually wrong
- **Defer**: valid but not maintenance-blocking

### 3. Cleanup actions

Apply only safe, explicit actions:
- Delete stale local branches
- Delete stale remote release branches (`release/v*`) that are merged/abandoned
- Close superseded release PRs when appropriate
- For bot **Fix** findings:
  - open PR: push fix on that branch
  - merged PR: create follow-up branch/PR (do not rewrite history)

Avoid destructive/unsafe actions:
- never force-push protected branches
- never delete unmerged branches with active intent
- never force-move or delete tags

### 4. Re-check and report

Re-run inventory commands after actions and provide:
- branch/PR action table (what changed)
- bot triage table (PR, comment, verdict, rationale)
- unresolved follow-ups (if any)

## Checklist

```
- [ ] Remote/local branches inventoried and pruned
- [ ] Stale `release/v*` branches addressed
- [ ] Open PRs reviewed for stale/conflicting branch state
- [ ] Bot comments triaged on open + recent merged PRs
- [ ] Fix/Skip/Defer outcomes documented
- [ ] Safe cleanup actions completed and re-verified
```

## Mistakes to avoid

1. Deleting active unmerged branches without confirmation
2. Ignoring bot comments on non-release open PRs
3. Treating all bot comments as mandatory fixes
4. Force-updating tags/branches during cleanup
