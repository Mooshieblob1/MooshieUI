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
  /**
   * The VAE emits a real alpha channel, so a transparent background can be
   * asked for. V5's custom VAE is the first to carry one.
   */
  alpha: boolean;
  /**
   * Quoted prompt text is auto-formatted into a trailing `Text:` block, the
   * way NovelAI's own V5 frontend does it. The transform itself runs in the
   * Rust payload builder; this flag only mirrors it for the UI.
   */
  autoText: boolean;
  /**
   * Character slots the UI offers, mirroring NovelAI's own client. V5's free
   * positioning was demonstrated with up to 22 characters; V4/V4.5 stop at 6.
   */
  maxCharacters: number;
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
    alpha: true,
    autoText: true,
    maxCharacters: 22,
  },
  {
    id: "nai-diffusion-5-curated",
    label: "NovelAI V5 Curated",
    // V5 Curated's own inpainting model is still training upstream, and
    // NovelAI's client substitutes V4.5 Curated's in the meantime. Point back
    // at `nai-diffusion-5-curated-inpainting` once it ships.
    inpaintingId: "nai-diffusion-4-5-curated-inpainting",
    v4Prompt: true,
    preciseReference: false,
    vibeTransfer: false,
    characterNegatives: true,
    alpha: true,
    autoText: true,
    maxCharacters: 22,
  },
  {
    id: "nai-diffusion-4-5-full",
    label: "NovelAI V4.5 Full",
    inpaintingId: "nai-diffusion-4-5-full-inpainting",
    v4Prompt: true,
    preciseReference: true,
    vibeTransfer: true,
    characterNegatives: true,
    alpha: false,
    autoText: false,
    maxCharacters: 6,
  },
  {
    id: "nai-diffusion-4-full",
    label: "NovelAI V4 Full",
    inpaintingId: "nai-diffusion-4-full-inpainting",
    v4Prompt: true,
    preciseReference: false,
    vibeTransfer: true,
    characterNegatives: true,
    alpha: false,
    autoText: false,
    maxCharacters: 6,
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

/**
 * The aspect ratios NovelAI's own UI offers.
 *
 * Every size preset on novelai.net (Small, Normal, Large, Wallpaper) is one of
 * these five shapes: square, 2:3 portrait, 3:2 landscape, and the 9:16 / 16:9
 * wallpapers. The API accepts any 64px-grid size, but the models are trained
 * and tuned on these, so in NovelAI mode the aspect ratio controls stay on
 * them: presets, inferred ratios and typed ratios all resolve to this list.
 */
export const NOVELAI_ASPECT_RATIOS: ReadonlyArray<{ label: string; w: number; h: number }> = [
  { label: "1:1", w: 1, h: 1 },
  { label: "3:2", w: 3, h: 2 },
  { label: "16:9", w: 16, h: 9 },
  { label: "2:3", w: 2, h: 3 },
  { label: "9:16", w: 9, h: 16 },
];

/**
 * The NovelAI aspect ratio closest to `w:h`.
 *
 * Compared in log space so being too wide and too tall by the same factor
 * count as equally far off. Any non-positive input falls back to square.
 */
export function nearestNovelAiAspect(w: number, h: number): { w: number; h: number } {
  if (!(w > 0) || !(h > 0)) return { w: 1, h: 1 };
  const target = Math.log(w / h);
  let best = NOVELAI_ASPECT_RATIOS[0];
  let bestErr = Number.POSITIVE_INFINITY;
  for (const r of NOVELAI_ASPECT_RATIOS) {
    const err = Math.abs(target - Math.log(r.w / r.h));
    if (err < bestErr) {
      best = r;
      bestErr = err;
    }
  }
  return { w: best.w, h: best.h };
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
 * Character slots the UI offers for a given checkpoint.
 *
 * Per-model because V5 raised the ceiling to 22 while V4/V4.5 keep NovelAI's
 * old limit of 6. Inpainting checkpoints and unknown ids fall back to the
 * conservative 6 rather than erroring: the cap gates a button, not a request.
 */
export function novelAiMaxCharacters(id: string | null | undefined): number {
  if (naiV5Variant(id) !== null) return 22;
  return findNovelAiModel(id ?? "")?.maxCharacters ?? 6;
}

/**
 * How many of each reference input the UI offers.
 *
 * These mirror NovelAI's own client rather than a hard API limit: the API
 * accepts more, but going past what NovelAI itself allows is untested and
 * costs Anlas to discover.
 */
export const NOVELAI_MAX_VIBES = 4;
export const NOVELAI_MAX_DIRECTOR_REFERENCES = 4;

/** What a Precise Reference is allowed to take from its image. */
export const NOVELAI_REFERENCE_DESCRIPTIONS = [
  { value: "character", labelKey: "generation.novelai.reference.desc_character" },
  { value: "style", labelKey: "generation.novelai.reference.desc_style" },
  { value: "character&style", labelKey: "generation.novelai.reference.desc_character_style" },
] as const;

/**
 * How close two character centres may sit before NovelAI calls them stacked.
 *
 * Taken from NovelAI's own client, which flags any pair less than 0.1 apart
 * (Euclidean, in the same normalised 0..1 canvas space we store) and warns
 * that overlapping characters degrade the result. Mirrored rather than
 * guessed so our canvas agrees with theirs instead of inventing a stricter
 * or looser rule.
 */
export const NOVELAI_OVERLAP_DISTANCE = 0.1;

/**
 * Indices of the characters sitting on top of another one.
 *
 * Both members of every too-close pair land in the set, so the UI can mark
 * each offending circle rather than guessing which one should move. Callers
 * pass only the placements that are actually on the canvas: a disabled
 * character has no circle to turn red.
 */
export function novelAiOverlappingCharacters(
  centers: readonly { x: number; y: number }[],
  threshold: number = NOVELAI_OVERLAP_DISTANCE,
): Set<number> {
  const overlapping = new Set<number>();
  for (let i = 0; i < centers.length; i++) {
    for (let j = i + 1; j < centers.length; j++) {
      const a = centers[i];
      const b = centers[j];
      if (Math.hypot(a.x - b.x, a.y - b.y) < threshold) {
        overlapping.add(i);
        overlapping.add(j);
      }
    }
  }
  return overlapping;
}
