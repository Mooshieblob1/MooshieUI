# MooshieUI Agent Config (`.agents/`)

**Canonical source** for project skills and rules. Cursor, Roo, and Copilot configs are synced from here.

## Skills

Invoke by name or slash-style request (`/push`, `/release`, etc.):

| Skill | Purpose |
|-------|---------|
| [push](skills/push/SKILL.md) | PR to main, no release |
| [release](skills/release/SKILL.md) | Version bump, tag, CI release |
| [quickrelease](skills/quickrelease/SKILL.md) | Quick release: version bump, tag, CI release (no local compile/lint checks) |
| [cleanup](skills/cleanup/SKILL.md) | Branch hygiene + bot PR triage |
| [pre-commit-check](skills/pre-commit-check/SKILL.md) | Pre-commit / pre-PR validation |
| [add-tauri-command](skills/add-tauri-command/SKILL.md) | New Tauri + TS IPC command |
| [add-generation-param](skills/add-generation-param/SKILL.md) | New generation setting (full stack) |
| [workflow-template-builder](skills/workflow-template-builder/SKILL.md) | ComfyUI workflow templates in Rust |

## Rules

| Rule | When |
|------|------|
| [mooshie-core](rules/mooshie-core.md) | **Always** — build, IPC, git/release |
| [mooshie-architect](rules/mooshie-architect.md) | System design, dual-mode, workflows |
| [mooshie-code-frontend](rules/mooshie-code-frontend.md) | Files under `src/` |
| [mooshie-code-rust](rules/mooshie-code-rust.md) | Files under `src-tauri/` |
| [mooshie-debug](rules/mooshie-debug.md) | Bugs, logs, browser mode |
| [mooshie-ask](rules/mooshie-ask.md) | Explanations, navigation |

## Sync targets

| Target | Format | Notes |
|--------|--------|-------|
| [`.claude/skills/`](../.claude/skills/) | `SKILL.md` | Direct copy (Claude Code project skills) |
| [`.cursor/skills/`](../.cursor/skills/) | `SKILL.md` | Direct copy |
| [`.cursor/rules/`](../.cursor/rules/) | `.mdc` | Cursor frontmatter (`alwaysApply`, `globs`) |
| [`.roo/commands/`](../.roo/commands/) | `.md` | Roo slash commands + `argument-hint` |
| [`.github/agents/`](../.github/agents/) | `.agent.md` | Copilot agents (skill + reference inlined) |

Roo mode rules remain in [`.roo/rules-*/`](../.roo/) (architect, code, debug, ask). When editing conventions, update `.agents/rules/` first, then re-sync `.cursor/rules/`.

## Also see

- [AGENTS.md](../AGENTS.md) — repo entry for all agents
- [`.cursor/README.md`](../.cursor/README.md) — Cursor-specific index
