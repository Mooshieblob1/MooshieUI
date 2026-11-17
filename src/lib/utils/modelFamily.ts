/**
 * Shared helpers for model-family detection and related metadata signals.
 */

import {
  findFamily,
  isRegisteredFamily,
  registeredFamilies,
  registeredFamilyLabel,
} from "./architectures.js";

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
  "flux3klein",
  "flux3kleinbase",
  "mossimage",
  "mossimagebase",
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
  "hunyuandit",
  "kolors",
  "mugen",
  "nanosaur",
  "unknown",
] as const;

/** Families with hand-written support. Mirrored by `LEGACY_FAMILIES` in commands/api.rs. */
export type LegacyModelFamily = typeof MODEL_FAMILIES[number];

/**
 * A hand-written family, or one declared in the backend's architecture
 * registry (`src-tauri/src/architectures/`, see `utils/architectures.ts`),
 * whose ids are only known at runtime.
 */
export type ModelFamily = LegacyModelFamily | (string & {});

/** True for a hand-written family or a registered one. */
export function isKnownModelFamily(value: unknown): value is ModelFamily {
  return (
    typeof value === "string" &&
    ((MODEL_FAMILIES as readonly string[]).includes(value) || isRegisteredFamily(value))
  );
}

/** Every family the app knows: hand-written ones, then registered ones. */
export function allModelFamilies(): ModelFamily[] {
  return [...MODEL_FAMILIES, ...registeredFamilies()];
}

/** Display name of a family (registered or hand-written), if it has one. */
export function modelFamilyLabel(family: ModelFamily | null | undefined): string | undefined {
  if (!family) return undefined;
  return registeredFamilyLabel(family) ?? MODEL_FAMILY_LABELS[family];
}

/** True when a family is prompted with Danbooru-style tags. */
export function familyUsesTagPrompts(family: ModelFamily | null | undefined): boolean {
  if (!family) return false;
  const registered = findFamily(family);
  if (registered) return registered.arch.prompt_style === "tags";
  return TAG_PROMPT_FAMILIES.has(family);
}

/**
 * Families whose prompts are written as Danbooru-style tags. Everything else
 * (Flux, Krea, Qwen, Z-Image, ...) is prompted in natural language, where tag
 * tooling such as the spellcheck underline only misfires.
 */
export const TAG_PROMPT_FAMILIES: ReadonlySet<ModelFamily> = new Set<ModelFamily>([
  "sdxl",
  "illustrious",
  "pony",
  "sd15",
  "anima",
]);

/** Display names per family. Families without an entry (`unknown`, the edit models) have none. */
export const MODEL_FAMILY_LABELS: Partial<Record<ModelFamily, string>> = {
  anima: "Anima",
  sdxl: "SDXL",
  illustrious: "Illustrious",
  pony: "Pony",
  sd15: "SD 1.5",
  sd3: "SD3",
  flux: "Flux",
  flux1d: "Flux.1 Dev",
  flux1s: "Flux.1 Schnell",
  flux1krea: "Flux.1 Krea",
  flux2d: "Flux.2 Dev",
  flux2klein9b: "Flux.2 Klein 9B",
  flux2klein9bbase: "Flux.2 Klein 9B Base",
  flux2klein4b: "Flux.2 Klein 4B",
  flux2klein4bbase: "Flux.2 Klein 4B Base",
  flux3klein: "FLUX 3 Klein",
  flux3kleinbase: "FLUX 3 Klein Base",
  mossimage: "MOSS-Image",
  mossimagebase: "MOSS-Image Base",
  chroma: "Chroma",
  zib: "Z-Image Base",
  zit: "Z-Image Turbo",
  wan: "Wan",
  qwen: "Qwen",
  ideogram4: "Ideogram 4.0",
  krea2: "Krea 2",
  auraflow: "AuraFlow",
  hunyuandit: "HunyuanDiT",
  kolors: "Kolors",
  mugen: "Mugen",
  nanosaur: "Nanosaur",
};

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
  "flux3klein",
  "flux3kleinbase",
  "mossimage",
  "mossimagebase",
  "chroma",
  "ideogram4",
  "krea2",
]);

/** True when a family requires a separate text encoder (no baked CLIP). */
export function familyRequiresSeparateClip(family: ModelFamily | null | undefined): boolean {
  const registered = findFamily(family);
  if (registered) return registered.arch.split_only;
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
