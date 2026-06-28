# M2 — Parameter Correctness & Control

**Date:** 2026-06-17
**Status:** Approved (design)
**Topic:** Fix the Illustrious CFG=1 instability, correct Illustrious defaults, add a loud CFG=1 warning, and add an opt-in Advanced Mode that stops auto-resetting generation params on checkpoint change.

## Purpose

Everyone using Illustrious (a major model family) currently gets either broken output at CFG=1 or a bad-habit default that runs a CFG++ sampler far outside its band. This milestone makes Illustrious generate stably at the correct low CFG, makes CFG=1 impossible to footgun silently, and gives power users a way to stop the app from auto-tuning params when they swap checkpoints.

This is milestone M2 of the user-feedback roadmap (`docs/superpowers/specs/2026-06-17-feedback-milestones-roadmap-design.md`). It is independently releasable.

## Root-cause findings (from codebase investigation)

These reshape the milestone and are baked into the design below:

- **The guidance nodes are NOT the cause.** `MooshieSmartGuidance` is an opt-in toggle, off by default (`generation.svelte.ts:303` `smartGuidance = $state(false)`), injected only when `params.smart_guidance == true` (`src-tauri/src/templates/mod.rs:334`). `MooshieSoftGuidance` runs only in the optional upscale pass (`src-tauri/src/templates/upscale.rs:127`). Both are numerically stable at CFG=1 (no divide-by-`(cfg-1)`; std is clamped to `1e-8`). Neither is auto-attached for Illustrious.
- **The cause is `euler_cfg_pp` at CFG=1.** The main generation path is stock ComfyUI `KSampler` passing `cfg` verbatim (`src-tauri/src/templates/txt2img.rs:133`). CFG++ samplers steer with a `(cfg − 1) · (cond − uncond)` term; at CFG=1 that term is zero, leaving pure unconditional stepping, so the prompt has no influence and output is garbage.
- **The current Illustrious default is internally inconsistent.** The non-turbo preset pairs `euler_cfg_pp` with CFG 5.0 (`generation.svelte.ts:1069-1078`), but CFG++ samplers are designed for ~1–2.2 (`SamplerSettings.svelte:33`). Running a CFG++ sampler at 5.0 over-bakes output. The roadmap's "recommend 1.8–2.2" fix is about aligning CFG to the sampler.
- **The existing CFG warning is weak and misleading.** It is a tiny amber text label + "Fix" button, and its recommended floor for cfg_pp is `1.0` (`SamplerSettings.svelte:32-43`) — so the UI currently treats the exact value that breaks as "in range".
- **No Advanced/lock-params concept exists.** The only guard is the `modelPresetAppliedKey` idempotency check in `applyModelSpecificPreset()` (`generation.svelte.ts:867`), which still re-applies on every *new* model.
- **The sampler list is dynamic.** Available samplers are enumerated from the ComfyUI backend into `models.samplers` (`src/lib/stores/models.svelte.ts:40,108`). `euler_ancestral_cfg_pp` is standard in modern ComfyUI but may be absent on older builds — the preset must fall back gracefully.

## Design

### Part 1 — Fix the Illustrious non-turbo preset

In `applyModelSpecificPreset()` (`src/lib/stores/generation.svelte.ts`), change only the `case "illustrious"` non-turbo branch (currently lines 1069-1078):

| Field | Today | New |
|-------|-------|-----|
| samplerName | `euler_cfg_pp` | `euler_ancestral_cfg_pp` |
| cfg | `5.0` | `2.0` |
| scheduler | `sgm_uniform` | `sgm_uniform` (unchanged) |
| steps | `20` | `20` (unchanged) |
| width × height | 1024 × 1024 | 1024 × 1024 (unchanged) |

- Turbo Illustrious (`euler` @ CFG 1.0, scheduler `normal`, 10 steps) is **untouched**.
- **Isolation:** only the `illustrious` case changes. The `sdxl`/`mugen`/`unknown`/default block keeps `euler_cfg_pp` @ 5.0; Anima/Wan/Qwen keep `er_sde` @ 4.0; every other family is unchanged.
- **Sampler-availability fallback:** if the chosen sampler is not present in `models.samplers`, fall back to `euler_ancestral` (a universally-present stock sampler) so the preset never sets an invalid `sampler_name` on older ComfyUI builds. This fallback is applied to the value the preset assigns to `this.samplerName`, not to the literal in the switch.

### Part 2 — CFG=1 loud warning + fix the misleading range

Two changes in `src/lib/components/generation/SamplerSettings.svelte`:

1. **Raise the cfg_pp recommended floor.** In `recommendedCfgRange()` (line 33), change the cfg_pp branch from `{ min: 1.0, max: 2.2, target: 1.4 }` to `{ min: 1.5, max: 2.2, target: 1.8 }`. The standard-sampler branch (`{ min: 4.0, max: 8.0, target: 6.0 }`) is unchanged. This makes CFG=1 read as out-of-range for CFG++ samplers and aligns the "Fix" target with the new Illustrious default.
2. **Add a prominent warning box** that renders when `generation.cfg <= 1.0`. It is visually distinct from the existing small amber "recommended range" row: an alert box with a warning icon and a bold heading, using the dark-neutral palette with an amber/red accent border (Tailwind only, no `<style>` block). Copy explains: *CFG=1 disables prompt guidance — it only produces good results on Turbo / distilled / Lightning models, and it breaks CFG++ samplers entirely.* The box is **non-blocking**: generation still proceeds.

The existing small amber recommended-range row and "Fix" button (lines 230-245) remain for the milder out-of-range case.

**i18n:** new keys for the warning heading and body added to `src/lib/locales/en.ts` and to all 11 other locale files with matching `{placeholder}` names (English fallback text for non-English locales is acceptable per existing repo precedent). Keys to add (illustrative names): `generation.sampler.cfg1_warning_title`, `generation.sampler.cfg1_warning_body`.

### Part 3 — Advanced Mode toggle

- **New persisted flag:** `advancedMode = $state(false)` declared on `GenerationStore`, included in the `saveSettings()` `ipcStore.set(...)` object, and restored in `loadSettings()` with the `if (saved.advancedMode !== undefined) this.advancedMode = saved.advancedMode;` guard (the standard boolean-settings pattern; restores `false` correctly).
- **Gate inside `applyModelSpecificPreset()`:** when `advancedMode` is on, the method still runs family detection and sets `modelPresetAppliedKey` plus all metadata the templates depend on (`modelFamily`, `modelIsSdxlLike`, `modelTurboVariant`, 16-channel-latent / vpred flags) — but it **skips writing** the user-facing generation params: `steps`, `cfg`, `samplerName`, `scheduler`, `width`, `height`. Swapping checkpoints therefore preserves all generation params.
- **First-selection exception:** if no preset has ever been applied this session/profile (`modelPresetAppliedKey` is empty/initial at entry), apply the preset once even in Advanced Mode, so a fresh user is not left with nonsense defaults. Every subsequent swap preserves.
- **UI:** a labeled toggle in the Settings page (generation settings section). Enabling it shows a confirmation dialog explaining params will no longer auto-tune per model; confirming sets the flag and calls `saveSettings()`. Disabling is silent (sets flag, saves). Re-enabling does not retroactively re-apply a preset.

### Part 4 — Upscale CFG floor for CFG++ samplers

The upscale pass halves CFG (`cfg: params.cfg / 2.0`, `src-tauri/src/templates/upscale.rs:204`) and inherits the base sampler. With the new Illustrious default (CFG 2.0 + a CFG++ sampler), the upscale KSampler would run at CFG 1.0 with `euler_ancestral_cfg_pp` — the exact collapse this milestone fixes.

Fix: when the sampler name contains `cfg_pp`, clamp the upscale CFG to a floor of `2.0` — i.e. `max(params.cfg / 2.0, 2.0)` for CFG++ samplers; non-CFG++ samplers keep the existing `params.cfg / 2.0`. The inpainting template already detects cfg_pp samplers (`src-tauri/src/templates/inpainting.rs:113` `is_cfgpp_sampler`), so the detection idiom exists in the codebase to mirror.

## Components & touchpoints

| File | Change |
|------|--------|
| `src/lib/stores/generation.svelte.ts` | Illustrious preset (Part 1); sampler-availability fallback; `advancedMode` flag + save/load (Part 3); gate in `applyModelSpecificPreset()` (Part 3) |
| `src/lib/components/generation/SamplerSettings.svelte` | cfg_pp recommended range (Part 2.1); prominent CFG=1 warning box (Part 2.2) |
| Settings page (`src/lib/components/.../SettingsPage` or equivalent) | Advanced Mode toggle + confirmation dialog (Part 3) |
| `src/lib/locales/en.ts` + 11 other locales | new warning i18n keys with placeholder parity (Part 2) |
| `src-tauri/src/templates/upscale.rs` | CFG floor for cfg_pp samplers (Part 4) |

No new Tauri commands. No changes to the guidance Python nodes.

## Validation

No test framework exists. Validation is `npm run build` + `cargo check --manifest-path src-tauri/Cargo.toml`, plus manual verification of each "Done when":

- Selecting a non-turbo Illustrious checkpoint sets sampler `euler_ancestral_cfg_pp` (or `euler_ancestral` fallback) and CFG 2.0, and generates clean output.
- Dragging CFG to 1.0 shows the prominent warning box; cfg_pp recommended range reads `1.5-2.2`; generation still works.
- With Advanced Mode on, set custom CFG/sampler/steps/dimensions, swap checkpoints, and confirm those params are preserved (family-dependent template behavior still correct).
- With Advanced Mode off, swapping checkpoints still auto-applies per-model presets as before.
- Enabling upscale with the new Illustrious default does not produce a broken upscale pass (upscale CFG floored at 2.0 for cfg_pp).

## Out of scope / parked

- Changing any other model family's defaults.
- Reworking or removing the guidance nodes (exonerated by investigation).
- A general per-parameter "lock" UI beyond the single Advanced Mode flag.
- Surfacing Advanced Mode in the sampler panel (decided: Settings page only for this milestone).

## Delivery model

Single spec → writing-plans → implementation cycle, executed via subagent-driven-development. Independently releasable as a stability/correctness release.
