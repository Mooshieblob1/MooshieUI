/**
 * The NovelAI model table, mirroring `src-tauri/src/novelai/models.rs`.
 *
 * Duplicated rather than fetched because the UI has to know a model's
 * capabilities the moment it is selected, before any request is made. Keep the
 * two tables in step: the Rust side is authoritative for what the API accepts,
 * this side only decides what the UI offers.
 *
 * This is a leaf util. It must not import any store, or the generation store
 * that needs it would form a cycle.
 */

export interface NovelAiModelInfo {
  id: string;
  label: string;
  /** Checkpoint swapped in for `action: "infill"`. */
  inpaintingId: string;
  /** Uses the V4+ structured prompt block (characters, per-character UC). */
  v4Prompt: boolean;
  /** Precise Reference, also called character reference. */
  preciseReference: boolean;
  vibeTransfer: boolean;
  characterNegatives: boolean;
}

/**
 * Ordered newest-first, which is the order the model dropdown shows them in.
 *
 * V5's reference features are wired but off: NovelAI has not shipped them, and
 * flipping either boolean is all that is needed once it does.
 */
export const NOVELAI_MODELS: readonly NovelAiModelInfo[] = [
  {
    id: "nai-diffusion-5-full",
    label: "NovelAI V5 Full",
    inpaintingId: "nai-diffusion-5-full-inpainting",
    v4Prompt: true,
    preciseReference: false,
    vibeTransfer: false,
    characterNegatives: true,
  },
  {
    id: "nai-diffusion-5-curated",
    label: "NovelAI V5 Curated",
    inpaintingId: "nai-diffusion-5-curated-inpainting",
    v4Prompt: true,
    preciseReference: false,
    vibeTransfer: false,
    characterNegatives: true,
  },
  {
    id: "nai-diffusion-4-5-full",
    label: "NovelAI V4.5 Full",
    inpaintingId: "nai-diffusion-4-5-full-inpainting",
    v4Prompt: true,
    preciseReference: true,
    vibeTransfer: true,
    characterNegatives: true,
  },
  {
    id: "nai-diffusion-4-full",
    label: "NovelAI V4 Full",
    inpaintingId: "nai-diffusion-4-full-inpainting",
    v4Prompt: true,
    preciseReference: false,
    vibeTransfer: true,
    characterNegatives: true,
  },
] as const;

export function findNovelAiModel(id: string): NovelAiModelInfo | undefined {
  return NOVELAI_MODELS.find((m) => m.id === id);
}

/**
 * Is this checkpoint name a NovelAI model?
 *
 * This is the single switch that routes a generation to the NovelAI backend, so
 * it matches on the exact id rather than a prefix: a local checkpoint file that
 * happened to be named `nai-diffusion-something.safetensors` must still go to
 * ComfyUI.
 */
export function isNovelAiModel(id: string | null | undefined): boolean {
  return !!id && NOVELAI_MODELS.some((m) => m.id === id);
}

/**
 * Which V5 variant a checkpoint is, or null for anything else.
 *
 * The V5 prompt enhance is gated on this and nothing else. V4.5 and V4 take the
 * danbooru tag enhance they always took: their prompt format is a different
 * shape, and rewriting them to the V5 specification makes their output worse.
 * Inpainting checkpoints are matched too, because the prompt does not change
 * when the action does.
 */
export function naiV5Variant(id: string | null | undefined): "full" | "curated" | null {
  if (!id) return null;
  if (id.startsWith("nai-diffusion-5-full")) return "full";
  if (id.startsWith("nai-diffusion-5-curated")) return "curated";
  return null;
}

/**
 * NovelAI accepts only dimensions that are a multiple of 64, and its own UI
 * steps the side length on that grid (1024 -> 1088 -> 1152). Local backends are
 * happy on an 8px grid, so this applies to NovelAI mode only.
 */
export const NOVELAI_DIMENSION_STEP = 64;

/** Round a pixel dimension onto NovelAI's grid, never below one full step. */
export function snapNovelAiDimension(px: number): number {
  return Math.max(
    NOVELAI_DIMENSION_STEP,
    Math.round(px / NOVELAI_DIMENSION_STEP) * NOVELAI_DIMENSION_STEP,
  );
}

/** NovelAI's recommended sampling settings, applied when a NAI model is picked. */
export const NOVELAI_DEFAULTS = {
  steps: 23,
  cfg: 7.0,
  sampler: "k_euler_ancestral",
  noiseSchedule: "karras",
  cfgRescale: 0,
} as const;

/** Samplers NovelAI accepts, in the order its own UI lists them. */
export const NOVELAI_SAMPLERS = [
  { value: "k_euler_ancestral", label: "Euler Ancestral" },
  { value: "k_euler", label: "Euler" },
  { value: "k_dpmpp_2s_ancestral", label: "DPM++ 2S Ancestral" },
  { value: "k_dpmpp_2m_sde", label: "DPM++ 2M SDE" },
  { value: "k_dpmpp_2m", label: "DPM++ 2M" },
  { value: "k_dpmpp_sde", label: "DPM++ SDE" },
  { value: "ddim_v3", label: "DDIM" },
] as const;

export const NOVELAI_NOISE_SCHEDULES = [
  { value: "karras", label: "Karras" },
  { value: "exponential", label: "Exponential" },
  { value: "polyexponential", label: "Polyexponential" },
  { value: "native", label: "Native" },
] as const;

/** ComfyUI sampler name -> the nearest NovelAI sampler. */
const SAMPLER_ALIASES: Record<string, string> = {
  euler: "k_euler",
  euler_ancestral: "k_euler_ancestral",
  euler_ancestral_cfg_pp: "k_euler_ancestral",
  dpmpp_2m: "k_dpmpp_2m",
  dpmpp_2m_sde: "k_dpmpp_2m_sde",
  dpmpp_sde: "k_dpmpp_sde",
  dpmpp_2s_ancestral: "k_dpmpp_2s_ancestral",
  ddim: "ddim_v3",
};

/**
 * Coerce a sampler name into one NovelAI accepts.
 *
 * NovelAI keeps its own sampler field, but the value carried over from the
 * ComfyUI dropdown is the natural starting point when a NovelAI model is picked.
 * An unrecognised name would be rejected by the API, so anything without a
 * mapping falls back to NovelAI's own recommended sampler.
 */
export function toNovelAiSampler(name: string): string {
  if (NOVELAI_SAMPLERS.some((s) => s.value === name)) return name;
  return SAMPLER_ALIASES[name] ?? NOVELAI_DEFAULTS.sampler;
}

/**
 * How many of each reference input the UI offers.
 *
 * These mirror NovelAI's own client rather than a hard API limit: the API
 * accepts more, but going past what NovelAI itself allows is untested and
 * costs Anlas to discover.
 */
export const NOVELAI_MAX_CHARACTERS = 6;
export const NOVELAI_MAX_VIBES = 4;
export const NOVELAI_MAX_DIRECTOR_REFERENCES = 4;

/** What a Precise Reference is allowed to take from its image. */
export const NOVELAI_REFERENCE_DESCRIPTIONS = [
  { value: "character", labelKey: "generation.novelai.reference.desc_character" },
  { value: "style", labelKey: "generation.novelai.reference.desc_style" },
  { value: "character&style", labelKey: "generation.novelai.reference.desc_character_style" },
] as const;
