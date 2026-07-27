/**
 * Shared helpers for model-family detection and related metadata signals.
 */

export const MODEL_FAMILIES = [
  "anima",
  "sdxl",
  "illustrious",
  "pony",
  "sd15",
  "sd3",
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
  "zib",
  "zit",
  "wan",
  "qwen",
  "qwen_edit",
  "qwen_edit_plus",
  "flux1kontext",
  "ideogram4",
  "krea2",
  "auraflow",
  "pixart",
  "hunyuandit",
  "cascade",
  "kolors",
  "mugen",
  "nanosaur",
  "unknown",
] as const;

export type ModelFamily = typeof MODEL_FAMILIES[number];

export const TURBO_MODEL_VARIANTS = [
  "none",
  "turbo",
  "lightning",
  "lcm",
  "hyper",
  "dmd",
  "dmd2",
] as const;

export type TurboModelVariant = typeof TURBO_MODEL_VARIANTS[number];

export interface ModelFamilySignals {
  /** Checkpoint filename or diffusion UNET filename */
  filename?: string | null;
  modelspecPredictionType?: string | null;
  modelspecPredictKey?: string | null;
  headerVPred?: boolean | null;
  /** Backend-resolved model family bucket. */
  modelFamily?: ModelFamily | null;
}

/** Filename heuristic for v-pred SDXL variants. */
export function filenameIndicatesVPred(name: string | null | undefined): boolean {
  if (!name) return false;
  const n = name.toLowerCase();
  if (n.includes("vpred") || n.includes("v-pred") || n.includes("v_pred")) return true;
  if (n.includes("juice")) return true;
  if (n.includes("2048") && (n.includes("noob") || n.includes("seele"))) return true;
  if (n.includes("seele") && n.includes("pop")) return true;
  return false;
}

/** True when metadata explicitly marks the model as v-pred. */
export function metadataIndicatesVPred(
  predictionType: string | null | undefined,
  predictKey: string | null | undefined,
  headerVPred?: boolean | null,
): boolean {
  if ((predictionType ?? "").trim().toLowerCase() === "v") return true;
  if ((predictKey ?? "").trim().toLowerCase() === "v") return true;
  return headerVPred === true;
}

/** Combined signal — metadata first, then filename fallback. */
export function signalsIndicateVPred(signals: ModelFamilySignals): boolean {
  if (
    metadataIndicatesVPred(
      signals.modelspecPredictionType,
      signals.modelspecPredictKey,
      signals.headerVPred,
    )
  ) return true;
  return filenameIndicatesVPred(signals.filename);
}

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
  "qwen_edit",
  "qwen_edit_plus",
  "flux",
  "flux1d",
  "flux1s",
  "flux1krea",
  "flux1kontext",
  "flux2d",
  "flux2klein9b",
  "flux2klein9bbase",
  "flux2klein4b",
  "flux2klein4bbase",
  "chroma",
  "ideogram4",
  "krea2",
]);

/** True when a family requires a separate text encoder (no baked CLIP). */
export function familyRequiresSeparateClip(family: ModelFamily | null | undefined): boolean {
  return !!family && SPLIT_ONLY_FAMILIES.has(family);
}

/** SDXL-architecture families, used when a manual override skips backend detection. */
export function familyIsSdxlLike(family: ModelFamily): boolean {
  return (["sdxl", "illustrious", "pony", "mugen"] as ReadonlyArray<ModelFamily>).includes(family);
}

/** Coerce a raw ModelSpec string into a known turbo variant. */
export function toTurboModelVariant(value: string | undefined): TurboModelVariant {
  return TURBO_MODEL_VARIANTS.includes(value as TurboModelVariant)
    ? (value as TurboModelVariant)
    : "none";
}
