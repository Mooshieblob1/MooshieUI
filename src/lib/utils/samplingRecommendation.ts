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

import { registeredSampling } from "./architectures.js";

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

/**
 * RES4LYF's beta57 scheduler. The Anima model card suggests it for a more
 * painterly or realistic look, since it spends more of the schedule on
 * low-noise steps. It only exists when the RES4LYF node pack is installed, so
 * it is offered only while ComfyUI lists it.
 */
export const BETA57_SCHEDULER = "beta57";

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
 * Krea 2 Turbo (and Turbo merges such as Wulver). Distilled to run without
 * guidance, so CFG stays at 1: anything higher burns the image and doubles
 * the work. Euler/simple is Krea's own ComfyUI setup; see `kreaFewStep.ts`.
 */
export const KREA2_TURBO_SAMPLING: SamplingRecommendation = {
  steps: 8,
  cfg: 1.0,
  samplerName: "euler",
  scheduler: "simple",
  upscaleSteps: 3,
  facefixSteps: 3,
};

/** Krea 2 Raw, the undistilled weights. Matches the store's Krea 2 preset. */
export const KREA2_SAMPLING: SamplingRecommendation = {
  steps: 30,
  cfg: 4.0,
  samplerName: "er_sde",
  scheduler: "sgm_uniform",
  upscaleSteps: 10,
  facefixSteps: 10,
};

/**
 * FLUX 3 Image Klein, distilled flow-map checkpoint: 16 steps without guidance
 * (Black Forest Labs' model card). Matches the store's flux3klein preset.
 */
export const FLUX3_KLEIN_SAMPLING: SamplingRecommendation = {
  steps: 16,
  cfg: 1.0,
  samplerName: "euler",
  scheduler: "simple",
};

/** FLUX 3 Image Klein Base, the undistilled weights: 50 steps at CFG 3.5. */
export const FLUX3_KLEIN_BASE_SAMPLING: SamplingRecommendation = {
  steps: 50,
  cfg: 3.5,
  samplerName: "euler",
  scheduler: "simple",
};

/**
 * Moody Moss Mix (catlover1937), a MOSS-Image fine-tune. Its "25STEPS CFG3"
 * versions and the author's Moody Moss 4K HD ComfyUI workflow sample with
 * Euler on the simple schedule, 25 steps at CFG 3.
 */
export const MOODY_MOSS_SAMPLING: SamplingRecommendation = {
  steps: 25,
  cfg: 3.0,
  samplerName: "euler",
  scheduler: "simple",
};

/** Base canvas of the 4K HD workflow (2:3, about 1 MP); the second pass is 1.5x. */
export const MOODY_MOSS_BASE_SIZE = { width: 832, height: 1248 } as const;

/** Latent upscale between the two 4K HD passes. Mirrors MOODY_MOSS_LATENT_SCALE in txt2img.rs. */
export const MOODY_MOSS_HD_SCALE = 1.5;

/** The negative prompt the 4K HD workflow ships with. */
export const MOODY_MOSS_NEGATIVE =
  "worst quality, low quality, bad anatomy, deformed hands, extra fingers, malformed limbs, distorted face, plastic skin, overly smooth skin, waxy skin, oversharpening, excessive skin retouching, blurry, jpeg artifacts, watermark";

/** CivitAI model IDs, for the credit links (both pages are rated mature). */
export const MOODY_MOSS_MODEL_ID = 3007871;
export const MOODY_MOSS_WORKFLOW_ID = 3012555;

/**
 * True for a Moody Moss Mix file (`Moody-Moss-Mix-V4.1_00001__clean-fp8.safetensors`
 * and the like). The V1 Turbo edition is left out: it is a few-step model and
 * the 4K HD workflow is written for the 25-step ones. Its files are the only
 * ones named `V1_` with no step count.
 */
export function isMoodyMossFilename(filename: string | null | undefined): boolean {
  const name = (filename ?? "").toLowerCase();
  if (!/moody[\s_-]*moss/.test(name)) return false;
  return !name.includes("turbo") && !/moody[\s_-]*moss[\s_-]*mix[\s_-]*v1_/.test(name);
}

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
 * `turbo` is the detected turbo variant, which picks between a family's
 * distilled and undistilled settings where they differ.
 */
export function recommendedSamplingFor(
  filename: string | null | undefined,
  family?: string | null,
  turbo = false,
): SamplingRecommendation | null {
  const name = (filename ?? "").toLowerCase();
  const fam = (family ?? "").toLowerCase();
  // Moody Moss Mix is a MOSS-Image fine-tune with its author's own settings;
  // the registered MOSS-Image defaults would otherwise win.
  if ((fam === "mossimage" || fam === "mossimagebase") && isMoodyMossFilename(name)) {
    return MOODY_MOSS_SAMPLING;
  }
  // A registered architecture (src-tauri/src/architectures) declares its own.
  const registered = registeredSampling(family);
  if (registered) {
    return {
      steps: registered.steps,
      cfg: registered.cfg,
      samplerName: registered.sampler,
      scheduler: registered.scheduler,
    };
  }
  if (fam === "krea2") return turbo ? KREA2_TURBO_SAMPLING : KREA2_SAMPLING;
  if (fam === "flux3klein") return FLUX3_KLEIN_SAMPLING;
  if (fam === "flux3kleinbase") return FLUX3_KLEIN_BASE_SAMPLING;
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
