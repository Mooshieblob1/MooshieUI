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

export const MAGNITUDE_MIN = 1;
export const MAGNITUDE_MAX = 5;
/** The middle of the range, and what NovelAI's own panel opens on. */
export const MAGNITUDE_DEFAULT = 3;

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
