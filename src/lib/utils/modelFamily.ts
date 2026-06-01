/**
 * Shared heuristics for detecting model families (especially Anima / Wan2.1 fine-tunes
 * like animayume that may not include "anima" in the filename).
 */

export type ModelFamily =
  | "sdxl"
  | "illustrious"
  | "sd15"
  | "sd3"
  | "flux"
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
  /** Checkpoint filename, diffusion UNET filename, or curated display label */
  filename?: string | null;
  modelspecArchitecture?: string | null;
  /** CivitAI `baseModel` from hash lookup (e.g. "Wan Video 2.1") */
  civitaiBaseModel?: string | null;
  /** modelspec.tags comma-separated string */
  modelspecTags?: string | null;
}

/** True when CivitAI lists the version under an Anima / Wan video base. */
export function civitaiBaseModelIndicatesAnima(baseModel: string | null | undefined): boolean {
  if (!baseModel) return false;
  const bm = baseModel.toLowerCase();
  return (
    bm.includes("anima") ||
    bm.includes("wan video") ||
    bm.includes("wan 2") ||
    bm.includes("wan2") ||
    bm.includes("wan 2.1") ||
    bm === "wan"
  );
}

/** True when modelspec architecture / tags describe Anima or Wan2.1. */
export function modelspecIndicatesAnima(
  architecture: string | null | undefined,
  tags?: string | null,
): boolean {
  const arch = (architecture ?? "").toLowerCase();
  if (arch.includes("anima") || arch.includes("wan")) return true;
  const tagStr = (tags ?? "").toLowerCase();
  if (tagStr.includes("anima")) return true;
  return false;
}

/** Filename / label heuristics for local Anima-family checkpoints and UNET files. */
export function filenameIndicatesAnima(name: string | null | undefined): boolean {
  if (!name) return false;
  const n = name.toLowerCase();
  if (n.includes("nanosaur") || n.includes("mugen")) return false;
  if (n.includes("anima")) return true;
  // Fine-tunes such as animayume_v05.safetensors
  if (n.includes("yume")) return true;
  return false;
}

/** Combined signal — modelspec, CivitAI hash metadata, or filename. */
export function signalsIndicateAnima(signals: ModelFamilySignals): boolean {
  if (filenameIndicatesAnima(signals.filename)) return true;
  if (modelspecIndicatesAnima(signals.modelspecArchitecture, signals.modelspecTags)) return true;
  if (civitaiBaseModelIndicatesAnima(signals.civitaiBaseModel)) return true;
  return false;
}

/** Illustrious / NoobAI / SIH family — must win over generic Anima/Wan CivitAI metadata heuristics. */
export function filenameIndicatesIllustrious(name: string | null | undefined): boolean {
  if (!name) return false;
  const n = name.toLowerCase();
  return (
    n.includes("illustrious") ||
    n.includes("noobai") ||
    n.includes("noob") ||
    n.includes("sih") ||
    n.includes("juice") ||
    n.includes("seele")
  );
}

export function modelNamesIndicateIllustrious(
  checkpoint?: string | null,
  diffusionModel?: string | null,
): boolean {
  return filenameIndicatesIllustrious(checkpoint) || filenameIndicatesIllustrious(diffusionModel);
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
