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
