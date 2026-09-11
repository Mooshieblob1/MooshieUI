# MooshieUI documentation

Documentation for the current implementation, reviewed against **v2.3.2**. Start with the [repository README](../README.md) for an overview or the [user wiki](https://github.com/Mooshieblob1/MooshieUI/wiki) for installation and feature walkthroughs.

## Feature and technical references

| Document | Purpose |
|----------|---------|
| [METADATA_CARRIERS.md](METADATA_CARRIERS.md) | Where generation metadata lives per output format, and what survives |
| [NOVELAI.md](NOVELAI.md) | NovelAI architecture, personal keys, face detailing, local post-processing and historical test notes |
| [RANDOM_PROMPTS.md](RANDOM_PROMPTS.md) | Seeded alternation, multiple choices, weights, nesting and escapes |
| [STYLE_REFERENCE.md](STYLE_REFERENCE.md) | IP-Adapter/Flux Redux support, model files and controls |
| [MACOS.md](MACOS.md) | Apple Silicon candidate installation, validation, and release gating |

The wiki also covers [video](https://github.com/Mooshieblob1/MooshieUI/wiki/Video-Generation), [pause and continue](https://github.com/Mooshieblob1/MooshieUI/wiki/Generation-Basics#pause-and-continue-a-generation), [Style Creator](https://github.com/Mooshieblob1/MooshieUI/wiki/Prompting-Guide#style-creator), [Image Edit](https://github.com/Mooshieblob1/MooshieUI/wiki/Image-Edit-Mode), and [hosted accounts](https://github.com/Mooshieblob1/MooshieUI/wiki/Server,-LAN-and-Multi-User).

## Planning and maintenance

These documents include proposals and historical evidence; they are not promises of available features.

| Document | Purpose |
|----------|---------|
| [ROADMAP-2026-gaps.md](ROADMAP-2026-gaps.md) | Original gap analysis with current implementation status |
| [FEATURE_RESEARCH.md](FEATURE_RESEARCH.md) | Historical research shortlist; its ecosystem table is a snapshot |
| [MACOS_RELEASE_PLAN.md](MACOS_RELEASE_PLAN.md) | Apple Silicon release requirements and outstanding qualification gates |
| [BOT_REVIEW_TRIAGE.md](BOT_REVIEW_TRIAGE.md) | Triage notes for automated PR review comments |
| [issue_cleanup_followup_tracks.md](issue_cleanup_followup_tracks.md) | Historical issue follow-up tracks |

Local implementation plans and specs may also exist under `docs/superpowers/`. That directory is git-ignored and is not part of a fresh clone's published documentation.

## Elsewhere

| Topic | Path |
|-------|------|
| User-facing readme | [README.md](../README.md) |
| Contributing / PR workflow | [push-instructions.md](../push-instructions.md) |
| Contributor checks / scope | [CONTRIBUTING.md](../CONTRIBUTING.md), [SCOPE.md](../SCOPE.md) |
| Changelog / release notes | [CHANGELOG.md](../CHANGELOG.md), [RELEASE_NOTES.md](../RELEASE_NOTES.md) |
| Agent entry | [AGENTS.md](../AGENTS.md) |
| Copilot / Gemini overview | [GEMINI.md](../GEMINI.md), [.github/copilot-instructions.md](../.github/copilot-instructions.md) |
| Full agent conventions | [.github/instructions/mooshieui.instructions.md](../.github/instructions/mooshieui.instructions.md) |
| Layer-specific rules | [.github/instructions/](../.github/instructions/) |
| Smoke test runbook | [scripts/SMOKE_TEST_RUNBOOK.md](../scripts/SMOKE_TEST_RUNBOOK.md) |

## Keeping documentation current

For a user-facing change, update the relevant wiki page and its navigation, the README if it changes a headline capability or setup step, and any technical reference above. Check the code and shipped release notes for backend/model restrictions and exact control names. Keep dated experiments and plans identified as history rather than rewriting them as current behavior.

The wiki is a separate Git repository (`https://github.com/Mooshieblob1/MooshieUI.wiki.git`). Changes here do not automatically publish wiki changes, and wiki edits do not update this repository. Check relative links, wiki page names and section anchors in both before publishing.
