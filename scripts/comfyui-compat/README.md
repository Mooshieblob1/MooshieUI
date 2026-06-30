# ComfyUI custom-node compatibility check

MooshieUI pins ComfyUI to a release tag (`COMFYUI_REF` in
[`src-tauri/src/setup.rs`](../../src-tauri/src/setup.rs)). Fresh installs and the
in-app updater both target that tag. Bumping the pin is usually safe for the app
itself, but ComfyUI's internal refactors can break MooshieUI's **bundled custom
nodes**, which import deep ComfyUI internals (`comfy.sample`, `comfy.samplers`,
`comfy.model_management`, `folder_paths`, `latent_preview`, ...). When such an
import breaks, the node silently fails to register and disappears from
`/object_info` while ComfyUI itself still starts.

`smoke_test.py` is the checker for that. It reproduces what the app does at
runtime (`ensure_mooshie_nodes` in `src-tauri/src/comfyui/nodes.rs`): deploy the
bundled nodes into a ComfyUI checkout, start ComfyUI in CPU mode, and assert
every required node class appears in `/object_info`.

## What it verifies

The required node classes and the `ultralytics` pin are parsed from
`src-tauri/src/comfyui/nodes.rs` (single source of truth), so the test never
drifts from the app. Today that is:

`MooshieSaveImage`, `MooshieFaceDetailer`, `MooshieSegmentDetailer`,
`MooshieSoftGuidance`, `MooshieSmartGuidance`, `NanoSaurLoader`,
`ApplyTiledDiffusion`.

## Scope and limitations

- **Bundled MooshieUI nodes only.** The external ControlNet and style-transfer
  packages (`comfyui_controlnet_aux`, `ComfyUi-Untwisting-RoPE`, ...) are
  third-party git repos cloned at runtime; their compatibility is their own
  maintainers' concern and is out of scope here. The script logs this rather
  than silently skipping it.
- It checks **registration**, not generation. A node that registers can still
  misbehave at inference time; this catches import/registration breakage, which
  is the common failure mode for ComfyUI version bumps.

## Run it locally

```bash
# 1. Get a ComfyUI checkout at the version you want to test:
git clone --depth=1 --branch v0.26.0 https://github.com/comfyanonymous/ComfyUI.git comfyui-target

# 2. Install deps into the active Python env (CPU torch is fine):
pip install torch torchvision --index-url https://download.pytorch.org/whl/cpu
pip install -r comfyui-target/requirements.txt
pip install ultralytics==8.4.75

# 3. Run the smoke test from the repo root:
python scripts/comfyui-compat/smoke_test.py --comfyui-dir comfyui-target
```

Exit code `0` = all required bundled nodes registered. `1` = at least one failed
(the ComfyUI log tail is printed, and the full log is at
`comfyui-target/comfyui-smoke.log`).

## CI bot

[`.github/workflows/comfyui-compat.yml`](../../.github/workflows/comfyui-compat.yml)
runs this weekly (and on demand via "Run workflow"):

1. **detect** parses the current `COMFYUI_REF` and the latest ComfyUI GitHub
   release, and decides whether the candidate is strictly newer.
2. **smoke-test** checks out that candidate, installs deps, and runs
   `smoke_test.py`. The log and a JSON summary are uploaded as an artifact.
3. **propose** (only when the smoke test passes and the candidate is newer)
   bumps `COMFYUI_REF` with a one-line `sed`, commits it to a
   `bot/comfyui-bump-<tag>` branch, and opens a PR with `gh`. The compatibility
   result and a link to the smoke-test run go in the PR body.

If the smoke test fails, the run goes red and no PR is opened: that newer ComfyUI
release would break the bundled nodes and needs code changes first.

### No secrets or API keys

The bot is fully deterministic and never calls an LLM, so there is **nothing to
configure**: every job runs on the free Actions `GITHUB_TOKEN`, and no
`ANTHROPIC_API_KEY` (or any other secret) is required or consumed. The PR it
opens is a mechanical pin bump plus the smoke-test result; a maintainer reviews
and merges it (optionally using Claude Code locally to vet the change). The
smoke test passing is the "this is mergeable" signal.

### Maintainer note on the bot PR

PRs opened with the default Actions token do not automatically trigger other
workflows, so the required status checks (for example the GlassWorm Infection
Audit) may not run on the bot's PR. To start them, close and reopen the PR or
push an empty commit before merging.
