/**
 * Geometry and denoise settings for NovelAI's image Enhance.
 *
 * Enhance is not a separate endpoint. It is an img2img pass at a larger canvas,
 * so everything here just decides the two numbers that pass needs -- the target
 * size and the strength/noise pair -- and the request goes out through the same
 * `novelai_generate` command a normal generation uses.
 *
 * This is a leaf util. It must not import a store.
 */

/**
 * The pixel ceiling Enhance will scale up to.
 *
 * NovelAI's own Enhance tops out at a 3MP result; its UI labels that button
 * "Max". Expressed in pixels rather than as a scale factor because the limit is
 * on total area, so how far a given image can grow depends on its aspect ratio.
 */
export const ENHANCE_MAX_PIXELS = 3 * 1024 * 1024;

/**
 * NovelAI's dimension grid, mirroring `snap_dimension` in `novelai/mod.rs`.
 *
 * The rounding has to match the backend's exactly. The modal quotes the target
 * resolution and prices it, and the backend re-snaps whatever it is sent, so a
 * different rule here would mean quoting a size that is not the one generated.
 */
const DIMENSION_STEP = 64;

/**
 * The fixed step NovelAI's own Enhance offers between 1x and Max.
 *
 * A named constant rather than a literal at the call site because two things
 * have to agree on it: the button that quotes the size and the check that
 * decides whether the button is worth showing at all.
 */
export const ENHANCE_MID_SCALE = 1.5;

/**
 * The `action` value that routes a request to NovelAI's standalone upscaler.
 *
 * Mirrors `UPSCALE_ACTION` in `novelai/mod.rs`. The backend treats anything
 * else as a generation, so a typo here would be billed as a whole new image
 * rather than rejected.
 */
export const UPSCALE_ACTION = "upscale";

/**
 * That upscaler is a fixed 4x model. Mirrors `UPSCALE_SCALE`.
 *
 * Nothing in the UI can change it: the endpoint takes the factor as a field and
 * accepts no other value.
 */
export const UPSCALE_FACTOR = 4;

/**
 * The largest image the upscaler accepts. Mirrors `UPSCALE_MAX_PIXELS`.
 *
 * An area rather than a pair of side limits, which is what lets a tall portrait
 * through despite being over 1536 on one side. The same number is where the
 * price table in `novelaiCost.ts` runs out, which is not a coincidence: past it
 * NovelAI's own client neither quotes nor sends. The backend enforces it too;
 * this copy exists so the button can explain itself before the click instead of
 * the request coming back rejected after it.
 */
export const UPSCALE_MAX_PIXELS = 3145728;

/** Whether the upscaler will take this image at all. */
export function upscaleFits(width: number, height: number): boolean {
  if (!(width > 0 && height > 0)) return false;
  return width * height <= UPSCALE_MAX_PIXELS;
}

/**
 * What a 4x upscale comes back as.
 *
 * Deliberately not snapped to the 64px grid. The upscaler is not a generation
 * and never sees that grid: it returns exactly four times the source, odd
 * dimensions included.
 */
export function upscaleTargetSize(
  width: number,
  height: number,
): { width: number; height: number } {
  return {
    width: Math.round(width * UPSCALE_FACTOR),
    height: Math.round(height * UPSCALE_FACTOR),
  };
}

/** How many variations one run asks for. */
export const VARIATION_COUNT_MIN = 1;
export const VARIATION_COUNT_MAX = 8;
/** NovelAI's own variation control asks for four. */
export const VARIATION_COUNT_DEFAULT = 4;

/**
 * How far a variation may stray from its source, as an img2img strength.
 *
 * The default is low enough that the results stay recognisably the same image,
 * and it is the value that reproduces the price NovelAI quotes for a set of
 * four off the default portrait.
 */
export const VARIETY_MIN = 0.1;
export const VARIETY_MAX = 0.9;
export const VARIETY_DEFAULT = 0.4;

export function clampVariationCount(value: number): number {
  if (!Number.isFinite(value)) return VARIATION_COUNT_DEFAULT;
  return Math.min(
    VARIATION_COUNT_MAX,
    Math.max(VARIATION_COUNT_MIN, Math.round(value)),
  );
}

export const MAGNITUDE_MIN = 1;
export const MAGNITUDE_MAX = 5;
/** The middle of the range, and what NovelAI's own panel opens on. */
export const MAGNITUDE_DEFAULT = 3;

/** Which of the Enhance modal's upscale buttons is selected. */
export type EnhanceScaleChoice = "1x" | "1.5x" | "max";

const ENHANCE_SCALE_CHOICES: readonly EnhanceScaleChoice[] = ["1x", "1.5x", "max"];

/** Type guard for a persisted scale choice, which may come from an older blob. */
export function isEnhanceScaleChoice(value: unknown): value is EnhanceScaleChoice {
  return (ENHANCE_SCALE_CHOICES as readonly unknown[]).includes(value);
}

export interface EnhanceDenoise {
  /** img2img strength. Higher redraws more of the image. */
  strength: number;
  /** Extra noise added before sampling. Higher invents more new detail. */
  noise: number;
}

/**
 * What each whole magnitude maps to.
 *
 * NovelAI documents only that "the Magnitude slider uses combinations of
 * Strength & Noise" and never publishes the table, so this is a reconstruction,
 * anchored on three things: NovelAI's panel shows Strength 0.5 / Noise 0 at the
 * default magnitude of 3; enhanced images found in the wild carry embedded
 * values up to strength 0.7 / noise 0.2; and the docs describe the bottom of
 * the range as reproducing the input almost exactly. Noise stays at zero over
 * the lower half because that is the half meant to preserve the original.
 *
 * Treat it as a sensible curve rather than NovelAI's literal one. Anyone who
 * wants the exact pair sets it in the advanced controls instead.
 */
const MAGNITUDE_TABLE: readonly EnhanceDenoise[] = [
  { strength: 0.3, noise: 0 }, // 1
  { strength: 0.4, noise: 0 }, // 2
  { strength: 0.5, noise: 0 }, // 3
  { strength: 0.6, noise: 0.1 }, // 4
  { strength: 0.7, noise: 0.2 }, // 5
];

/** Two decimals, so an interpolated 0.55 does not surface as 0.5500000000001. */
function round2(value: number): number {
  return Math.round(value * 100) / 100;
}

export function clampMagnitude(value: number): number {
  if (!Number.isFinite(value)) return MAGNITUDE_DEFAULT;
  return round2(Math.min(MAGNITUDE_MAX, Math.max(MAGNITUDE_MIN, value)));
}

/**
 * Round a pixel dimension onto NovelAI's 64px grid.
 *
 * Round-to-nearest and not floor, because that is what the backend does and the
 * quoted resolution has to be the one actually requested.
 */
export function snapDimension(px: number): number {
  const snapped = Math.round(px / DIMENSION_STEP) * DIMENSION_STEP;
  return Math.max(DIMENSION_STEP, snapped);
}

/** The canvas an enhance at `scale` will actually be generated at. */
export function enhanceTargetSize(
  width: number,
  height: number,
  scale: number,
): { width: number; height: number } {
  return {
    width: snapDimension(width * scale),
    height: snapDimension(height * scale),
  };
}

/**
 * Whether a fixed scale's snapped result still fits under the pixel ceiling.
 *
 * Checked on the snapped size and not on the raw multiplication, because
 * snapping rounds up as often as down: a 1.5x that computes to just under 3MP
 * can land just over it once both dimensions are on the 64px grid, and the
 * backend would then reject the request the modal had already priced.
 */
export function enhanceScaleFits(
  width: number,
  height: number,
  scale: number,
): boolean {
  if (!(width > 0 && height > 0)) return false;
  const target = enhanceTargetSize(width, height, scale);
  return target.width * target.height <= ENHANCE_MAX_PIXELS;
}

/**
 * The largest scale whose snapped result still fits under the pixel ceiling.
 *
 * Starting from the exact fit and walking down, rather than solving it, because
 * snapping rounds up as often as down and can push an exactly-fitting scale
 * back over the line. One percent per step lands within a pixel or two of the
 * grid boundary, which is closer than a 64px step can resolve anyway.
 *
 * Never returns less than 1: an image already past the ceiling has no room to
 * grow, and quoting "Max" as a downscale would be a worse answer than 1x.
 */
export function maxEnhanceScale(width: number, height: number): number {
  const pixels = width * height;
  if (!(pixels > 0)) return 1;
  let scale = Math.sqrt(ENHANCE_MAX_PIXELS / pixels);
  for (let i = 0; i < 100 && scale > 1; i++) {
    const target = enhanceTargetSize(width, height, scale);
    if (target.width * target.height <= ENHANCE_MAX_PIXELS) break;
    scale -= 0.01;
  }
  return Math.max(1, scale);
}

/**
 * The strength/noise pair a magnitude stands for.
 *
 * Interpolated between the whole steps so a typed 1.25 lands between 1 and 2
 * instead of snapping. The slider only ever produces whole numbers; the paired
 * number input is what makes the in-between values reachable.
 */
export function magnitudeToDenoise(magnitude: number): EnhanceDenoise {
  const m = clampMagnitude(magnitude);
  const lower = MAGNITUDE_TABLE[Math.floor(m) - MAGNITUDE_MIN];
  const upper = MAGNITUDE_TABLE[Math.ceil(m) - MAGNITUDE_MIN];
  const t = m - Math.floor(m);
  return {
    strength: round2(lower.strength + (upper.strength - lower.strength) * t),
    noise: round2(lower.noise + (upper.noise - lower.noise) * t),
  };
}
