/**
 * Anlas cost estimate for a NovelAI request.
 *
 * The constants are NovelAI's own, from the cost function its web client ships,
 * cross-checked against the community `novelai-api` client. Treat the result as
 * an **estimate**: NovelAI is authoritative, the balance readout above the
 * generate button is what confirms an actual charge, and the badge that shows
 * this is prefixed with `~` for exactly that reason.
 *
 * This is a leaf util. It must not import a store.
 */

export interface NovelAiCostInput {
  width: number;
  height: number;
  steps: number;
  /** Batch size, i.e. NovelAI's `n_samples`. */
  nSamples: number;
  /** Anything other than 1.0 costs 30 percent more per sample. */
  uncondScale: number;
  /** img2img / infill strength. Pass 1.0 for txt2img. */
  strength: number;
  /** Opus makes the first sample of a small, short request free. */
  isOpus: boolean;
  /** Vibes with no cached encoding, each of which is billed an encode. */
  vibeEncodes: number;
}

/** Per-vibe encode charge on V4 and later. Cached `.naiv4vibe` files are free. */
export const VIBE_ENCODE_COST = 2;

/** Opus covers one free sample only up to this pixel count... */
export const OPUS_FREE_PIXELS = 1024 * 1024;
/** ...and only up to this many steps. */
export const OPUS_FREE_STEPS = 28;

/** NovelAI's own per-pixel and per-pixel-per-step coefficients. */
const PIXEL_COEFFICIENT = 2951823174884865e-21;
const PIXEL_STEP_COEFFICIENT = 5.753298233447344e-7;

/** A generation is never billed less than this per sample. */
const MIN_PER_SAMPLE = 2;

/** True when Opus covers one sample of this request outright. */
export function novelAiOpusCovers(
  width: number,
  height: number,
  steps: number,
  isOpus: boolean,
): boolean {
  return (
    isOpus && width * height <= OPUS_FREE_PIXELS && steps <= OPUS_FREE_STEPS
  );
}

/** Anlas a single sample of this request costs, before the Opus discount. */
export function novelAiCostPerSample(input: NovelAiCostInput): number {
  const pixels =
    Math.max(0, Math.floor(input.width)) *
    Math.max(0, Math.floor(input.height));
  const steps = Math.max(1, Math.floor(input.steps));
  if (pixels === 0) return 0;

  let perSample = Math.ceil(
    PIXEL_COEFFICIENT * pixels + PIXEL_STEP_COEFFICIENT * pixels * steps,
  );
  // Strength only shortens an img2img run, so it scales the sampling cost.
  const strength = Math.min(1, Math.max(0, input.strength));
  if (strength < 1) perSample = Math.ceil(perSample * strength);
  // An uncond scale off 1.0 runs an extra pass.
  if (input.uncondScale !== 1) perSample = Math.ceil(perSample * 1.3);
  return Math.max(MIN_PER_SAMPLE, perSample);
}

/** Total Anlas this request is expected to cost, Opus discount included. */
export function estimateNovelAiCost(input: NovelAiCostInput): number {
  const samples = Math.max(1, Math.floor(input.nSamples));
  const perSample = novelAiCostPerSample(input);
  // Opus makes the *first* sample free, not the whole batch.
  const billable = novelAiOpusCovers(
    input.width,
    input.height,
    input.steps,
    input.isOpus,
  )
    ? samples - 1
    : samples;
  const encodes = Math.max(0, Math.floor(input.vibeEncodes)) * VIBE_ENCODE_COST;
  return Math.max(0, billable) * perSample + encodes;
}
