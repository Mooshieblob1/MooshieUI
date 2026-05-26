import { generation } from "../stores/generation.svelte.js";
import { progress } from "../stores/progress.svelte.js";
import type { GenerationParams, RegionalPromptSelection } from "../types/index.js";
import { buildRegionalContextPrompt, mergeRegionalPromptText } from "./promptSchedule.js";
import { uploadImageBytes } from "./api.js";
import { renderRegionMaskPngBytes, regionStrengthToDenoise } from "./regionalMask.js";
import { regionalChainStepSeed } from "./regionalChainSeed.js";
import {
  tempOutputToUploadBytes,
  waitForPromptCompletion,
  waitForPromptOutput,
} from "./waitForPrompt.js";

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export interface RegionalInpaintChainStepContext {
  phase: "base" | "region";
  /** Step index in the chain (0 = base, 1..N = regions). */
  index: number;
  /** Total ComfyUI jobs (1 base + N regions). */
  total: number;
  /** True only for the last region inpaint — gallery save allowed. */
  isFinalOutput: boolean;
}

export interface RegionalInpaintChainCallbacks {
  submit: (
    params: GenerationParams,
    ctx: RegionalInpaintChainStepContext,
  ) => Promise<{ promptId: string; seed: number }>;
  onStep?: (info: { phase: "base" | "region"; index: number; total: number }) => void;
  onWaitingForOutput?: () => void;
  shouldCancel?: () => boolean;
}

export interface RegionalInpaintChainResult {
  lastPromptId: string;
  regionCount: number;
}

async function waitForChainStepOutput(
  promptId: string,
  isFinalOutput: boolean,
): Promise<string> {
  const temp = await waitForPromptOutput(promptId);
  if (isFinalOutput) {
    await waitForPromptCompletion(promptId);
    await sleep(600);
  }
  return temp;
}

/**
 * txt2img base pass, then sequential inpainting per region on the accumulated image.
 * Each region step uses the previous step's output as input (one combined result).
 */
export async function runRegionalInpaintChain(
  regions: RegionalPromptSelection[],
  callbacks: RegionalInpaintChainCallbacks,
): Promise<RegionalInpaintChainResult> {
  const validRegions = regions.filter(
    (r) => r.text.trim() && r.width > 0 && r.height > 0,
  );
  if (validRegions.length === 0) {
    throw new Error("No valid regional prompts for inpaint chain");
  }

  const totalSteps = 1 + validRegions.length;

  const baseParams = generation.toParams({ includeConditioningRegions: false });
  const facefixOnFinal = baseParams.facefix_enabled;
  // Face fix once on the final combined image; skip on base + intermediate inpaints.
  baseParams.facefix_enabled = false;

  const regionalContext = buildRegionalContextPrompt(
    baseParams.positive_prompt,
    baseParams.positive_segments,
    generation.loras.filter((l) => l.enabled && l.name),
  );

  callbacks.onStep?.({ phase: "base", index: 0, total: totalSteps });
  if (callbacks.shouldCancel?.()) throw new Error("Regional inpaint chain cancelled");

  const baseResult = await callbacks.submit(baseParams, {
    phase: "base",
    index: 0,
    total: totalSteps,
    isFinalOutput: false,
  });
  callbacks.onWaitingForOutput?.();
  let inputTemp = await waitForChainStepOutput(baseResult.promptId, false);
  progress.clearPromptOutput(baseResult.promptId);
  if (callbacks.shouldCancel?.()) throw new Error("Regional inpaint chain cancelled");

  let lastPromptId = baseResult.promptId;
  const resolvedBaseSeed = baseResult.seed;

  for (let i = 0; i < validRegions.length; i++) {
    const region = validRegions[i];
    const isFinalOutput = i === validRegions.length - 1;
    callbacks.onStep?.({ phase: "region", index: i + 1, total: totalSteps });
    if (callbacks.shouldCancel?.()) throw new Error("Regional inpaint chain cancelled");

    const inputBytes = await tempOutputToUploadBytes(inputTemp);
    const inputUpload = await uploadImageBytes(
      inputBytes,
      `regional_chain_input_${i}_${Date.now()}.png`,
    );

    const maskBytes = await renderRegionMaskPngBytes(
      region,
      generation.width,
      generation.height,
    );
    const maskUpload = await uploadImageBytes(
      maskBytes,
      `regional_chain_mask_${i}_${Date.now()}.png`,
    );

    const mergedPrompt = mergeRegionalPromptText(regionalContext, region.text);
    const regionParams: GenerationParams = {
      ...baseParams,
      mode: "inpainting",
      input_image: inputUpload.name,
      mask_image: maskUpload.name,
      positive_prompt: mergedPrompt,
      positive_regions: [],
      seed: regionalChainStepSeed(resolvedBaseSeed, i),
      denoise: regionStrengthToDenoise(region.strength),
      differential_diffusion: generation.isAnima || generation.differentialDiffusion,
      facefix_enabled: isFinalOutput && facefixOnFinal,
    };

    const regionResult = await callbacks.submit(regionParams, {
      phase: "region",
      index: i + 1,
      total: totalSteps,
      isFinalOutput,
    });
    callbacks.onWaitingForOutput?.();
    inputTemp = await waitForChainStepOutput(regionResult.promptId, isFinalOutput);
    progress.clearPromptOutput(regionResult.promptId);
    if (callbacks.shouldCancel?.()) throw new Error("Regional inpaint chain cancelled");

    lastPromptId = regionResult.promptId;
  }

  return { lastPromptId, regionCount: validRegions.length };
}
