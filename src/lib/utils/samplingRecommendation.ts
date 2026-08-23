/**
 * Per-model sampling recommendations, in one place.
 *
 * These are the numbers the "Apply" button in the sampler panel writes. The
 * NovelAI local post-process borrows the sampler, schedule and guidance from
 * the same table: it cannot inherit NovelAI's, because those describe
 * NovelAI's own model and "k_euler_ancestral" / "karras" are not even names
 * ComfyUI's KSampler knows, and the sampler panel is hidden in NovelAI mode so
 * nothing else fills them. Its step count and denoise come from the upscale
 * and face-fix panels instead, which the user can see. The table lives here
 * rather than inside a component because both callers need it.
 */

export interface SamplingRecommendation {
  steps: number;
  cfg: number;
  samplerName: string;
  scheduler: string;
  /** Refine steps for the upscale pass. Left out where the model card has no opinion. */
  upscaleSteps?: number;
  /** Refine steps for FaceDetailer. Left out where the model card has no opinion. */
  facefixSteps?: number;
  upscaleDenoise?: number;
}

/** Anima. Face fix and upscale steps are a third of main steps. */
export const ANIMA_SAMPLING: SamplingRecommendation = {
  steps: 30,
  cfg: 4.0,
  samplerName: "er_sde",
  scheduler: "sgm_uniform",
  upscaleSteps: 10,
  facefixSteps: 10,
};

/** Juice / Seele / SIH. */
export const JUICE_SAMPLING: SamplingRecommendation = {
  steps: 20,
  cfg: 1.4,
  samplerName: "euler_cfg_pp",
  scheduler: "sgm_uniform",
  upscaleSteps: 7,
  facefixSteps: 7,
};

/** Nanosaur, which asks for a much heavier refine pass than the others. */
export const NANOSAUR_SAMPLING: SamplingRecommendation = {
  steps: 40,
  cfg: 7,
  samplerName: "euler",
  scheduler: "simple",
  upscaleSteps: 20,
  upscaleDenoise: 0.5,
};

/**
 * A conservative middle for a model nothing is known about.
 *
 * Only the NovelAI local pass uses this: it has to put some sampler in the
 * graph no matter what was picked, whereas the sampler panel simply shows no
 * recommendation and leaves an unrecognised model on the user's own settings.
 */
export const GENERIC_SAMPLING: SamplingRecommendation = {
  steps: 25,
  cfg: 6.0,
  samplerName: "euler",
  scheduler: "normal",
  upscaleSteps: 9,
  facefixSteps: 9,
};

/**
 * The recommendation for a model, or `null` when none is known.
 *
 * The filename is matched as well as the detected `family` because a finetune
 * keeps its base family but is named after the model whose settings it wants.
 */
export function recommendedSamplingFor(
  filename: string | null | undefined,
  family?: string | null,
): SamplingRecommendation | null {
  const name = (filename ?? "").toLowerCase();
  const fam = (family ?? "").toLowerCase();
  if (fam === "anima" || name.includes("anima")) return ANIMA_SAMPLING;
  if (name.includes("nanosaur")) return NANOSAUR_SAMPLING;
  if (
    name.includes("juice") ||
    name.includes("seele") ||
    name.includes("sih") ||
    name.includes("σih")
  ) {
    return JUICE_SAMPLING;
  }
  return null;
}
