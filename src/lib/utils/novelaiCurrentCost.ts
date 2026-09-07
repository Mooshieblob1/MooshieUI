/**
 * The Anlas estimate for the current generation settings.
 *
 * This is the number the Generate button's badge shows, lifted out of the
 * component so the Style Creator can price a round without duplicating the
 * Opus and V5 rules. `nSamples` defaults to the batch size, so a caller that
 * submits one image at a time passes 1.
 *
 * Returns null outside NovelAI mode, where Anlas do not apply.
 */
import { generation } from "../stores/generation.svelte.js";
import { novelai } from "../stores/novelai.svelte.js";
import { estimateNovelAiCost } from "./novelaiCost.js";
import { naiV5Variant } from "./novelaiModels.js";

export function estimateCurrentNovelAiCost(nSamples: number = generation.batchSize): number | null {
  if (!generation.isNovelAi) return null;
  const nai = generation.novelaiSettings;
  return estimateNovelAiCost({
    width: generation.width,
    height: generation.height,
    steps: generation.steps,
    nSamples,
    strength: generation.mode === "txt2img" ? 1 : nai.strength,
    isOpus: novelai.isOpus,
    // V5 has no Opus unlimited: with the allowance drained it bills in full.
    opusExhausted: naiV5Variant(generation.checkpoint) !== null && novelai.opusAllowanceEmpty,
    vibeEncodes: nai.vibes.filter((v) => !v.encoding).length,
  });
}
