export const BUCKET_DIMENSION_STEP = 64;

/**
 * Largest pair on the pixel grid whose area does not go over `area`.
 *
 * NovelAI prices by pixel count and Opus only covers a generation while it
 * stays at or under one megapixel, so rounding a ratio up past the requested
 * area turns a free 1024 generation into a paid one. SDXL-family models also
 * use these 64px under-area buckets (e.g. 3:2 at 1024 -> 1216x832). Walking
 * the width across a few grid steps and taking the tallest height that still
 * fits lands on those training buckets.
 */
export function dimsUnderArea(
  aw: number,
  ah: number,
  area: number,
  q: number,
): { w: number; h: number } {
  const target = aw / ah;
  let best: { w: number; h: number; err: number } | null = null;
  const mid = Math.round(Math.sqrt(area * target) / q);
  for (let k = Math.max(1, mid - 2); k <= mid + 2; k++) {
    const w = k * q;
    const h = Math.floor(area / w / q) * q;
    if (h < q) continue;
    // Scored in log space so a result that is too wide and one that is too
    // tall by the same factor are treated as equally far off.
    const err = Math.abs(Math.log(w / h / target));
    if (!best || err < best.err) best = { w, h, err };
  }
  return best ? { w: best.w, h: best.h } : { w: q, h: q };
}

/**
 * Compute pixel dimensions for an aspect ratio at the given side length.
 *
 * When `underArea` is true (NovelAI and SDXL-family models), snaps to the
 * largest `q`-aligned pair at or below `side * side` so presets land on
 * standard 64px training buckets. Otherwise picks the closest `q`-aligned
 * pair around `side * side`.
 */
export function dimsForAspect(
  aw: number,
  ah: number,
  side: number,
  q: number,
  underArea: boolean,
): { w: number; h: number } {
  const area = side * side;
  if (underArea) return dimsUnderArea(aw, ah, area, q);
  const wA = Math.max(q, Math.round(Math.sqrt(area * (aw / ah)) / q) * q);
  const hA = Math.max(q, Math.round(area / wA / q) * q);
  const hB = Math.max(q, Math.round(Math.sqrt(area * (ah / aw)) / q) * q);
  const wB = Math.max(q, Math.round(area / hB / q) * q);
  return Math.abs(wA * hA - area) <= Math.abs(wB * hB - area)
    ? { w: wA, h: hA }
    : { w: wB, h: hB };
}
