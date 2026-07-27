# MooshieUI Feature Roadmap (2026 gap analysis)

This roadmap comes out of a survey of ComfyUI (core plus the common ecosystem
nodes, mid-2026) and SwarmUI (v0.9.8 and later, through mid-2026), compared
against what MooshieUI ships today. Everything here is filtered through
SCOPE.md: MooshieUI is a polished desktop and browser front-end over ComfyUI
generation workflows for a single user, not a node-graph editor, not a hosted
SaaS, not a training tool, not a general image editor, and not a plugin
marketplace. Features that only make sense in one of those out-of-scope
products were dropped before this list, not ranked low on it.

## Where MooshieUI already stands

MooshieUI is already competitive on the core generation experience. Prompt
tooling (weights, scheduling, regional conditioning, segment prompts,
wildcards), face detailer, tiled upscale with the custom guidance nodes,
CivitAI import, GGUF support, the compare grid, boards, interrogate, LAN mode,
and full i18n are all in place. The gaps below are mostly newer model families
and quality-of-life features that the two reference projects have and we do
not, chosen where they fit the charter.

## Prioritized roadmap

Ordered by value over effort. Effort key: S is under a day, M is one to three
days, L is one to two weeks, XL is multi-week.

| # | Feature | Effort | Charter fit and notes |
|---|---------|--------|-----------------------|
| 1 | Image Edit mode (Qwen Image Edit / Edit Plus, Flux.1 Kontext) | L | Instruction-driven editing of one or more reference images. Model families are already detected, and this is squarely "a polished UI over generation workflows." Implemented first (see below). |
| 2 | Completion notification (OS notification plus optional chime when a generation finishes while the window is unfocused) | S | Tauri notification plugin on desktop, Web Notifications API in browser mode. Pure quality of life, no new backend surface. |
| 3 | Random prompt syntax (`<random:a,b,c>`, `<random[n]:...>`, and optionally `<alternate:...>` later) | S-M | Extends the existing prompt translation layer that already handles wildcards and scheduling. In scope as prompt-building tooling. |
| 4 | LoRA trigger words (pulled from CivitAI metadata, shown on LoRA cards, one-click or automatic insert) | M | Reuses the existing SHA256 to CivitAI lookup that already backs architecture detection. Model-hub tooling, in scope. |
| 5 | Bulk CivitAI metadata scan (hash-scan local models to fetch previews, metadata, and trigger words in one batch) | M | Same CivitAI plumbing as #4, run as a batch job with a progress UI. Best built together with #4. |
| 6 | TeaCache / EasyCache and torch.compile toggles | M | Sits next to the existing SageAttention and FlashAttention settings, gated per family (TeaCache for Flux-class, EasyCache for Qwen). The custom-node auto-install pattern already exists (ComfyUI-GGUF is the precedent). |
| 7 | Generation queue panel (queue jobs with different settings, view and cancel pending items) | M-L | The ComfyUI backend queue already supports this. Needs a per-job settings snapshot (the Compare store's snapshot logic is the precedent) plus a queue UI. |
| 8 | Outpainting (extend the canvas and generate into the new space) | M-L | The canvas and mask editor and the inpaint pipeline already exist. Needs a canvas-extend UX and a pad/feather workflow (ComfyUI `ImagePadForOutpaint`). Stays inside the existing in-app editing features. |
| 9 | Reference-image prompting (IP-Adapter or Flux Redux style transfer for non-edit models) | L | Redux is a core node for Flux; IP-Adapter for SDXL-class needs a custom node. Best done after Image Edit mode since it shares the reference-image UI. |
| 10 | Video generation (Wan 2.2 T2V and I2V first, LTX-V later) | XL | The Wan family is already detected. Needs video workflow templates, frame/FPS/length params, video output handling outside the JXL pipeline, gallery playback, and previews. The biggest lift, best split into phases and scheduled last. |

## Explicitly out of scope

These came up in the survey and were rejected on charter or fit grounds, so they
are recorded here to avoid re-litigating them: axis-driven XY plot grids
(node-graph territory, and deselected during planning), audio generation, 3D
generation, multi-backend or multi-GPU orchestration, webhooks, closed-model
API nodes, and an App Builder or extension marketplace.

## Image Edit mode (implemented)

Image Edit mode is item #1 and is the first feature off this roadmap. It adds a
new `image_edit` generation mode supporting three families, all using core
ComfyUI nodes with no custom-node install:

- Qwen Image Edit (single reference image)
- Qwen Image Edit Plus (up to three reference images)
- Flux.1 Kontext dev (single reference image)

The mode reuses the existing model-family detection, the shared upload path that
works in both desktop and browser mode, and the standard steps/CFG/sampler
settings. It slots in as a fourth mode tab next to txt2img, img2img, and
inpainting, with its own reference-image section that only appears for edit
models and warns when a non-edit model is selected. See the family-specific
workflow templates in `src-tauri/src/templates/image_edit.rs` and the settings
UI in `src/lib/components/generation/ImageEditSettings.svelte`.

Everything past item #1 in the table is still a proposal, not committed work.
