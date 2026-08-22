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
