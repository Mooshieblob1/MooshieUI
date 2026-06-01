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
