# MooshieUI — Grok Agent Config

**Canonical source:** [`.agents/`](../.agents/) — edit skills and rules there, then sync to this folder.

## Skills (slash commands)

| Skill | Invoke | Purpose |
|-------|--------|---------|
| [push](skills/push/SKILL.md) | `/push` | PR to main, no release |
| [release](skills/release/SKILL.md) | `/release` | Version bump, tag, CI release |
| [quickrelease](skills/quickrelease/SKILL.md) | `/quickrelease` | Fast release (skips checks) |
| [cleanup](skills/cleanup/SKILL.md) | `/cleanup` | Branch hygiene + bot PR triage |
| [pre-commit-check](skills/pre-commit-check/SKILL.md) | (auto) | Pre-commit / pre-PR validation |
| [add-tauri-command](skills/add-tauri-command/SKILL.md) | `/add-tauri-command` | New Tauri + TS IPC command |
| [add-generation-param](skills/add-generation-param/SKILL.md) | `/add-generation-param` | New generation setting (full stack) |
| [workflow-template-builder](skills/workflow-template-builder/SKILL.md) | — | ComfyUI workflow templates in Rust |

## Rules

Copied from [`.agents/rules/`](../.agents/rules/):

| Rule | When |
|------|------|
| [mooshie-core](rules/mooshie-core.md) | Always — build, IPC, git/release |
| [mooshie-architect](rules/mooshie-architect.md) | System design, dual-mode, workflows |
| [mooshie-code-frontend](rules/mooshie-code-frontend.md) | Files under `src/` |
| [mooshie-code-rust](rules/mooshie-code-rust.md) | Files under `src-tauri/` |
| [mooshie-debug](rules/mooshie-debug.md) | Bugs, logs, browser mode |
| [mooshie-ask](rules/mooshie-ask.md) | Explanations, navigation |

## Sync

After editing `.agents/skills/` or `.agents/rules/`, re-copy to `.grok/`:

```powershell
Copy-Item -Path ".agents\skills\*" -Destination ".grok\skills\" -Recurse -Force
Copy-Item -Path ".agents\rules\*" -Destination ".grok\rules\" -Force
```

## Also see

- [AGENTS.md](../AGENTS.md) — repo entry for all agents
- [`.cursor/`](../.cursor/) — Cursor mirror
- [`.roo/commands/`](../.roo/commands/) — Roo slash commands
- [`.github/agents/`](../.github/agents/) — Copilot agents