# ComfyUI-MooshieTiledDiffusion: Standalone Node Export

Date: 2026-06-21
Issue: #338 (Release ApplyTiledDiffusion as a standalone ComfyUI node)

## Goal

Publish MooshieUI's tiled diffusion node as a standalone ComfyUI custom node so
users who run plain ComfyUI can install it. The existing third party node
(shiimizu/ComfyUI-TiledDiffusion) does not support Anima (COSMOS based) models,
and upscaling above 2.4 MP still produces artifacts without tiling. MooshieUI's
node handles 5D latents and works across all generation modes, which is what the
requester wants.

This is a one off export. There is no commitment to ongoing maintenance.

## Source of truth

MooshieUI stays canonical. The node implementation continues to live at
`comfyui-nodes/nodes_tiled_diffusion.py` and continues to be embedded into the
app via `include_str!` at `src-tauri/src/setup.rs` (around line 1103). That code
path is not modified by this work, so the app is unaffected.

The standalone repo vendors a byte for byte copy of the canonical file. The copy
relationship is one way and manual. A `SYNC.md` file in the new repo records the
upstream path and the source commit hash so any future re-sync is documented
rather than guessed. No submodule, no automation.

## What the node is

`ApplyTiledDiffusion` patches a model's UNet function wrapper to denoise in
overlapping or shifted tiles, then blends the tiles back together.

- MultiDiffusion: overlapping tiles with cosine feathered blending (best quality).
- SpotDiffusion: random tile shift per denoising step, no overlap (fastest).
- Supports 4D (B,C,H,W) and 5D (B,C,T,H,W) latents, so it works with Anima/COSMOS.
- Works in txt2img, img2img, and inpainting, and crops spatial conditioning
  (c_concat, ControlNet outputs) per tile.
- Uses the ComfyUI V3 extension API (`comfy_api.latest`, `ComfyExtension`,
  `comfy_entrypoint`). It self registers, no manual NODE_CLASS_MAPPINGS.
- Only depends on torch and comfy_api, both provided by ComfyUI. No extra pip deps.
- GPL-3.0, because the file header derives from ComfyUI which is GPL-3.0.

## Repo layout

Repo name: `ComfyUI-MooshieTiledDiffusion`, public, under the Mooshieblob1
account, created via `gh repo create`.

```text
ComfyUI-MooshieTiledDiffusion/
  __init__.py                    # from .nodes_tiled_diffusion import comfy_entrypoint
  nodes_tiled_diffusion.py       # byte for byte copy of the canonical file
  pyproject.toml                 # [project] + [tool.comfy] metadata for the registry
  LICENSE                        # GPL-3.0
  README.md                      # purpose, install, usage, credits, no maintenance note
  SYNC.md                        # upstream path and source commit
  .gitignore                     # __pycache__/, *.pyc
  .github/workflows/publish.yml  # publish to Comfy Registry
```

### Package loading detail

MooshieUI installs the file as a single top level `.py` in `custom_nodes/`. A git
clone is a folder, so ComfyUI loads it as a package and imports `__init__.py`.
The `__init__.py` re-exports `comfy_entrypoint` from the implementation module so
the V3 extension is discovered. The implementation file itself is unchanged from
the canonical copy.

### pyproject.toml

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

## Registry publishing

Use the official `Comfy-Org/publish-node-action`. Documented trigger is a push to
`main` that changes `pyproject.toml`. Cutting a new version means bumping the
`version` field and committing.

The workflow needs one repo secret, `REGISTRY_ACCESS_TOKEN`, which is an API key
generated at registry.comfy.org. The user adds this secret manually. The README
documents the steps. First publish happens automatically once the secret exists
and the workflow runs on main.

```yaml
name: Publish to Comfy Registry
on:
  push:
    branches: [main]
    paths: ["pyproject.toml"]
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: Comfy-Org/publish-node-action@main
        with:
          personal_access_token: ${{ secrets.REGISTRY_ACCESS_TOKEN }}
```

## README content

Plain ASCII, factual, no decorative punctuation. Sections:

- One line statement that this is a standalone export of MooshieUI's tiled
  diffusion node, published by request (issue #338), provided as is with no
  promise of maintenance or support.
- What it does: MultiDiffusion and SpotDiffusion, Anima/COSMOS support, all modes.
- Install: ComfyUI-Manager, git clone into custom_nodes, or Comfy Registry.
- Usage: connect the node MODEL output to a KSampler, set tile width/height to the
  model native resolution, set overlap. Note the above 2.4 MP artifact context.
- MultiDiffusion vs SpotDiffusion, when to use each.
- Credits: ComfyUI (GPL-3.0 origin), MultiDiffusion paper (Bar-Tal et al., 2023),
  SpotDiffusion paper (Ding et al., 2024).
- License: GPL-3.0.

## Writing style constraint

All authored content (README, repo description, commit messages, the #338
comment) is plain ASCII. No em dashes, no en dashes, no curly quotes or
apostrophes, no emojis, no rhetorical flourish. Short factual sentences.

## Issue #338 follow up

After the repo is live and the first registry publish succeeds, post a comment on
issue #338 in the user's voice announcing the repo and how to install it, then
the user decides whether to close the issue. Comment follows the writing style
constraint above.

## Out of scope

- No change to MooshieUI's embedded node or setup.rs.
- No automated sync between the two copies.
- No new node features. This is an export of existing behavior.

## Steps overview

1. Create the public GitHub repo via `gh repo create`.
2. Scaffold the repo files locally (copy of the node plus the scaffolding above).
3. Commit and push to main.
4. User adds the `REGISTRY_ACCESS_TOKEN` secret.
5. Confirm the publish workflow runs and the node appears on the registry.
6. Post the #338 comment.
