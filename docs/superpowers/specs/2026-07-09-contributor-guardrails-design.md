# Contributor Guardrails for MooshieUI — Design

Date: 2026-07-09
Status: Approved (design), pending implementation plan

## Problem

MooshieUI's core guidelines are enforced only for the maintainer's AI-assisted
workflow (via `.claude` / `.agents` skills such as `pre-commit-check`). An external
human contributor opening a pull request hits almost no automated quality gate:

- CI on PRs is only the **GlassWorm** security scan plus unicode annotation.
- There is **no build, type, i18n, or a11y gate** in CI.
- The i18n parity tooling exists (`scripts/check-i18n-parity.mjs`,
  `scripts/precommit_locale_check.py`) but is **not wired into CI**, and neither
  script is a complete gate (see "Existing tooling" below).
- There is **no `CONTRIBUTING.md`, no PR template, and no issue templates** — a
  contributor has nowhere to learn the rules.
- There is **no written scope statement** to judge "is this in scope?" against.

## Goals

1. Make contribution and local development easier.
2. Ensure PRs are only accepted when they follow the core guidelines:
   - **i18n**: every key and `{placeholder}` in `en.ts` exists in all other locale
     files (currently 12 locale files).
   - **a11y**: accessible markup; caught via `svelte-check` compiler warnings.
   - **scope**: the project stays within its stated goals.
3. Give contributors clear, actionable feedback so they understand what to follow.
4. Persist the guidelines to the maintainer's assistant memory.

## Non-goals (YAGNI)

- No new ESLint / `eslint-plugin-svelte` stack. `svelte-check` already surfaces a11y
  warnings; a second linter is redundant for now.
- No devcontainer / Codespaces configuration.
- No bot that attempts to machine-judge scope. Scope stays human-reviewed.
- No change to the existing GlassWorm gate or release process.

## Decisions (from brainstorming)

- **Enforcement posture: Tiered gate.** Hard-block the checks that have tooling
  (build, `svelte-check` types, i18n parity). a11y runs **advisory-first** (a
  non-blocking PR comment) until the codebase is baseline-clean, then can flip to
  blocking. Scope is enforced by PR template + human review, not CI.
- **`svelte-check` is diff-scoped.** The current tree has **39 pre-existing
  `svelte-check` errors and 90 warnings** (build stays green because vite does not
  type-check; `svelte-check` does). A global type-block would make every PR red for
  debt the contributor did not cause. So the type gate blocks a PR only on
  `svelte-check` **errors in files the PR changed**, tolerating the baseline in
  untouched files. This matches the repo's diff-aware pre-commit philosophy. Known
  limitation: an error a change *induces* in an untouched file (e.g. removing an
  export) is not caught by diff-scoping; documented, acceptable for v1.
- **Scope model: Charter + PR attestation.** A `SCOPE.md` charter is the source of
  truth (goals + non-goals). The PR template requires a linked issue and a short
  "how this fits the charter" note. The maintainer judges against `SCOPE.md`. No CI
  scope-blocking.

## Existing tooling (grounding)

- `scripts/check-i18n-parity.mjs`: loops **all** locale files, reports missing/extra
  keys and likely-untranslated values, but **never exits non-zero** and does **not**
  check placeholder parity. Report-only; not CI-ready.
- `scripts/precommit_locale_check.py`: exits non-zero and checks `{placeholder}`
  interpolation parity, but only for a **single hardcoded pair** (`en` vs `es`).
- `svelte-check` (v4) is an installed devDependency but has no npm script and is not
  in CI. Its Svelte-compiler pass surfaces a11y warnings and type errors.
- `scripts/setup-branch-protection.sh` already scripts required-check configuration.

## Architecture — six layers

### Layer 1: Docs ("what to follow")

Repo files (authoritative source of truth):

- `CONTRIBUTING.md` — one-stop guide covering:
  - setup and the local pre-flight command (`npm run check`),
  - the core guidelines: i18n (all locales), a11y (svelte-check), scope (link to
    `SCOPE.md`), plus existing conventions already documented in `CLAUDE.md`
    (`ipcInvoke` never raw `invoke`, Tailwind-only + no `<style>` blocks, `onclick`
    not `on:click`, Rust commands return `Result<T, AppError>`),
  - a "What CI checks and how to fix each" table so a red check is self-explaining.
- `SCOPE.md` — the charter: what MooshieUI **is** (goals) and explicitly **is not**
  (non-goals). Drafted from the project's actual purpose (a Tauri + web ComfyUI
  front-end); reviewed by the maintainer before merge.
- `.github/PULL_REQUEST_TEMPLATE.md` — required checklist:
  - Linked issue: `#___`
  - How this fits the charter (`SCOPE.md`): ______
  - [ ] i18n: new user-facing strings added to **all** locale files
  - [ ] a11y: interactive elements are keyboard- and screen-reader-accessible
  - [ ] Ran `npm run check` locally and it passed
- `.github/ISSUE_TEMPLATE/bug_report.md` and `feature_request.md` — the feature
  template asks "how does this fit the scope?" up front, so out-of-scope ideas are
  caught at issue time before code is written. Include `config.yml` if needed to keep
  blank issues enabled.

### Layer 1b: Wiki (user-facing docs mirror)

The wiki is a **separate repo**: `https://github.com/Mooshieblob1/MooshieUI.wiki.git`,
branch `master`, top-level `*.md`, nav in `_Sidebar.md`.

- Add a **Contributing** page mirroring the essentials of `CONTRIBUTING.md` (setup,
  pre-flight command, the three pillars, link back to the repo files as authoritative).
- Add a **Project Scope** page mirroring `SCOPE.md`.
- Add both to `_Sidebar.md` navigation.
- Written in the wiki's concise, present-tense voice. Repo files remain authoritative;
  the wiki summarizes and links back. Plain ASCII, no em dashes (maintainer-voice rule).

### Layer 2: Local self-check ("catch before you push")

New npm scripts in `package.json`:

- `check:i18n` -> `node scripts/check-i18n-parity.mjs` (upgraded gate, see Layer 5)
- `check:types` -> `svelte-check --output machine` piped to `svelte-check-diff.mjs`
  (surfaces all type errors + a11y warnings informationally). Runs full-tree, not
  diff-scoped, so the 39 baseline items show; contributors are told to ensure they did
  not add errors in the files they touched. Exits zero locally (informational).
- `check` -> `npm run build` + `check:i18n` (both hard-fail) then `check:types`
  (informational). The hard part mirrors CI's blocking build + i18n; the diff-scoped
  type enforcement is CI-only (it needs the PR base), so local `check:types` is a
  best-effort heads-up, documented as such in `CONTRIBUTING.md`.

Documented in `CONTRIBUTING.md` as the pre-flight. Giving contributors the same signal
locally as CI is the highest-leverage "makes it easier + drives adherence" lever: they
self-correct before opening a PR. The one honest asymmetry (local `check:types` shows
everything; CI blocks only new errors in changed files) is stated in the docs so a
green local run is not mistaken for a guaranteed-green CI.

### Layer 3: CI gate ("enforce on PR")

New workflow `.github/workflows/pr-guardrails.yml`, triggered on `pull_request`.
The `svelte-check` invocation is **diff-scoped** (see Decisions): its machine-format
output is parsed once and split into a blocking set (errors in changed files) and an
advisory set (a11y warnings in changed files). Concretely:

- **Blocking job** (`guardrails`): `npm ci` -> `npm run build` (fail on error) ->
  `node scripts/check-i18n-parity.mjs` (non-zero fails) -> run
  `svelte-check --output machine`, then a small parser (`scripts/svelte-check-diff.mjs`)
  that keeps only `ERROR` lines whose file is in the PR's changed-file set and **exits
  non-zero if any remain**. The 39 baseline errors in untouched files are ignored.
  Changed-file set comes from `git diff --name-only origin/<base>...HEAD`.
- **Advisory job** (`a11y-advisory`): reuse the same `svelte-check --output machine`
  output, keep `WARNING` lines matching the a11y rule ids (`a11y_*`) whose file is a
  changed `.svelte` file, and write them to the **GitHub job Step Summary**
  (`$GITHUB_STEP_SUMMARY`), always exiting zero (non-blocking). The Step Summary shows
  on the PR's Checks tab. Documented switch to flip a11y into the blocking job once the
  codebase is a11y-baseline-clean.
- **Scope**: no CI enforcement (per decision) — PR template + human review against
  `SCOPE.md`.

**Fork-PR constraint (why Step Summary, not a comment).** External contributors open
PRs from forks. Under the `pull_request` trigger a fork's `GITHUB_TOKEN` is read-only,
so `gh pr comment` silently fails; `pull_request_target` would grant write but runs the
base workflow against untrusted code (security footgun). The Step Summary needs no
token and works for forks. A real PR comment (nicer UX) is a **documented later
upgrade** via the two-workflow `workflow_run` pattern; out of scope for v1.

**Action pinning.** Pin `actions/checkout`, `actions/setup-node` by commit SHA (repo
convention). The plan resolves real SHAs at implementation time with `gh api` (it does
not hardcode possibly-wrong constants). No third-party actions are used — the Node
setup plus `gh` (preinstalled on the runner) cover everything; the a11y output is a
Step Summary, so no comment-posting action is needed.

Both jobs need the same `svelte-check --output machine` result. For simplicity each job
runs it independently (svelte-check finishes in well under a minute here); no artifact
sharing.

Register the blocking `guardrails` job as a required status check (extend
`scripts/setup-branch-protection.sh`; the maintainer runs it). GlassWorm remains a
required check unchanged.

Pin third-party actions by commit SHA, matching the existing workflows' convention.

### Layer 4: Feedback ("so contributors understand")

- The i18n gate prints exactly which locale file is missing which key and which
  placeholder mismatches exist, pointing at `en.ts` and the CONTRIBUTING i18n section.
- The advisory a11y comment lists each warning by `file:line` with a one-line fix hint
  and a link to the CONTRIBUTING a11y section.
- `CONTRIBUTING.md`'s "What CI checks and how to fix each" table means any red check is
  self-explaining without the contributor reading workflow YAML.

### Layer 5: Tooling work — upgrade the i18n gate

Rewrite `scripts/check-i18n-parity.mjs` into a real gate while keeping its friendly
report:

- Loop **all** locale files in `src/lib/locales/` against `en.ts`.
- Fail on: keys in `en.ts` missing from any locale; keys in a locale not in `en.ts`;
  `{placeholder}` set mismatch for any shared key.
- Keep the human-readable per-file report (counts, samples), and additionally print a
  concise failure summary.
- Exit non-zero on any failure, zero on clean. Keep the "likely untranslated" report as
  informational (does not fail the build).
- The en/es-only `scripts/precommit_locale_check.py` becomes redundant; remove it (or
  leave as-is; it is superseded and no longer referenced).

### Layer 6: Memory + assistant context

After the repo files exist, persist the three core guidelines to the maintainer's
assistant memory so they stay in context across sessions:

- A `project`/`feedback`-type memory recording: MooshieUI enforces i18n parity across
  all locales, a11y via `svelte-check`, and scope via the `SCOPE.md` charter; PRs are
  gated by the `pr-guardrails` CI workflow; contributors run `npm run check` locally.
- Cross-link to the relevant repo paths so future sessions verify against real files.

## Component boundaries

| Unit | Purpose | Depends on |
|------|---------|-----------|
| `scripts/check-i18n-parity.mjs` | i18n parity + placeholder gate | `src/lib/locales/*.ts` |
| `scripts/svelte-check-diff.mjs` | filter svelte-check machine output to a changed-file set; split errors (block) / a11y warnings (advise); exit non-zero on blocking errors | svelte-check machine output, git changed-file list |
| `package.json` scripts | local pre-flight mirroring CI | build, svelte-check, i18n script |
| `pr-guardrails.yml` | enforce blocking checks + advisory a11y | npm scripts, PR diff |
| `CONTRIBUTING.md` | teach the rules + fix table | links to SCOPE.md, scripts |
| `SCOPE.md` | scope source of truth | none |
| PR / issue templates | attestation + scope-at-issue-time | links to SCOPE.md |
| Wiki pages | user-facing mirror | repo docs (authoritative) |

## Testing / validation

No frontend/Rust test framework exists; validation is consistent with the repo norm:

- i18n gate: exercise `node scripts/check-i18n-parity.mjs` against the current tree
  (must pass) and against a deliberately broken locale (must fail non-zero) to confirm
  the gate works. Revert the deliberate break.
- `svelte-check-diff.mjs`: feed it a captured `svelte-check --output machine` sample
  plus a changed-file list; assert it blocks when a changed file has an ERROR, passes
  when the only ERRORs are in untouched (baseline) files, and lists a11y warnings only
  for changed `.svelte` files. Uses a small fixture, run with `node`.
- `npm run check` runs clean on a clean tree (build + i18n hard-pass; `check:types`
  prints the baseline informationally and exits zero).
- The CI workflow is validated by opening the implementation PR itself (it runs on that
  PR); confirm the blocking job passes and the advisory a11y comment appears.
- Wiki changes: verify rendered pages and `_Sidebar.md` nav after push.

## Rollout order

1. Upgrade `check-i18n-parity.mjs` + add npm scripts (Layer 5, 2).
2. Add CI workflow (Layer 3).
3. Add docs + templates (Layer 1) and wiki pages (Layer 1b).
4. Register required check via branch-protection script (maintainer-run).
5. Persist memory (Layer 6).

Steps 1 to 3 land via a normal PR (the `push` skill). The branch-protection change and
wiki push are maintainer-run follow-ups.

## Open item carried to the plan

- a11y advisory is comment-based from day one (chosen). Flipping it to blocking is a
  later, separate change once the baseline is clean; not in this scope.
