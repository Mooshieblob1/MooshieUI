/**
 * Shared helpers for model-family detection and related metadata signals.
 */

export type ModelFamily =
  | "sdxl"
  | "illustrious"
  | "sd15"
  | "sd3"
  | "flux"
  | "wan"
  | "qwen"
  | "pony"
  | "auraflow"
  | "pixart"
  | "hunyuandit"
  | "cascade"
  | "kolors"
  | "mugen"
  | "nanosaur"
  | "anima"
  | "unknown";

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
  return n.includes("vpred") || n.includes("v-pred");
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

/** Combined signal — backend family/modelspec tags, then filename fallback. */
export function signalsIndicateAnima(signals: ModelFamilySignals): boolean {
  return signals.modelFamily === "anima";
}

/** True when ModelSpec or filename indicates v-prediction with zero-terminal SNR scheduling. */
export function predictionTypeIndicatesVpred(predictionType: string | null | undefined): boolean {
  if (!predictionType) return false;
  const p = predictionType.toLowerCase().replace(/[\s_-]+/g, "");
  return p.includes("vpred") || p === "vprediction";
}

/** Filename heuristics for NoobAI v-pred / 2048-family checkpoints (e.g. seele_pop3). */
export function filenameIndicatesVpredZsnr(name: string | null | undefined): boolean {
  if (!name) return false;
  const n = name.toLowerCase();
  if (n.includes("vpred") || n.includes("v-pred") || n.includes("v_pred")) return true;
  if (n.includes("juice")) return true;
  if (n.includes("2048") && (n.includes("noob") || n.includes("seele"))) return true;
  if (n.includes("seele") && n.includes("pop")) return true;
  return false;
}

export function modelNamesIndicateVpredZsnr(
  checkpoint?: string | null,
  diffusionModel?: string | null,
  predictionType?: string | null,
): boolean {
  if (predictionTypeIndicatesVpred(predictionType)) return true;
  return filenameIndicatesVpredZsnr(checkpoint) || filenameIndicatesVpredZsnr(diffusionModel);
}
