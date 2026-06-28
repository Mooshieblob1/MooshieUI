# ComfyUI-MooshieTiledDiffusion Standalone Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish MooshieUI's tiled diffusion node as a standalone, registry-installable ComfyUI custom node, in response to issue #338.

**Architecture:** Create a new public GitHub repo that vendors a byte for byte copy of `comfyui-nodes/nodes_tiled_diffusion.py`. MooshieUI stays canonical and is not modified. The repo adds only the packaging glue ComfyUI and the Comfy Registry require: `__init__.py`, `pyproject.toml`, `LICENSE`, `README.md`, `SYNC.md`, `.gitignore`, and a publish workflow.

**Tech Stack:** Python (ComfyUI V3 extension API: `comfy_api.latest`, `ComfyExtension`, `comfy_entrypoint`), GitHub Actions, Comfy Registry (`Comfy-Org/publish-node-action@main`), `gh` CLI.

**Spec:** `docs/superpowers/specs/2026-06-21-comfyui-mooshietileddiffusion-standalone-design.md`

## Global Constraints

- Repo name: `ComfyUI-MooshieTiledDiffusion`, public, under the `Mooshieblob1` GitHub account.
- Repo description: `MultiDiffusion and SpotDiffusion tiled diffusion for ComfyUI with Anima (COSMOS) support. Standalone export from MooshieUI, unmaintained.`
- License: GPL-3.0-or-later (the node header derives from ComfyUI, which is GPL-3.0). Not optional.
- Registry: `[tool.comfy] PublisherId = "mooshieblob"`. Version starts at `1.0.0`.
- Publish action: `Comfy-Org/publish-node-action@main`, input `personal_access_token`, repo secret `REGISTRY_ACCESS_TOKEN`. Trigger: push to `main` changing `pyproject.toml` (plus `workflow_dispatch`).
- `dependencies = []`: torch and comfy_api are provided by ComfyUI. No extra pip deps.
- The node implementation file is copied verbatim. Do not edit its logic.
- Canonical source: `comfyui-nodes/nodes_tiled_diffusion.py` at MooshieUI commit `00bd71a7791e50ee4d5373e6f093c74ac3047b9e` (file last changed in `d34b4af93a02053e39738f917e02c61cbd2bbd31`).
- All authored text (README, repo description, commit messages, the #338 comment) is plain ASCII: no em dashes, no en dashes, no curly quotes or apostrophes, no emojis, no rhetorical flourish. Short factual sentences.
- MooshieUI is not modified by this work. `src-tauri/src/setup.rs` and `comfyui-nodes/` stay as they are.
- This is a one off export with no promise of maintenance, stated plainly in the README.

## Working location

The new repo is built in a scratch directory outside the MooshieUI tree so its git history is independent. Use `../ComfyUI-MooshieTiledDiffusion` relative to the MooshieUI root (that is `d:\Repos\ComfyUI-MooshieTiledDiffusion`). All file paths below are inside that new repo unless prefixed with `MooshieUI/`.

## File Structure

Files created in the new repo:

- `nodes_tiled_diffusion.py` - verbatim copy of the canonical node. The whole implementation.
- `__init__.py` - re-exports `comfy_entrypoint` so ComfyUI discovers the V3 extension when the repo is loaded as a package.
- `pyproject.toml` - `[project]` metadata plus `[tool.comfy]` registry metadata.
- `LICENSE` - GPL-3.0 full text.
- `README.md` - purpose, install, usage, credits, no maintenance note.
- `SYNC.md` - upstream path and source commit for mirror provenance.
- `.gitignore` - Python caches.
- `.github/workflows/publish.yml` - Comfy Registry publish on pyproject change.

---

### Task 1: Create the GitHub repo and local working copy

**Files:**
- Create: the empty remote repo and a local clone at `d:\Repos\ComfyUI-MooshieTiledDiffusion`

**Interfaces:**
- Produces: an empty git repo with `origin` set, default branch `main`, ready for files.

- [ ] **Step 1: Confirm gh account**

Run: `gh api user --jq .login`
Expected: `Mooshieblob1`

- [ ] **Step 2: Create the public repo (no push yet)**

```bash
gh repo create ComfyUI-MooshieTiledDiffusion \
  --public \
  --description "MultiDiffusion and SpotDiffusion tiled diffusion for ComfyUI with Anima (COSMOS) support. Standalone export from MooshieUI, unmaintained." \
  --clone
```

Run this from `d:\Repos` so the clone lands at `d:\Repos\ComfyUI-MooshieTiledDiffusion`.
Expected: repo created on GitHub and an empty local clone exists.

- [ ] **Step 3: Verify the clone and default branch**

Run: `cd d:/Repos/ComfyUI-MooshieTiledDiffusion && git status && git branch --show-current`
Expected: clean working tree, branch is `main` (if `master`, run `git branch -m main`).

- [ ] **Step 4: STOP and prompt the user for the registry secret**

Tell the user the repo exists and ask them to add the `REGISTRY_ACCESS_TOKEN` secret now (GitHub UI or paste the key so you can run `gh secret set REGISTRY_ACCESS_TOKEN --repo Mooshieblob1/ComfyUI-MooshieTiledDiffusion`). Do not block file scaffolding on this, but the secret must exist before the first push to `main` for the publish to succeed. Continue to Task 2 once acknowledged.

---

### Task 2: Vendor the node file verbatim

**Files:**
- Create: `nodes_tiled_diffusion.py` (copy of `MooshieUI/comfyui-nodes/nodes_tiled_diffusion.py`)

**Interfaces:**
- Produces: module exposing `ApplyTiledDiffusion`, `TiledDiffusionExtension`, and `async def comfy_entrypoint()`.

- [ ] **Step 1: Copy the file byte for byte**

```bash
cp d:/Repos/MooshieUI/comfyui-nodes/nodes_tiled_diffusion.py \
   d:/Repos/ComfyUI-MooshieTiledDiffusion/nodes_tiled_diffusion.py
```

- [ ] **Step 2: Verify the copy is identical**

```bash
diff d:/Repos/MooshieUI/comfyui-nodes/nodes_tiled_diffusion.py \
     d:/Repos/ComfyUI-MooshieTiledDiffusion/nodes_tiled_diffusion.py && echo IDENTICAL
```
Expected: `IDENTICAL` (diff prints nothing).

- [ ] **Step 3: Syntax check**

Run: `python -m py_compile d:/Repos/ComfyUI-MooshieTiledDiffusion/nodes_tiled_diffusion.py && echo OK`
Expected: `OK`. (This checks syntax only; it does not import torch or comfy_api, which are not installed here.)

- [ ] **Step 4: Confirm the entrypoint exists in the copy**

Run: `grep -n "async def comfy_entrypoint" d:/Repos/ComfyUI-MooshieTiledDiffusion/nodes_tiled_diffusion.py`
Expected: one match.

---

### Task 3: Add the package entrypoint glue

**Files:**
- Create: `__init__.py`

**Interfaces:**
- Consumes: `comfy_entrypoint` from `nodes_tiled_diffusion`.
- Produces: package-level `comfy_entrypoint` so ComfyUI's loader finds it via `getattr(module, "comfy_entrypoint")` when the repo is loaded as a folder.

- [ ] **Step 1: Write `__init__.py`**

```python
"""ComfyUI-MooshieTiledDiffusion.

Standalone export of the tiled diffusion node from MooshieUI.
Re-exports the V3 extension entrypoint so ComfyUI discovers the node
when this repository is loaded as a custom node package.
"""

from .nodes_tiled_diffusion import comfy_entrypoint

__all__ = ["comfy_entrypoint"]
```

- [ ] **Step 2: Syntax check**

Run: `python -m py_compile d:/Repos/ComfyUI-MooshieTiledDiffusion/__init__.py && echo OK`
Expected: `OK`.

---

### Task 4: Add pyproject.toml

**Files:**
- Create: `pyproject.toml`

**Interfaces:**
- Produces: registry metadata. `name` is the immutable registry id; `PublisherId` must match the user's registry publisher exactly.

- [ ] **Step 1: Write `pyproject.toml`**

```toml
[project]
name = "comfyui-mooshietileddiffusion"
version = "1.0.0"
description = "MultiDiffusion and SpotDiffusion tiled diffusion for ComfyUI, with Anima (COSMOS) 5D latent support."
license = { text = "GPL-3.0-or-later" }
requires-python = ">=3.10"
readme = "README.md"
dependencies = []

[project.urls]
Repository = "https://github.com/Mooshieblob1/ComfyUI-MooshieTiledDiffusion"

[tool.comfy]
PublisherId = "mooshieblob"
DisplayName = "Mooshie Tiled Diffusion"
Repository = "https://github.com/Mooshieblob1/ComfyUI-MooshieTiledDiffusion"
```

- [ ] **Step 2: Validate TOML parses**

Run: `python -c "import tomllib,pathlib; tomllib.loads(pathlib.Path('d:/Repos/ComfyUI-MooshieTiledDiffusion/pyproject.toml').read_text()); print('OK')"`
Expected: `OK`.

- [ ] **Step 3: Confirm required registry fields are present**

Run: `python -c "import tomllib,pathlib; d=tomllib.loads(pathlib.Path('d:/Repos/ComfyUI-MooshieTiledDiffusion/pyproject.toml').read_text()); assert d['project']['name'] and d['project']['version'] and d['project']['description'] and d['project']['license']; assert d['tool']['comfy']['PublisherId']=='mooshieblob' and d['tool']['comfy']['DisplayName']; print('OK')"`
Expected: `OK`.

---

### Task 5: Add the GPL-3.0 LICENSE

**Files:**
- Create: `LICENSE`

**Interfaces:**
- Produces: the license file matching the GPL-3.0 header in the node source.

- [ ] **Step 1: Write the GPL-3.0 license text**

Fetch the canonical text and write it to `LICENSE`:

```bash
curl -fsSL https://www.gnu.org/licenses/gpl-3.0.txt \
  -o d:/Repos/ComfyUI-MooshieTiledDiffusion/LICENSE
```

If offline, copy the GPL-3.0 text from any GPL-3.0 project. The file must be the full unmodified license body.

- [ ] **Step 2: Verify it is the GPL-3.0 body**

Run: `grep -c "GNU GENERAL PUBLIC LICENSE" d:/Repos/ComfyUI-MooshieTiledDiffusion/LICENSE`
Expected: at least `1`.
Run: `grep -c "Version 3" d:/Repos/ComfyUI-MooshieTiledDiffusion/LICENSE`
Expected: at least `1`.

---

### Task 6: Add SYNC.md provenance

**Files:**
- Create: `SYNC.md`

**Interfaces:**
- Produces: human-readable record of where the vendored file came from.

- [ ] **Step 1: Write `SYNC.md`**

```markdown
# Sync provenance

This repository is a one way mirror. The canonical source of
nodes_tiled_diffusion.py is the MooshieUI repository.

- Upstream repo: https://github.com/Mooshieblob1/MooshieUI
- Upstream path: comfyui-nodes/nodes_tiled_diffusion.py
- Copied from commit: 00bd71a7791e50ee4d5373e6f093c74ac3047b9e
- File last changed upstream in commit: d34b4af93a02053e39738f917e02c61cbd2bbd31

To update this mirror, copy the upstream file over nodes_tiled_diffusion.py
again and update the commit hashes above. There is no automated sync.
```

---

### Task 7: Add .gitignore

**Files:**
- Create: `.gitignore`

- [ ] **Step 1: Write `.gitignore`**

```gitignore
__pycache__/
*.py[cod]
*.egg-info/
.DS_Store
```

---

### Task 8: Add the README

**Files:**
- Create: `README.md`

**Interfaces:**
- Produces: install and usage docs. Plain ASCII, no AI-tell punctuation, per the global constraint.

- [ ] **Step 1: Write `README.md`**

```markdown
# ComfyUI-MooshieTiledDiffusion

Standalone export of the tiled diffusion node from MooshieUI, published by
request (issue #338). Provided as is, with no promise of maintenance or support.

## What it does

Adds one node, Apply Tiled Diffusion, that patches a model to denoise in tiles
and blend the tiles back together. This lets you generate and upscale above a
model's native resolution without the seams and artifacts you get from a single
large pass.

Two methods:

- MultiDiffusion: overlapping tiles with cosine feathered blending. Best quality.
- SpotDiffusion: random tile shift per denoising step, no overlap. Fastest.

It supports 4D and 5D latents, so it works with Anima (COSMOS based) models as
well as standard diffusion models. It works in txt2img, img2img, and inpainting,
and crops spatial conditioning (inpaint masks and ControlNet) per tile.

The only dependencies are torch and the ComfyUI API, both of which ComfyUI
already provides. There are no extra packages to install.

## Install

Pick one.

- ComfyUI-Manager: search for Mooshie Tiled Diffusion and install.
- Comfy Registry: install comfyui-mooshietileddiffusion.
- Manual: clone this repo into ComfyUI/custom_nodes and restart ComfyUI.

```
cd ComfyUI/custom_nodes
git clone https://github.com/Mooshieblob1/ComfyUI-MooshieTiledDiffusion
```

## Usage

1. Add the Apply Tiled Diffusion node (category: model_patches/unet).
2. Connect your model into the node and the node MODEL output into your KSampler.
3. Set tile_width and tile_height to the model native resolution (for example
   1024 for SDXL or Flux, 512 for SD1.5).
4. Set tile_overlap for MultiDiffusion. Higher overlap means smoother seams and
   slower generation. Overlap is ignored by SpotDiffusion.

Use MultiDiffusion when you want the cleanest result. Use SpotDiffusion when you
want speed and can accept an experimental method.

This is useful above about 2.4 MP, where a single pass tends to show artifacts.

## License

GPL-3.0-or-later. The node is derived from ComfyUI, which is GPL-3.0.

## Credits

- ComfyUI by Comfy Org.
- MultiDiffusion: Bar-Tal et al., MultiDiffusion: Fusing Diffusion Paths for
  Controlled Image Generation, ICML 2023. arxiv.org/abs/2302.08113
- SpotDiffusion: Ding et al., 2024. arxiv.org/abs/2407.15507
```

- [ ] **Step 2: Scan the README for disallowed punctuation**

Run (from the new repo root):
`grep -nP "[\x{2010}-\x{2015}\x{2018}\x{2019}\x{201C}\x{201D}\x{2026}]" d:/Repos/ComfyUI-MooshieTiledDiffusion/README.md && echo "FOUND BAD CHARS" || echo "CLEAN"`
Expected: `CLEAN` (no em/en dashes, curly quotes, or ellipsis characters).

---

### Task 9: Add the Comfy Registry publish workflow

**Files:**
- Create: `.github/workflows/publish.yml`

**Interfaces:**
- Consumes: repo secret `REGISTRY_ACCESS_TOKEN`.
- Produces: a workflow that publishes to the registry on pyproject changes.

- [ ] **Step 1: Write `.github/workflows/publish.yml`**

```yaml
name: Publish to Comfy Registry

on:
  push:
    branches: [main]
    paths: ["pyproject.toml"]
  workflow_dispatch:

jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Publish custom node
        uses: Comfy-Org/publish-node-action@main
        with:
          personal_access_token: ${{ secrets.REGISTRY_ACCESS_TOKEN }}
```

- [ ] **Step 2: Validate the YAML parses**

Run: `python -c "import yaml,pathlib; yaml.safe_load(pathlib.Path('d:/Repos/ComfyUI-MooshieTiledDiffusion/.github/workflows/publish.yml').read_text()); print('OK')"`
Expected: `OK`. (If PyYAML is missing, skip and rely on the GitHub Actions run in Task 10.)

---

### Task 10: Commit, confirm secret, push, verify publish

**Files:**
- Modify: all of the above (first commit on `main`)

- [ ] **Step 1: Stage and review**

```bash
cd d:/Repos/ComfyUI-MooshieTiledDiffusion
git add .
git status
```
Expected: all 8 files staged (`__init__.py`, `nodes_tiled_diffusion.py`, `pyproject.toml`, `LICENSE`, `README.md`, `SYNC.md`, `.gitignore`, `.github/workflows/publish.yml`).

- [ ] **Step 2: Confirm the registry secret is set before pushing**

Run: `gh secret list --repo Mooshieblob1/ComfyUI-MooshieTiledDiffusion`
Expected: `REGISTRY_ACCESS_TOKEN` is listed. If it is not, prompt the user to add it (or set it with `gh secret set REGISTRY_ACCESS_TOKEN --repo Mooshieblob1/ComfyUI-MooshieTiledDiffusion`) before continuing.

- [ ] **Step 3: Commit**

```bash
git commit -m "Add standalone tiled diffusion node, scaffolding, and registry publish workflow"
```

- [ ] **Step 4: Push to main**

```bash
git push -u origin main
```

- [ ] **Step 5: Verify the publish workflow ran**

Run: `gh run list --repo Mooshieblob1/ComfyUI-MooshieTiledDiffusion --workflow "Publish to Comfy Registry"`
Then: `gh run watch <run-id> --repo Mooshieblob1/ComfyUI-MooshieTiledDiffusion` (or `gh run view <run-id> --log-failed` on failure).
Expected: the run succeeds. If it fails on auth, the secret is missing or wrong; fix and re-run with `gh workflow run "Publish to Comfy Registry" --repo Mooshieblob1/ComfyUI-MooshieTiledDiffusion`.

- [ ] **Step 6: Confirm the node appears on the registry**

Check `https://registry.comfy.org` for `comfyui-mooshietileddiffusion` under publisher `mooshieblob`. Expected: the node pack is listed at version 1.0.0.

---

### Task 11: Comment on issue #338

**Files:**
- None (GitHub issue comment on the MooshieUI repo)

**Interfaces:**
- Consumes: the live repo URL and registry listing from Task 10.

- [ ] **Step 1: Post the comment in the user's voice**

Plain ASCII, no em dashes, no curly quotes, no emojis. Run from the MooshieUI repo:

```bash
gh issue comment 338 --body "Done. I have published the tiled diffusion node as a standalone ComfyUI custom node.

Repo: https://github.com/Mooshieblob1/ComfyUI-MooshieTiledDiffusion

It is the same node MooshieUI uses, so it supports Anima (COSMOS) and 5D latents, MultiDiffusion and SpotDiffusion, and txt2img, img2img, and inpainting. It works above 2.4 MP where a single pass shows artifacts.

Install with ComfyUI-Manager (search Mooshie Tiled Diffusion), the Comfy Registry (comfyui-mooshietileddiffusion), or git clone into custom_nodes.

This is a one off export with no promise of ongoing maintenance. The canonical source stays in MooshieUI."
```

- [ ] **Step 2: Decide on closing the issue**

Ask the user whether to close #338 now or leave it open for the requester to confirm. Do not close it without the user's say so.

---

## Self-Review

**Spec coverage:**
- Source of truth / one way mirror: Tasks 2 and 6. Covered.
- Repo layout (8 files): Tasks 2 through 9. Covered.
- Package loading via `__init__.py` re-export: Task 3. Covered.
- pyproject.toml fields and PublisherId: Task 4. Covered.
- Registry publishing action, secret, trigger: Tasks 9 and 10. Covered.
- README content and no maintenance note: Task 8. Covered.
- Writing style constraint (plain ASCII): Global Constraints plus grep check in Task 8 Step 2 and the #338 comment in Task 11. Covered.
- Issue #338 follow up: Task 11. Covered.
- MooshieUI not modified: stated in Global Constraints; no task touches the MooshieUI tree except reading the source file and posting the issue comment. Covered.
- GitHub repo via gh: Task 1. Covered.

**Placeholder scan:** No TBD or vague steps. Every file has full content. License is fetched, not pasted as a placeholder, with an offline fallback.

**Type consistency:** `comfy_entrypoint` name is consistent across Tasks 2, 3. Registry id `comfyui-mooshietileddiffusion`, PublisherId `mooshieblob`, secret `REGISTRY_ACCESS_TOKEN`, and DisplayName `Mooshie Tiled Diffusion` are consistent across Tasks 4, 8, 9, 10, 11.

**Note on TDD:** There is no unit test framework for this node (matching MooshieUI, which has none). Verification uses syntax checks, TOML/YAML parse checks, a punctuation grep, and the live registry publish run. Final functional validation is loading the node in ComfyUI, which only the user can do in a real ComfyUI environment.
