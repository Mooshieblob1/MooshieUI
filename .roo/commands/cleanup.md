---
name: cleanup
description: Branch hygiene and bot PR comment triage maintenance workflow
argument-hint: "Optional scope hint, e.g. 'release branches only' or 'all open PRs'"
---

Run a standalone maintenance cleanup for branch hygiene and bot PR comment triage. Execute autonomously and summarize actions taken.

## Goal

Clean stale/conflicting branches and triage bot comments across relevant PRs without cutting a release.

## Execution Plan

### 1. Branch hygiene inventory

```powershell
git fetch --prune origin
git branch -vv
git branch -r
gh pr list --state open --base main --json number,title,headRefName,updatedAt,isDraft
gh api repos/Mooshieblob1/MooshieUI/branches?per_page=100 --jq '.[].name'
```

Identify and classify:
- stale local branches tracking deleted remotes
- stale/duplicate remote `release/v*` branches
- open PR branches superseded by newer work

### 2. Bot comment triage

Read bot comments for open PRs and recently merged PRs:

```powershell
gh pr list --state open --base main --json number,title,headRefName
gh pr list --state merged --base main --limit 20 --json number,title,mergedAt
# Per PR N:
gh pr view N --json reviews,comments,state,mergedAt
gh api repos/Mooshieblob1/MooshieUI/pulls/N/comments
gh api repos/Mooshieblob1/MooshieUI/pulls/N/reviews
```

Classify with `docs/BOT_REVIEW_TRIAGE.md`:
- **Fix**: correctness/safety/consistency
- **Skip**: nits/premature abstraction/factually wrong
- **Defer**: valid but non-blocking

### 3. Apply cleanup actions

Safe actions only:
- delete stale locals
- delete stale remote `release/v*` branches with merged/closed PRs
- close superseded stale PRs where appropriate
- for bot **Fix** findings:
  - open PR → fix on branch and push
  - merged PR → create follow-up branch/PR

Never:
- force-push protected branches
- delete active unmerged branches with ongoing work intent
- move/delete tags

### 4. Verify and report

Re-run branch inventory and report:
- branch action table (branch/PR, status, action)
- bot triage table (PR, bot comment, verdict, rationale)
- unresolved follow-ups

## Common mistakes to avoid

1. Treating every bot comment as mandatory
2. Deleting branches with active unmerged work
3. Ignoring recently merged PR bot comments
4. Using destructive git operations for convenience
