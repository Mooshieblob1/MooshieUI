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
  /** img2img / infill strength. Pass 1.0 for txt2img. */
  strength: number;
  /** Opus makes a single small, short, batch-of-one request free. */
  isOpus: boolean;
  /** Vibes with no cached encoding, each of which is billed an encode. */
  vibeEncodes: number;
}

/** Per-vibe encode charge on V4 and later. Cached `.naiv4vibe` files are free. */
export const VIBE_ENCODE_COST = 2;

/**
 * The pixel ceiling for a free Opus generation. Hard, and measured in total
 * pixels, so 1088x1088 is over it even though neither side reaches 1152.
 */
export const OPUS_FREE_PIXELS = 1024 * 1024;
/** The step cap for a free Opus generation. 29 steps is charged in full. */
export const OPUS_FREE_STEPS = 28;

/** NovelAI clamps anything smaller than this up before pricing it. */
const MIN_BILLED_PIXELS = 65536;

/** NovelAI's own per-pixel and per-pixel-per-step coefficients. */
const PIXEL_COEFFICIENT = 2951823174884865e-21;
const PIXEL_STEP_COEFFICIENT = 5.753298233447344e-7;

/** A generation is never billed less than this per sample. */
const MIN_PER_SAMPLE = 2;

/**
 * The surcharge NovelAI applies on top of the raw pixel cost.
 *
 * It sits in the same slot NovelAI's cost function reserved for the old SMEA
 * factors (1.2 SMEA, 1.4 SMEA-dyn), applied to the *rounded* pixel cost rather
 * than the raw one, which is why the rounding below happens twice. Confirmed
 * against two real charges, so it is not guesswork:
 *
 *   1088x1088 @ 28 steps -> ceil(ceil(22.563) * 1.5) = 35 Anlas
 *   832x1216  @ 29 steps -> ceil(ceil(19.866) * 1.5) = 30 Anlas
 *
 * Applied to every model the app offers, all of which are V4 or later. If a
 * future model prices differently this becomes a per-model lookup.
 */
const COST_FACTOR = 1.5;

/**
 * True when Opus covers this request outright.
 *
 * Both limits are hard, and both were confirmed against real charges: at 29
 * steps, or above one megapixel, NovelAI bills the full price. The batch rule
 * lives in `estimateNovelAiCost`, because it is a property of the request
 * rather than of the dimensions this function is also asked about on its own.
 */
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
  const rawPixels =
    Math.max(0, Math.floor(input.width)) *
    Math.max(0, Math.floor(input.height));
  if (rawPixels === 0) return 0;
  const pixels = Math.max(MIN_BILLED_PIXELS, rawPixels);
  const steps = Math.max(1, Math.floor(input.steps));

  // Rounded first, then surcharged, then rounded again. Collapsing the two
  // roundings into one is off by an Anlas on both of the confirmed charges.
  let perSample =
    Math.ceil(
      PIXEL_COEFFICIENT * pixels + PIXEL_STEP_COEFFICIENT * pixels * steps,
    ) * COST_FACTOR;
  // Strength only shortens an img2img run, so it scales the sampling cost.
  const strength = Math.min(1, Math.max(0, input.strength));
  if (strength < 1) perSample *= strength;
  return Math.max(MIN_PER_SAMPLE, Math.ceil(perSample));
}

/**
 * Total Anlas this request is expected to cost, Opus discount included.
 *
 * The discount is all-or-nothing on a batch of one. NovelAI's rule is that a
 * free generation is made "one at a time", so a batch of two is not one free
 * sample plus one paid one: every sample is charged.
 */
export function estimateNovelAiCost(input: NovelAiCostInput): number {
  const samples = Math.max(1, Math.floor(input.nSamples));
  const perSample = novelAiCostPerSample(input);
  const free =
    samples === 1 &&
    novelAiOpusCovers(input.width, input.height, input.steps, input.isOpus);
  const encodes = Math.max(0, Math.floor(input.vibeEncodes)) * VIBE_ENCODE_COST;
  return (free ? 0 : samples * perSample) + encodes;
}
