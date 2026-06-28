# User-Feedback Milestone Roadmap

**Date:** 2026-06-17
**Status:** Approved (design)
**Topic:** Decompose a 16-item user-feedback backlog into six independently shippable MVP milestones.

## Purpose

A batch of user feedback (bugs, sampler issues, UX gaps, feature requests) needs to be turned into a sequence of scopeable, independently releasable milestones. Each milestone is a coherent release that visibly improves the product. Sequencing is by **breadth x severity**: how many users are blocked, and how badly.

Each milestone gets its own spec -> plan -> implementation cycle. This document is the roadmap, not the per-milestone design.

## Reality-checks from codebase exploration

These findings reshape several feedback items and are baked into the milestones below:

- **There is no custom KSampler.** All templates use ComfyUI's stock `KSampler` (`src-tauri/src/templates/txt2img.rs:133`). The "blows up at CFG 1" report is therefore mis-attributed. Real suspects: the `euler_cfg_pp` sampler the Illustrious preset selects (`src/lib/stores/generation.svelte.ts:1068`), or the custom model-patching guidance nodes `MooshieSmartGuidance` / `MooshieSoftGuidance` (`comfyui-nodes/nodes_guidance.py`).
- **Most LoRA infrastructure already exists** - types, store methods, presets, gallery, template chain, pre-gen validation (`src/lib/stores/loraPresets.svelte.ts`, `src-tauri/src/templates/mod.rs:253`, `src-tauri/src/commands/api.rs:4150`). The only real gap is **installation** (getting a LoRA file into the folder).
- **The Enhance 524 is a Cloudflare edge timeout (~100s)** in hosted/browser mode, firing before the local 120s chat timeout (`src-tauri/src/prompt_assistant/server.rs:323`). The fix is architectural (streaming or async job), not a timeout bump.
- **No arrow-key "inspectable object" viewer exists** - only a flat text `TerminalLog.svelte`. The reported "dynamic UI navigation" item was clarified to mean **gallery image navigation** (arrows move selection and the metadata/info panel updates live).
- **Gen-time output**: a progress bar with step count + elapsed time already exists (`src/lib/components/progress/ProgressBar.svelte`). The clarified gap is a **persistent final timing summary** on the result, not the transient counter.
- **Per-model defaults** live in `applyModelSpecificPreset()` (`src/lib/stores/generation.svelte.ts:854-1108`); the checkpoint-change auto-reset is the `$effect` in `src/lib/components/generation/ModelSelector.svelte:465`. CFG warning/recommendation infra is in `src/lib/components/generation/SamplerSettings.svelte:32`.

## Milestone sequence

### M1 - P0: Won't-generate blockers

Everyone affected; these stop users cold. Ship as a stability hotfix.

- **Missing CLIP on default model.** Auto-pick grabs `checkpoints[0]` blindly (`generation.svelte.ts:1809`); a UNET-only/split file makes `CheckpointLoaderSimple` yield `clip: None` -> `RuntimeError: clip input is invalid: None`. Validate the auto-picked default has CLIP, or skip/repair non-checkpoint files.
- **Generation-start pop-up.** Investigate; likely the same root cause surfacing as a toast/`errorMsg` in `GenerateButton.svelte:347`.
- **Swarm import / `<segment:>` pop-up.** Investigate the parse error when importing a SwarmUI image containing segment syntax (`src/lib/utils/promptSegmentDetail.ts`, `src/lib/utils/metadataImport.ts`).

**Done when:** the default model generates clean; a normal gen-start raises no error toast; importing a SwarmUI image with segments does not pop an error.

### M2 - Parameter correctness & control

Everyone using Illustrious (a major model family) gets broken output or bad-habit defaults. Concentrated in the generation store + SamplerSettings.

- **Diagnose & fix CFG-1 instability.** Determine whether the `euler_cfg_pp` sampler or the guidance-patch nodes cause the breakdown at cfg=1 for Illustrious, and align behavior with stock KSampler.
- **Recommended CFG for Illustrious** -> 1.8-2.2 with Euler Ancestral, **isolated to Illustrious** so Anima and other families keep their own defaults (`applyModelSpecificPreset()`).
- **CFG=1 heavy warning.** Extend the existing amber warning infra (`SamplerSettings.svelte:32`) into a loud, hard-to-miss warning explaining CFG=1 only works with specific models.
- **Advanced Mode toggle.** Opt-in flag (with confirmation warnings) that stops `applyModelSpecificPreset()` from resetting CFG/dimensions on checkpoint change (`ModelSelector.svelte:465`).

**Done when:** Illustrious generates stably at low CFG with correct defaults; CFG=1 shows a prominent warning; Advanced Mode preserves user-set params across checkpoint swaps.

### M3 - Prompt editor UX

Daily friction for everyone; localized to `PromptTextarea.svelte` plus one action file.

- **Autocomplete overwrite bug.** Off-by-one in the replacement `end` boundary when inserting a tag mid-prompt overwrites the following tag (`PromptTextarea.svelte:104` `getCurrentTagFragment`, `:219` `acceptSuggestion`).
- **Tag editor cursor focus.** First click places the caret inside the tag for immediate editing instead of selecting the whole span, while retaining the highlight state (`PromptTextarea.svelte:500`).
- **Prompt box scroll-wheel lock.** The `wheelScrollLock` action exists but textareas are not detected as scrollable until overflow (`src/lib/utils/wheelScrollLock.ts:19`); ensure a focused prompt box retains focus and scroll does not bleed between the positive/negative boxes.

**Done when:** mid-prompt autocomplete preserves the next tag; a single click edits a tag; scrolling a focused prompt box does not interfere with the other.

### M4 - Display quick wins

Low-risk, mostly one-file changes. A satisfying polish release.

- **Megapixel display.** Add an MP figure alongside `W x H` (`DimensionControls.svelte:339`).
- **Gen-time final timing summary.** Persist total generation time on the finished result (chosen over the existing transient elapsed counter).
- **Gallery arrow-key live navigation.** Arrow keys move gallery selection and the metadata/info panel updates live to match the selected image.

**Done when:** resolution shows megapixels; finished generations display total time; arrowing through the gallery updates the info pane.

### M5 - Enhance reliability (hosted)

The Enhance feature is fully broken on the hosted/browser deployment (gpu.garden).

- **Enhance 524.** Cloudflare's ~100s edge timeout fires before the local 120s chat timeout (`server.rs:323`) and before/within a cold llama-server start (180s health deadline). Fix is architectural: stream the response (SSE) or convert Enhance to an async job + poll, so no single proxied request exceeds Cloudflare's limit.

**Done when:** Enhance completes on the hosted deployment without a 524, including on a cold llama-server start.

### M6 - Model & LoRA management

Largest effort; feature expansion, so it ships last.

- **Model management UI clarity.** Disambiguate look-alike entries in the delete flow (`ModelManagerModal.svelte:413`): truncated filenames make models indistinguishable. Surface family / directory / size / hash so users know exactly what they are deleting.
- **LoRA installation (URL/ID download).** Apply, presets, gallery, and validation already exist. Add installation: paste a CivitAI or HuggingFace URL / model-ID and download the LoRA into the loras folder in-app, reusing the existing CivitAI hash + metadata lookup (`src-tauri/src/commands/api.rs:4172` `get_lora_civitai_info`). Tested against a real target such as `anima-turbo-lora`.

**Done when:** users can tell models apart when deleting; users can install a new LoRA from a URL/ID inside the app and apply it to a generation.

## Out of scope / parked

- **Arrow-key navigation for error logs / arbitrary inspectable objects** beyond gallery images. The original report was too vague; only gallery navigation is in scope (M4). Revisit if a reporter can demo the intended behavior.
- **Local-file LoRA install** and a generic in-app model browser/search. M6 covers URL/ID download only.

## Per-milestone delivery model

Each milestone is independently releasable and follows its own spec -> writing-plans -> implementation cycle. M1 and M5 begin with an investigation step (root-cause confirmation) before fixes are scoped. Validation per milestone is `npm run build` + `cargo check` (no test framework exists), plus manual verification of the "Done when" criteria.
