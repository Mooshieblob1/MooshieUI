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
| 2 | Completion notification (OS notification plus optional chime when a generation finishes while the window is unfocused) | S | Tauri notification plugin on desktop, Web Notifications API in browser mode. Pure quality of life, no new backend surface. Implemented (see below). |
| 3 | Random prompt syntax (`{a\|b\|c}`, `{2$$a\|b\|c}`, weights, nesting) | S-M | **Implemented.** See `docs/RANDOM_PROMPTS.md`. Expander lives in `src/lib/utils/randomPrompt.ts` and runs in `toParams()` before the workflow is built. |
| 4 | LoRA trigger words (pulled from CivitAI metadata, shown on LoRA cards, one-click or automatic insert) | M | Reuses the existing SHA256 to CivitAI lookup that already backs architecture detection. Model-hub tooling, in scope. |
| 5 | Bulk CivitAI metadata scan (hash-scan local models to fetch previews, metadata, and trigger words in one batch) | M | **Implemented.** `civitai_bulk_scan` / `civitai_bulk_scan_cancel` Rust commands walk all model categories (loras, checkpoints, diffusion_models, embeddings), hash each file, look it up via the CivitAI API with 429 backoff, and write a `.civitai.info` sidecar. Progress is delivered via `comfyui:civitai_scan` SSE/Tauri events. A compact progress row with cancel button lives in the LoRA gallery header. |
| 6 | TeaCache / EasyCache and torch.compile toggles | M | Sits next to the existing SageAttention and FlashAttention settings, gated per family (TeaCache for Flux-class, EasyCache for Qwen). The custom-node auto-install pattern already exists (ComfyUI-GGUF is the precedent). |
| 7 | Generation queue panel (queue jobs with different settings, view and cancel pending items) | M-L | **Implemented.** Queue toggle button in GenerateButton shows a popover listing running and pending items with cancel, move-up, move-down, and move-to-front actions. Rust `reorder_queue_item` command re-holds submitted prompts so the drain reactor resubmits them in the new order. |
| 8 | Outpainting (extend the canvas and generate into the new space) | M-L | The canvas and mask editor and the inpaint pipeline already exist. Needs a canvas-extend UX and a pad/feather workflow (ComfyUI `ImagePadForOutpaint`). Stays inside the existing in-app editing features. |
| 9 | Reference-image prompting (IP-Adapter or Flux Redux style transfer for non-edit models) | L | **Implemented.** Redux is a core node for Flux; IP-Adapter for SDXL-class needs a custom node. See Style Reference section in GenerationPage, `src-tauri/src/templates/style_ref.rs`, and `docs/STYLE_REFERENCE.md`. |
| 10 | Video generation (Wan 2.2 T2V and I2V first, LTX-V later) | XL | The Wan family is already detected. Needs video workflow templates, frame/FPS/length params, video output handling outside the JXL pipeline, gallery playback, and previews. The biggest lift, best split into phases and scheduled last. |

## Explicitly out of scope

These came up in the survey and were rejected on charter or fit grounds, so they
are recorded here to avoid re-litigating them: axis-driven XY plot grids
(node-graph territory, and deselected during planning), audio generation, 3D
generation, multi-backend or multi-GPU orchestration, webhooks, closed-model
API nodes, and an App Builder or extension marketplace.

## Completion notifications (implemented)

Item #2 is now shipped. The app shows OS-level (system tray) notifications when
a generation or video finishes, using `tauri-plugin-notification` on desktop and
the Web Notifications API in browser mode. A new Notifications section in
Settings lets users enable the feature and optionally restrict it to fire only
when the app window is not focused. The setting defaults to off; turning it on
triggers the platform permission request inline. OS notifications fire for:
image batches done, video done, and generation errors. The implementation lives
in `src/lib/utils/osNotify.ts` with hook points in `src/App.svelte`.

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

Items #1 and #9 in the table are implemented. Everything else is still a proposal, not committed work.

## Style reference (item #9, implemented)

Style reference lets users guide the style of any generation by supplying a reference image. The implementation adapts to the active model family automatically:

- Flux.1 (flux1d, flux1s, flux1krea): uses Flux Redux, a core ComfyUI workflow with no custom nodes required. Needs `flux1-redux-dev.safetensors` in `models/style_models/` and `sigclip_vision_patch14_384.safetensors` in `models/clip_vision/`.
- SD1.5 (sd15): uses IP-Adapter Plus (ComfyUI_IPAdapter_plus custom node pack, installed lazily from the panel). Needs `ip-adapter-plus_sd15.safetensors` in `models/ipadapter/` and `CLIP-ViT-H-14-laion2B-s32B-b79K.safetensors` in `models/clip_vision/`.
- SDXL-class (sdxl, illustrious, pony): uses IP-Adapter Plus with `ip-adapter-plus_sdxl_vit-h.safetensors` in `models/ipadapter/`.
- All other families (Wan, Qwen, SD3, Chroma, etc.): not supported; the panel shows an explanatory hint and generation is blocked with a clear error.

The Style Reference panel appears on the right column of the Generation page. It can be shown, hidden, and dragged like all other generation sections. See `docs/STYLE_REFERENCE.md` for model download instructions.
