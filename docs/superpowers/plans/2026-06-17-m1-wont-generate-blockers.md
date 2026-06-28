# M1 — Won't-Generate Blockers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate three independent P0 issues that block or alarm users at generation start: the `clip input is invalid: None` crash on diffusion-only models, a spurious regional-prompt warning toast that fires on most generations, and SwarmUI segment tags being rejected/left literal on image import.

**Architecture:** All three fixes are frontend-only (TypeScript/Svelte). Bug A adds a defensive guard in the generation store that converts a cryptic ComfyUI crash into an actionable error, using the already-resolved `modelFamily` signal — no Rust change. Bug B tightens one toast condition. Bug C relaxes the segment-spec numeric bounds and clamps, so SwarmUI edge values import as valid segments instead of broken literal text. The three are independent and each commits separately.

**Tech Stack:** Svelte 5 runes, TypeScript, Tailwind. No test framework exists in this repo — validation per task is `npm run build` (the project's pre-commit gate) plus a manual reproduction check. Prefix git commands with `git -c core.hooksPath=/dev/null` on Windows (the bash pre-commit hook hangs in PowerShell, per CLAUDE.md).

---

## File Structure

- **Modify** `src/lib/utils/modelFamily.ts` — add a `SPLIT_ONLY_FAMILIES` set and a `familyRequiresSeparateClip(family)` helper (canonical home for family classification).
- **Modify** `src/lib/stores/generation.svelte.ts` — add a guard in `toParams()` (~line 1486) that throws an actionable error when a non-split checkpoint belongs to a diffusion-only family.
- **Modify** `src/lib/components/generation/GenerateButton.svelte` — tighten the `skippedEmpty` warning condition (~line 268).
- **Modify** `src/lib/utils/promptSegmentDetail.ts` — relax + clamp the creativity/threshold bounds in `parseSegmentSpec` (~lines 64-66).

No new files, no new i18n keys (Bug A follows the existing hardcoded-`throw new Error(...)` pattern already used in `toParams()` at lines 1475-1485; Bug B removes a toast call; Bug C is internal parsing).

---

## Bug A — `clip input is invalid: None` on diffusion-only models

**Root cause:** `applyDefaultsIfNeeded` auto-picks `checkpoints[0]` blindly (`generation.svelte.ts:1819-1821`). Properly-installed split models live in ComfyUI's `diffusion_models/` folder, but if a UNET/diffusion-only file (e.g. an Anima or Flux UNET) is misplaced in `checkpoints/`, it is loaded via `CheckpointLoaderSimple` whose CLIP output is `None`, and every downstream conditioning node fails with `RuntimeError: clip input is invalid: None`. The store already resolves the file's `family` (via the `read_modelspec` IPC call wired through `ModelSelector`'s `$effect` → `loadModelSpec` → `applyModelMetadata`), so we can detect this case and raise an actionable error instead of crashing.

**Files:**
- Modify: `src/lib/utils/modelFamily.ts`
- Modify: `src/lib/stores/generation.svelte.ts:1472-1486` (`toParams`)

- [ ] **Step 1: Add the split-family classifier to `modelFamily.ts`**

Append to `src/lib/utils/modelFamily.ts` (after the `signalsIndicateVPred` function, end of file):

```ts
/**
 * Families that have no text encoder baked into a single checkpoint file — they
 * must be loaded as a separate diffusion model + text encoder (ComfyUI's
 * `diffusion_models/` + `text_encoders/`). If a file from one of these families
 * is loaded via `CheckpointLoaderSimple` (i.e. it was placed in `checkpoints/`),
 * ComfyUI returns a `None` CLIP and conditioning fails with
 * "clip input is invalid: None". Conservative list — only families that are
 * never distributed as a full single-file checkpoint with baked CLIP.
 */
export const SPLIT_ONLY_FAMILIES: ReadonlySet<ModelFamily> = new Set([
  "anima",
  "wan",
  "qwen",
  "flux",
  "flux1d",
  "flux1s",
  "flux1krea",
  "flux2d",
  "flux2klein9b",
  "flux2klein9bbase",
  "flux2klein4b",
  "flux2klein4bbase",
  "chroma",
]);

/** True when a family requires a separate text encoder (no baked CLIP). */
export function familyRequiresSeparateClip(family: ModelFamily | null | undefined): boolean {
  return !!family && SPLIT_ONLY_FAMILIES.has(family);
}
```

- [ ] **Step 2: Import the helper in the generation store**

In `src/lib/stores/generation.svelte.ts`, find the existing import from `../utils/modelFamily.js` (line 14) and add `familyRequiresSeparateClip` to the imported names. The existing line looks like:

```ts
} from "../utils/modelFamily.js";
```

Ensure `familyRequiresSeparateClip` is in the import list pulled from `"../utils/modelFamily.js"` (the import block that ends at line 14). For example, if the block imports `{ MODEL_FAMILIES, signalsIndicateVPred }`, change it to `{ MODEL_FAMILIES, signalsIndicateVPred, familyRequiresSeparateClip }`.

- [ ] **Step 3: Add the guard in `toParams()`**

In `src/lib/stores/generation.svelte.ts`, the existing split-model guard block ends at line 1486 with a closing `}`. Immediately after that closing brace (before the `const style = ...` line at 1488), insert:

```ts
    // Diffusion-only families (Flux, Anima, Wan, Qwen, Chroma, ...) carry no
    // text encoder in a single checkpoint. If one was placed in `checkpoints/`
    // and loaded via CheckpointLoaderSimple, ComfyUI returns a None CLIP and
    // fails with "clip input is invalid: None". Surface an actionable error.
    if (!this.useSplitModel && familyRequiresSeparateClip(this.modelFamily)) {
      throw new Error(
        `"${this.checkpoint}" is a ${this.modelFamily} diffusion model with no built-in text encoder. ` +
          `Move it to ComfyUI's diffusion_models/ folder and select it as a diffusion model, ` +
          `or choose a full checkpoint instead.`,
      );
    }
```

- [ ] **Step 4: Build gate**

Run: `npm run build`
Expected: build succeeds with no TypeScript errors. If it fails, the most likely cause is the import in Step 2 — confirm `familyRequiresSeparateClip` is exported from `modelFamily.ts` and listed in the store's import block.

- [ ] **Step 5: Manual reproduction check**

Reproduce the original crash and confirm the new behavior:
1. Place a UNET/diffusion-only `.safetensors` file (e.g. an Anima or Flux UNET) in ComfyUI's `checkpoints/` folder (not `diffusion_models/`).
2. Clear persisted settings so no checkpoint is selected, launch the app, let it connect to ComfyUI, and let the model selector resolve the auto-picked default's metadata.
3. Click Generate.
4. Expected: instead of the ComfyUI `clip input is invalid: None` crash, the inline error under the Generate button reads: `"<file>" is a <family> diffusion model with no built-in text encoder. Move it to ComfyUI's diffusion_models/ folder ...`
5. Sanity check no false positives: select a normal full checkpoint (SDXL/Illustrious/SD1.5) and confirm generation proceeds without the new error.

Note: the guard relies on `modelFamily` having been resolved by the model-selector effect. If a user clicks Generate in the brief window before metadata resolves, `modelFamily` is `"unknown"` and the guard does not fire (no false positive). This is acceptable for the common case; the guard's purpose is to make the steady-state failure actionable.

- [ ] **Step 6: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/utils/modelFamily.ts src/lib/stores/generation.svelte.ts
git -c core.hooksPath=/dev/null commit -m "fix: actionable error for diffusion-only models with no built-in CLIP"
```

---

## Bug B — spurious regional-prompt warning toast on every generation

**Root cause:** `regionalPrompts` is persisted to localStorage. Empty entries (boxes a user added then left blank, possibly long ago) survive across sessions. On every generation, `GenerateButton.svelte` computes `skippedEmpty = generation.regionalPrompts.length - configuredRegions` (line 267); when stale empty entries exist, `skippedEmpty > 0` is permanently true and fires a `"warning"` toast (lines 268-273) even when the user is not using regional prompting at all (`configuredRegions === 0`).

**Files:**
- Modify: `src/lib/components/generation/GenerateButton.svelte:268`

- [ ] **Step 1: Tighten the warning condition**

In `src/lib/components/generation/GenerateButton.svelte`, change line 268 from:

```ts
      if (skippedEmpty > 0) {
```

to:

```ts
      if (skippedEmpty > 0 && configuredRegions > 0) {
```

Rationale: the "N region(s) had no prompt text and were not sent" warning is only meaningful when the user is actively sending *some* regions. When `configuredRegions === 0` there is no active regional prompting, so stale empty entries should not warn.

- [ ] **Step 2: Build gate**

Run: `npm run build`
Expected: build succeeds.

- [ ] **Step 3: Manual reproduction check**

1. Open the regional prompting panel, add 1-2 region boxes, leave their prompt text empty, and close the panel (this persists empty entries).
2. With no valid regional prompts configured, click Generate.
3. Expected: no "region(s) had no prompt text" warning toast appears.
4. Regression check: add a region with non-empty text AND leave another empty, then Generate — the warning SHOULD still appear (because `configuredRegions > 0`).

- [ ] **Step 4: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/components/generation/GenerateButton.svelte
git -c core.hooksPath=/dev/null commit -m "fix: suppress regional empty-region warning when no regions are active"
```

---

## Bug C — SwarmUI segment tags rejected/left literal on import

**Root cause:** `metadataImport.ts` intentionally preserves `<segment:...>` tags on import. SwarmUI authors segment values at the bounds of its `0.0–1.0` range (e.g. `<segment:face,0.6,1.0>` or a `0` value). MooshieUI's `parseSegmentSpec` (`promptSegmentDetail.ts:64-65`) uses strict open bounds — `creativity > 0 && creativity <= 1` and `threshold > 0 && threshold < 1` — so boundary values (`0`, `1.0`) are rejected, the tag is left as literal text in the prompt (`promptSegmentDetail.ts:96-100`), and the segment never resolves. Relaxing to the closed `[0,1]` interval and clamping to a safe interior makes SwarmUI segments import as valid segments.

**Files:**
- Modify: `src/lib/utils/promptSegmentDetail.ts:64-66`

- [ ] **Step 1: Relax bounds and clamp**

In `src/lib/utils/promptSegmentDetail.ts`, replace lines 64-66:

```ts
  if (!(creativity > 0 && creativity <= 1)) return null;
  if (!(threshold > 0 && threshold < 1)) return null;
  return { target, creativity, threshold };
```

with:

```ts
  // Accept the full SwarmUI-compatible closed range [0, 1]; reject only
  // non-numeric / out-of-range values. Clamp to a safe interior so boundary
  // values (0, 1) imported from SwarmUI produce a usable mask instead of being
  // left as broken literal text in the prompt.
  if (!Number.isFinite(creativity) || creativity < 0 || creativity > 1) return null;
  if (!Number.isFinite(threshold) || threshold < 0 || threshold > 1) return null;
  const safeCreativity = Math.min(1, Math.max(0.05, creativity));
  const safeThreshold = Math.min(0.99, Math.max(0.01, threshold));
  return { target, creativity: safeCreativity, threshold: safeThreshold };
```

- [ ] **Step 2: Build gate**

Run: `npm run build`
Expected: build succeeds.

- [ ] **Step 3: Manual reproduction check**

1. Import (drag/drop or paste) a SwarmUI image whose prompt contains a boundary-value segment, e.g. `1girl, park <segment:face,0.6,1.0> freckles`. (If no such image is handy, paste the prompt text directly and open the Segment Refinement panel.)
2. Expected: the `<segment:face,0.6,1.0>` tag parses into a valid segment (visible in the Segment Refinement panel) rather than remaining as literal `<segment:...>` text in the prompt box.
3. Also verify `<segment:face,0.6,0>` (threshold 0) and `<segment:face>` (defaults) both parse without leaving literal text.

Note: this fix targets the most likely concrete defect (boundary-value rejection). The Segment Refinement panel appearing after import is expected/intended behavior, not part of this fix. If the user's actual reported pop-up turns out to be a different symptom, re-investigate with the specific SwarmUI image that triggered it before extending scope.

- [ ] **Step 4: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/utils/promptSegmentDetail.ts
git -c core.hooksPath=/dev/null commit -m "fix: accept SwarmUI boundary-value segment tags on import"
```

---

## Self-Review

**Spec coverage** (against M1 in the roadmap spec):
- "Missing CLIP on default model" → Bug A. ✓
- "Generation-start pop-up" → Bug B (the Rank-1 culprit: spurious regional warning). ✓ Lower-ranked culprits (stale LoRA path, style-transfer combos) only fire when those features are enabled, so they are not the "unexpected pop-up on a normal gen"; they are out of scope for this plan and tracked under M6 (LoRA) / future work if reported.
- "Swarm import / `<segment:>` pop-up" → Bug C. ✓

**Placeholder scan:** No TBD/TODO/"handle edge cases"; every code step shows the exact code. ✓

**Type consistency:** `familyRequiresSeparateClip` / `SPLIT_ONLY_FAMILIES` defined in `modelFamily.ts` (Step A1), imported (A2), and called (A3) with the same names. `this.modelFamily` is typed `ModelFamily` (`generation.svelte.ts:379`), matching the helper's parameter type. `configuredRegions` already exists in scope at `GenerateButton.svelte:255`. ✓

**Validation reality:** No test framework — each task ends with `npm run build` + manual repro + commit, matching the repo's pre-commit gate. All three changes are frontend-only, so no `cargo check` is required. ✓
