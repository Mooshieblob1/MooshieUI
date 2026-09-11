# MooshieUI Feature Roadmap (2026 gap analysis)

This document retains the original gap-analysis topics and records their status
against **v2.3.1**. Proposals are not release commitments. The current project
charter is [SCOPE.md](../SCOPE.md); user instructions live in the
[wiki](https://github.com/Mooshieblob1/MooshieUI/wiki).

## Status of the original roadmap

| # | Topic | Current status |
|---|-------|----------------|
| 1 | Image Edit mode | **Implemented.** Qwen Image Edit, Qwen Image Edit Plus, Flux.1 Kontext and Anima ReStyler. |
| 2 | Completion notifications | **Implemented.** Desktop/Web Notifications for image batches, videos and errors, with an optional unfocused-window restriction. The original optional chime is not a documented control. |
| 3 | Random prompt syntax | **Implemented.** Alternation, multiple choices, weights and nesting, resolved using the generation seed. See [RANDOM_PROMPTS.md](RANDOM_PROMPTS.md). |
| 4 | LoRA trigger words | **Implemented.** Trigger-word chips insert and highlight tags; disabling or removing the LoRA removes the words it inserted. |
| 5 | Bulk CivitAI metadata scan | **Implemented.** Hashes local LoRAs, checkpoints, diffusion models and embeddings; saves sidecars with progress, cancellation and rate-limit backoff. |
| 6 | Cache/compile controls | **Partially implemented.** Anima image sampling and H3 video offer TeaCache. Broader EasyCache/torch.compile controls remain proposals. |
| 7 | Generation queue panel | **Implemented.** Inspect running/pending jobs, reorder or cancel pending items, interrupt a run and clear pending work. |
| 8 | Dedicated outpainting workflow | **Proposal.** Inpainting and a canvas editor exist; a dedicated canvas-extension/pad-and-feather workflow remains separate work. |
| 9 | Reference-image prompting | **Implemented for selected families.** Flux.1 Redux and SD1.5/SDXL-class IP-Adapter Plus. See [STYLE_REFERENCE.md](STYLE_REFERENCE.md). |
| 10 | Video generation | **Implemented with MiniMax H3.** Text-to-video, first/last frames, reference images, shot timeline, preset/custom stacks, Turbo, TeaCache, interpolation, playback and export. The original Wan 2.2/LTX-V proposal is not the shipped video workflow. |

## Features shipped beyond the original list

- **NovelAI image backend:** V5 Full/Curated, V4.5 Full and V4 Full; characters,
  positioning, supported references, cost estimates, Director Tools,
  Enhance/Upscale/Variations, and dedicated face detailing. See [NOVELAI.md](NOVELAI.md).
- **Personal NovelAI credentials:** encrypted keys for hosted users and
  moderators, separate from the desktop/admin owner's key.
- **Pause and continue:** ComfyUI text-to-image sampling can pause for inspection,
  parameter changes and masked corrections, then continue or try another ending.
- **Style Creator:** compare artist combinations on a shared seed and save
  selections as Artist Styles with generated thumbnails.
- **Additional model/refinement tools:** GGUF and optional INT8-Fast loaders,
  DMD2, RescaleCFG/NAG/APG, SeedVR2 and GMFSS interpolation.
- **Gallery and usability:** manual image/video saving, A/B comparison,
  generation-time display, gallery retention controls, categorized Settings,
  custom tagger folders and 12 UI languages.
- **Apple Silicon candidates:** native builds and managed Metal runtime checks;
  physical-Mac generation, installation and update qualification remain pending.
  See [MACOS.md](MACOS.md).

## Implementation references

| Area | Main source |
|------|-------------|
| Image Edit | `src-tauri/src/templates/image_edit.rs`, `src/lib/components/generation/ImageEditSettings.svelte` |
| Notifications | `src/lib/utils/osNotify.ts`, `src/App.svelte` |
| Random prompts | `src/lib/utils/randomPrompt.ts`, `src/lib/stores/generation.svelte.ts` |
| Style Reference | `src-tauri/src/templates/style_ref.rs` |
| Video | `src-tauri/src/templates/video.rs`, `src/lib/components/video/` |
| NovelAI | `src-tauri/src/novelai/`, `src-tauri/src/user_secrets.rs` |

The historical exclusion list is superseded by the current charter and shipped
features. In particular, Compare Grid, NovelAI, configured GPU workers and video
with generated audio already exist; they must not be described as absent solely
because the original survey excluded them. New proposals still need scope review.
