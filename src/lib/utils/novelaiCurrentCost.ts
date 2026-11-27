/**
 * The Anlas estimate for the current generation settings.
 *
 * This is the number the Generate button's badge shows, lifted out of the
 * component so the Style Creator can price a round without duplicating the
 * Opus and V5 rules. `nSamples` defaults to the batch size, so a caller that
 * submits one image at a time passes 1.
 *
 * `forceTxt2ImgStrength` defaults to false so the Generate button's caller is
 * unaffected. Pass true for a caller (Style Creator) whose request forces
 * `mode: "txt2img"` regardless of the app's current mode, so the estimate
 * does not under-price the request while the app itself sits in img2img.
 *
 * Returns null outside NovelAI mode, where Anlas do not apply.
 */
import { generation } from "../stores/generation.svelte.js";
import { novelai } from "../stores/novelai.svelte.js";
import { savedCharacters } from "../stores/savedCharacters.svelte.js";
import { summarizeMultiCharPrPlan } from "./characterPreciseRefLogic.js";
import { OPUS_FREE_STEPS, estimateNovelAiCost } from "./novelaiCost.js";
import {
  NOVELAI_MEDIUM_EFFORT,
  naiV5Variant,
  novelAiActiveDirectorReferenceCount,
  novelAiEffortForcedByPreciseReference,
  novelAiUsesMediumEffort,
} from "./novelaiModels.js";

export function estimateCurrentNovelAiCost(
  nSamples: number = generation.batchSize,
  forceTxt2ImgStrength = false,
): number | null {
  if (!generation.isNovelAi) return null;
  const nai = generation.novelaiSettings;
  const model = generation.novelAiModel;
  const isInfill = !forceTxt2ImgStrength && generation.isNovelAiInfill;
  const opusExhausted = naiV5Variant(generation.checkpoint) !== null && novelai.opusAllowanceEmpty;
  const transparency = nai.transparent_background && generation.supportsNovelAiTransparency;

  const strategy = nai.multi_char_pr.strategy ?? "sheet";
  const usesPasses = strategy === "passes" || strategy === "hybrid";
  const usesSheet = strategy === "sheet" || strategy === "hybrid";
  const modeAllowed = usesPasses
    ? generation.mode === "txt2img" && nSamples === 1 && !transparency
    : (generation.mode === "txt2img" || generation.mode === "img2img") && !isInfill;

  if (
    novelai.experimentalMultiCharPr &&
    nai.multi_char_pr.enabled &&
    !forceTxt2ImgStrength &&
    modeAllowed
  ) {
    const plan = summarizeMultiCharPrPlan(
      model,
      nai.director_references,
      nai.characters,
      savedCharacters.characters,
      isInfill,
      true,
      strategy,
    );
    if (plan.active) {
      const baseRefsList = nai.director_references.filter((_, i) => plan.resolvedTargets[i] === null);
      const forcedHigh =
        usesSheet || novelAiEffortForcedByPreciseReference(model, baseRefsList, isInfill);
      const mediumEffort = novelAiUsesMediumEffort(model, nai.effort, forcedHigh);
      const baseSteps = mediumEffort ? NOVELAI_MEDIUM_EFFORT.steps : generation.steps;
      let total = estimateNovelAiCost({
        width: generation.width,
        height: generation.height,
        steps: baseSteps,
        mediumEffort,
        nSamples: usesPasses ? 1 : nSamples,
        strength: generation.mode === "txt2img" ? 1 : nai.strength,
        isOpus: novelai.isOpus,
        opusExhausted,
        vibeEncodes: nai.vibes.filter((v) => !v.encoding).length,
        preciseReferences: plan.baseRefCount + (usesSheet ? 1 : 0),
      });
      if (usesPasses) {
        const cropSteps =
          nai.multi_char_pr.anlas_policy === "allow_paid"
            ? Math.max(1, generation.steps)
            : Math.min(Math.max(1, generation.steps), OPUS_FREE_STEPS);
        const cropStrength = Math.min(1, Math.max(0.05, nai.multi_char_pr.strength));
        for (const group of plan.groups) {
          total += estimateNovelAiCost({
            width: 1024,
            height: 1024,
            steps: cropSteps,
            mediumEffort: false,
            nSamples: 1,
            strength: cropStrength,
            isOpus: novelai.isOpus,
            opusExhausted,
            vibeEncodes: 0,
            preciseReferences: group.refCount,
          });
        }
      }
      return total;
    }
  }

  const activeRefs = novelAiActiveDirectorReferenceCount(
    model,
    nai.director_references,
    isInfill,
  );
  const forcedHigh = novelAiEffortForcedByPreciseReference(
    model,
    nai.director_references,
    isInfill,
  );
  const mediumEffort = novelAiUsesMediumEffort(model, nai.effort, forcedHigh);
  const steps = mediumEffort ? NOVELAI_MEDIUM_EFFORT.steps : generation.steps;
  const isFocusedInpaint = generation.mode === "inpainting" && nai.focused_inpainting && !forceTxt2ImgStrength;
  const estWidth = isFocusedInpaint ? 1024 : generation.width;
  const estHeight = isFocusedInpaint ? 1024 : generation.height;
  const estStrength = forceTxt2ImgStrength || generation.mode === "txt2img"
    ? 1
    : isInfill
      ? nai.inpaint_strength
      : nai.strength;
  return estimateNovelAiCost({
    width: estWidth,
    height: estHeight,
    steps,
    mediumEffort,
    nSamples,
    strength: estStrength,
    isOpus: novelai.isOpus,
    // V5 has no Opus unlimited: with the allowance drained it bills in full.
    opusExhausted,
    vibeEncodes: nai.vibes.filter((v) => !v.encoding).length,
    preciseReferences: activeRefs,
  });
}
