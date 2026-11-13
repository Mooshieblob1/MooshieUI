/**
 * Pure logic for the built-in image editor: colour adjustment maths, blur and
 * sharpen on raw RGBA, crop rectangle geometry and the edit note that is saved
 * into the image's metadata. No DOM access, so `scripts/tests` can run it.
 */

export interface Adjustments {
  /** -100..100 */
  brightness: number;
  /** -100..100 */
  contrast: number;
  /** -100..100 */
  saturation: number;
  /** -180..180 degrees */
  hue: number;
  /** -100 (cool) .. 100 (warm) */
  temperature: number;
  /** -100 (green) .. 100 (magenta) */
  tint: number;
  /** 0..100 */
  sharpen: number;
  /** 0..100 */
  blur: number;
  /** -100..100, recovers (negative) or lifts the bright tones */
  highlights: number;
  /** -100..100, deepens or opens up the dark tones */
  shadows: number;
  /** -100..100, saturation that favours the muted colours */
  vibrance: number;
  /** -100 (dark corners) .. 100 (light corners) */
  vignette: number;
  /** Tone curve control points, x and y in 0..255, sorted by x. */
  curve: CurvePoint[];
}

export type CurvePoint = [number, number];

/** The sliders: every adjustment that is a single number. */
export type AdjustmentKey = Exclude<keyof Adjustments, "curve">;

export const IDENTITY_CURVE: readonly CurvePoint[] = Object.freeze([
  Object.freeze([0, 0]) as unknown as CurvePoint,
  Object.freeze([255, 255]) as unknown as CurvePoint,
]);

export function isIdentityCurve(curve: readonly CurvePoint[]): boolean {
  return curve.every(([x, y]) => x === y);
}

function sameCurve(a: readonly CurvePoint[], b: readonly CurvePoint[]): boolean {
  return a.length === b.length && a.every((p, i) => p[0] === b[i][0] && p[1] === b[i][1]);
}

export const NEUTRAL_ADJUSTMENTS: Readonly<Adjustments> = Object.freeze({
  brightness: 0,
  contrast: 0,
  saturation: 0,
  hue: 0,
  temperature: 0,
  tint: 0,
  sharpen: 0,
  blur: 0,
  highlights: 0,
  shadows: 0,
  vibrance: 0,
  vignette: 0,
  curve: IDENTITY_CURVE as CurvePoint[],
});

export const ADJUSTMENT_KEYS = (Object.keys(NEUTRAL_ADJUSTMENTS) as (keyof Adjustments)[]).filter(
  (k): k is AdjustmentKey => k !== "curve",
);

export function isNeutral(a: Adjustments): boolean {
  return ADJUSTMENT_KEYS.every((k) => a[k] === 0) && isIdentityCurve(a.curve);
}

export function sameAdjustments(a: Adjustments, b: Adjustments): boolean {
  return ADJUSTMENT_KEYS.every((k) => a[k] === b[k]) && sameCurve(a.curve, b.curve);
}

/**
 * Clean up adjustments read from storage (a saved preset): unknown keys are
 * dropped, numbers clamped, and a malformed curve becomes the identity.
 */
export function normalizeAdjustments(raw: unknown): Adjustments {
  const out: Adjustments = { ...NEUTRAL_ADJUSTMENTS, curve: IDENTITY_CURVE.map((p) => [p[0], p[1]] as CurvePoint) };
  if (!raw || typeof raw !== "object") return out;
  const source = raw as Record<string, unknown>;
  for (const key of ADJUSTMENT_KEYS) {
    const value = source[key];
    if (typeof value !== "number" || !Number.isFinite(value)) continue;
    const min = key === "hue" ? -180 : key === "sharpen" || key === "blur" ? 0 : -100;
    const max = key === "hue" ? 180 : 100;
    out[key] = Math.round(Math.min(max, Math.max(min, value)));
  }
  if (Array.isArray(source.curve)) {
    const points: CurvePoint[] = [];
    for (const p of source.curve) {
      if (!Array.isArray(p) || p.length !== 2) continue;
      const [x, y] = p as unknown[];
      if (typeof x !== "number" || typeof y !== "number" || !Number.isFinite(x) || !Number.isFinite(y)) continue;
      points.push([Math.round(Math.min(255, Math.max(0, x))), Math.round(Math.min(255, Math.max(0, y)))]);
    }
    points.sort((a, b) => a[0] - b[0]);
    const unique = points.filter((p, i) => i === 0 || p[0] !== points[i - 1][0]);
    if (unique.length >= 2) out.curve = unique;
  }
  return out;
}

/** Row-major 3x4 affine colour transform: out = M * [r, g, b, 1]. */
export type ColorMatrix = [
  number, number, number, number,
  number, number, number, number,
  number, number, number, number,
];

const LUMA_R = 0.2126;
const LUMA_G = 0.7152;
const LUMA_B = 0.0722;

function multiply(a: ColorMatrix, b: ColorMatrix): ColorMatrix {
  // Result applies `b` first, then `a`.
  const out = new Array<number>(12) as ColorMatrix;
  for (let row = 0; row < 3; row++) {
    const a0 = a[row * 4];
    const a1 = a[row * 4 + 1];
    const a2 = a[row * 4 + 2];
    const a3 = a[row * 4 + 3];
    for (let col = 0; col < 4; col++) {
      out[row * 4 + col] = a0 * b[col] + a1 * b[4 + col] + a2 * b[8 + col] + (col === 3 ? a3 : 0);
    }
  }
  return out;
}

/**
 * Every colour slider is an affine transform of RGB, so the whole stack folds
 * into one matrix and a pixel is touched once whatever the slider count.
 * Order: temperature/tint, hue, saturation, contrast, brightness.
 */
export function colorMatrix(a: Adjustments): ColorMatrix {
  let m: ColorMatrix = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0];

  if (a.temperature !== 0 || a.tint !== 0) {
    const t = a.temperature * 0.5;
    const g = a.tint * 0.5;
    // Warm adds red and removes blue; magenta tint removes green.
    m = multiply([1, 0, 0, t, 0, 1, 0, -g, 0, 0, 1, -t], m);
  }

  if (a.hue !== 0) {
    const rad = (a.hue * Math.PI) / 180;
    const c = Math.cos(rad);
    const s = Math.sin(rad);
    // The CSS/SVG hueRotate matrix: a rotation about the grey axis.
    m = multiply(
      [
        0.213 + c * 0.787 - s * 0.213, 0.715 - c * 0.715 - s * 0.715, 0.072 - c * 0.072 + s * 0.928, 0,
        0.213 - c * 0.213 + s * 0.143, 0.715 + c * 0.285 + s * 0.14, 0.072 - c * 0.072 - s * 0.283, 0,
        0.213 - c * 0.213 - s * 0.787, 0.715 - c * 0.715 + s * 0.715, 0.072 + c * 0.928 + s * 0.072, 0,
      ],
      m,
    );
  }

  if (a.saturation !== 0) {
    const s = 1 + a.saturation / 100;
    const r = LUMA_R * (1 - s);
    const g = LUMA_G * (1 - s);
    const b = LUMA_B * (1 - s);
    m = multiply([r + s, g, b, 0, r, g + s, b, 0, r, g, b + s, 0], m);
  }

  if (a.contrast !== 0) {
    // Negative values flatten towards mid grey; positive values go up to 2.5x.
    const k = a.contrast < 0 ? 1 + a.contrast / 100 : 1 + (a.contrast / 100) * 1.5;
    const o = 128 * (1 - k);
    m = multiply([k, 0, 0, o, 0, k, 0, o, 0, 0, k, o], m);
  }

  if (a.brightness !== 0) {
    const o = a.brightness * 1.275;
    m = multiply([1, 0, 0, o, 0, 1, 0, o, 0, 0, 1, o], m);
  }

  return m;
}

/** Apply a colour matrix to RGBA bytes in place. Alpha is left alone. */
export function applyColorMatrix(data: Uint8ClampedArray, m: ColorMatrix): void {
  const [m0, m1, m2, m3, m4, m5, m6, m7, m8, m9, m10, m11] = m;
  for (let i = 0; i < data.length; i += 4) {
    const r = data[i];
    const g = data[i + 1];
    const b = data[i + 2];
    data[i] = m0 * r + m1 * g + m2 * b + m3;
    data[i + 1] = m4 * r + m5 * g + m6 * b + m7;
    data[i + 2] = m8 * r + m9 * g + m10 * b + m11;
  }
}

/** Box widths whose repeated blur approximates a gaussian of `sigma`. */
function boxesForGauss(sigma: number, n: number): number[] {
  const ideal = Math.sqrt((12 * sigma * sigma) / n + 1);
  let low = Math.floor(ideal);
  if (low % 2 === 0) low--;
  const high = low + 2;
  const lowCount = Math.round((12 * sigma * sigma - n * low * low - 4 * n * low - 3 * n) / (-4 * low - 4));
  return Array.from({ length: n }, (_, i) => (i < lowCount ? low : high));
}

/** One box blur pass along rows (`horizontal`) or columns, edges clamped. */
function boxPass(
  src: Uint8ClampedArray,
  dst: Uint8ClampedArray,
  width: number,
  height: number,
  radius: number,
  horizontal: boolean,
): void {
  const lines = horizontal ? height : width;
  const length = horizontal ? width : height;
  const step = horizontal ? 4 : width * 4;
  const lineStep = horizontal ? width * 4 : 4;
  const window = radius * 2 + 1;
  for (let line = 0; line < lines; line++) {
    const base = line * lineStep;
    for (let channel = 0; channel < 4; channel++) {
      const start = base + channel;
      const first = src[start];
      const last = src[start + (length - 1) * step];
      let sum = first * radius;
      for (let i = 0; i < radius; i++) sum += src[start + Math.min(i, length - 1) * step];
      for (let i = 0; i < length; i++) {
        const add = i + radius < length ? src[start + (i + radius) * step] : last;
        sum += add;
        dst[start + i * step] = sum / window;
        const remove = i - radius >= 0 ? src[start + (i - radius) * step] : first;
        sum -= remove;
      }
    }
  }
}

/** Gaussian blur (three box passes) of RGBA bytes, in place. */
export function gaussianBlur(data: Uint8ClampedArray, width: number, height: number, sigma: number): void {
  if (sigma <= 0.05 || width < 1 || height < 1) return;
  const scratch = new Uint8ClampedArray(data.length);
  for (const box of boxesForGauss(sigma, 3)) {
    const radius = Math.max(0, (box - 1) >> 1);
    if (radius === 0) continue;
    boxPass(data, scratch, width, height, radius, true);
    boxPass(scratch, data, width, height, radius, false);
  }
}

/** Unsharp mask: push each pixel away from its blurred neighbourhood. */
export function unsharpMask(
  data: Uint8ClampedArray,
  width: number,
  height: number,
  amount: number,
  sigma: number,
): void {
  if (amount <= 0) return;
  const blurred = new Uint8ClampedArray(data);
  gaussianBlur(blurred, width, height, sigma);
  for (let i = 0; i < data.length; i += 4) {
    data[i] = data[i] + (data[i] - blurred[i]) * amount;
    data[i + 1] = data[i + 1] + (data[i + 1] - blurred[i + 1]) * amount;
    data[i + 2] = data[i + 2] + (data[i + 2] - blurred[i + 2]) * amount;
  }
}

/** Blur slider (0..100) to gaussian sigma in full-resolution pixels. */
export function blurSigma(blur: number): number {
  return (blur / 100) * 12;
}

/**
 * A 256-entry lookup table through the curve's control points, using a
 * monotone cubic (Fritsch-Carlson) so the curve never overshoots between two
 * points. Input below the first point or above the last holds that point's y.
 */
export function curveLut(curve: readonly CurvePoint[]): Uint8ClampedArray {
  const lut = new Uint8ClampedArray(256);
  const n = curve.length;
  if (n === 0) {
    for (let i = 0; i < 256; i++) lut[i] = i;
    return lut;
  }
  if (n === 1) {
    lut.fill(curve[0][1]);
    return lut;
  }
  const xs = curve.map((p) => p[0]);
  const ys = curve.map((p) => p[1]);
  const slopes: number[] = [];
  for (let i = 0; i < n - 1; i++) slopes.push((ys[i + 1] - ys[i]) / Math.max(1e-6, xs[i + 1] - xs[i]));
  const tangents: number[] = [slopes[0]];
  for (let i = 1; i < n - 1; i++) {
    tangents.push(slopes[i - 1] * slopes[i] <= 0 ? 0 : (slopes[i - 1] + slopes[i]) / 2);
  }
  tangents.push(slopes[n - 2]);
  for (let i = 0; i < n - 1; i++) {
    if (slopes[i] === 0) {
      tangents[i] = 0;
      tangents[i + 1] = 0;
      continue;
    }
    const a = tangents[i] / slopes[i];
    const b = tangents[i + 1] / slopes[i];
    const length = a * a + b * b;
    if (length > 9) {
      const t = 3 / Math.sqrt(length);
      tangents[i] = t * a * slopes[i];
      tangents[i + 1] = t * b * slopes[i];
    }
  }
  let segment = 0;
  for (let x = 0; x < 256; x++) {
    if (x <= xs[0]) {
      lut[x] = ys[0];
      continue;
    }
    if (x >= xs[n - 1]) {
      lut[x] = ys[n - 1];
      continue;
    }
    while (segment < n - 2 && x > xs[segment + 1]) segment++;
    const h = xs[segment + 1] - xs[segment];
    const t = (x - xs[segment]) / h;
    const t2 = t * t;
    const t3 = t2 * t;
    lut[x] =
      (2 * t3 - 3 * t2 + 1) * ys[segment] +
      (t3 - 2 * t2 + t) * h * tangents[segment] +
      (-2 * t3 + 3 * t2) * ys[segment + 1] +
      (t3 - t2) * h * tangents[segment + 1];
  }
  return lut;
}

/**
 * The tone tools that are not a plain matrix: shadows, highlights, vibrance,
 * the tone curve and the vignette. One pass over the pixels, in place.
 */
export function applyTone(data: Uint8ClampedArray, width: number, height: number, a: Adjustments): void {
  const shadows = a.shadows / 100;
  const highlights = a.highlights / 100;
  const vibrance = a.vibrance / 100;
  const vignette = a.vignette / 100;
  const lut = isIdentityCurve(a.curve) ? null : curveLut(a.curve);
  const tonal = shadows !== 0 || highlights !== 0;
  if (!tonal && vibrance === 0 && vignette === 0 && !lut) return;

  const cx = (width - 1) / 2;
  const cy = (height - 1) / 2;
  const maxDist2 = Math.max(1, cx * cx + cy * cy);
  // 27/4 scales both weights to peak at exactly 1.
  const PEAK = 6.75;

  let i = 0;
  for (let y = 0; y < height; y++) {
    const dy2 = (y - cy) * (y - cy);
    for (let x = 0; x < width; x++, i += 4) {
      let r = data[i];
      let g = data[i + 1];
      let b = data[i + 2];

      if (tonal) {
        const l = (LUMA_R * r + LUMA_G * g + LUMA_B * b) / 255;
        // Shadows act around the lower third, highlights around the upper
        // third; both fade to nothing at pure black and pure white.
        const delta = 90 * PEAK * (shadows * l * (1 - l) * (1 - l) + highlights * l * l * (1 - l));
        r += delta;
        g += delta;
        b += delta;
      }

      if (vibrance !== 0) {
        const max = Math.max(r, g, b);
        const min = Math.min(r, g, b);
        const saturation = Math.min(1, Math.max(0, (max - min) / 255));
        const k = 1 + vibrance * (1 - saturation);
        const l = LUMA_R * r + LUMA_G * g + LUMA_B * b;
        r = l + (r - l) * k;
        g = l + (g - l) * k;
        b = l + (b - l) * k;
      }

      if (lut) {
        r = lut[r < 0 ? 0 : r > 255 ? 255 : Math.round(r)];
        g = lut[g < 0 ? 0 : g > 255 ? 255 : Math.round(g)];
        b = lut[b < 0 ? 0 : b > 255 ? 255 : Math.round(b)];
      }

      if (vignette !== 0) {
        const d = Math.sqrt(((x - cx) * (x - cx) + dy2) / maxDist2);
        // Untouched centre, smooth falloff to the corners.
        const t = d <= 0.3 ? 0 : (d - 0.3) / 0.7;
        const w = t * t * (3 - 2 * t);
        if (vignette < 0) {
          const k = 1 + vignette * w * 0.85;
          r *= k;
          g *= k;
          b *= k;
        } else {
          const k = vignette * w * 0.85;
          r += (255 - r) * k;
          g += (255 - g) * k;
          b += (255 - b) * k;
        }
      }

      data[i] = r;
      data[i + 1] = g;
      data[i + 2] = b;
    }
  }
}

/**
 * Apply the whole adjustment stack to RGBA bytes in place. `scale` is the
 * ratio of this buffer to the full-resolution image (1 for the export, less
 * for the on-screen preview), so blur and sharpen look the same at both.
 */
export function applyAdjustments(
  data: Uint8ClampedArray,
  width: number,
  height: number,
  a: Adjustments,
  scale = 1,
): void {
  if (a.blur > 0) gaussianBlur(data, width, height, blurSigma(a.blur) * scale);
  if (a.sharpen > 0) unsharpMask(data, width, height, (a.sharpen / 100) * 1.5, Math.max(0.4, 1.2 * scale));
  if (
    a.brightness !== 0 ||
    a.contrast !== 0 ||
    a.saturation !== 0 ||
    a.hue !== 0 ||
    a.temperature !== 0 ||
    a.tint !== 0
  ) {
    applyColorMatrix(data, colorMatrix(a));
  }
  applyTone(data, width, height, a);
}

// ---------------------------------------------------------------------------
// Crop geometry
// ---------------------------------------------------------------------------

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type CropHandle = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw" | "move" | "new";

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/**
 * The rectangle spanned from a fixed anchor to a dragged point, kept inside a
 * `width` x `height` image and, when `ratio` (w / h) is set, locked to it.
 */
export function rectFromAnchor(
  ax: number,
  ay: number,
  px: number,
  py: number,
  width: number,
  height: number,
  ratio: number | null,
): Rect {
  ax = clamp(ax, 0, width);
  ay = clamp(ay, 0, height);
  px = clamp(px, 0, width);
  py = clamp(py, 0, height);
  const dirX = px >= ax ? 1 : -1;
  const dirY = py >= ay ? 1 : -1;
  let w = Math.abs(px - ax);
  let h = Math.abs(py - ay);
  if (ratio && ratio > 0) {
    const maxW = dirX > 0 ? width - ax : ax;
    const maxH = dirY > 0 ? height - ay : ay;
    w = Math.min(Math.max(w, h * ratio), maxW, maxH * ratio);
    h = w / ratio;
  }
  return { x: dirX > 0 ? ax : ax - w, y: dirY > 0 ? ay : ay - h, w, h };
}

/** A crop rectangle after dragging `handle` by (dx, dy) image pixels. */
export function dragCropRect(
  start: Rect,
  handle: CropHandle,
  dx: number,
  dy: number,
  width: number,
  height: number,
  ratio: number | null,
): Rect {
  const left = start.x;
  const top = start.y;
  const right = start.x + start.w;
  const bottom = start.y + start.h;

  if (handle === "move") {
    return {
      x: clamp(left + dx, 0, Math.max(0, width - start.w)),
      y: clamp(top + dy, 0, Math.max(0, height - start.h)),
      w: start.w,
      h: start.h,
    };
  }
  if (handle === "new") {
    return rectFromAnchor(left, top, left + dx, top + dy, width, height, ratio);
  }
  if (handle.length === 2) {
    const ax = handle.includes("w") ? right : left;
    const ay = handle.includes("n") ? bottom : top;
    const px = (handle.includes("w") ? left : right) + dx;
    const py = (handle.includes("n") ? top : bottom) + dy;
    return rectFromAnchor(ax, ay, px, py, width, height, ratio);
  }

  // Edge handles move one side; with a locked ratio the other axis grows
  // around the rectangle's centre line.
  if (handle === "e" || handle === "w") {
    const anchor = handle === "e" ? left : right;
    const moved = clamp((handle === "e" ? right : left) + dx, 0, width);
    let w = Math.abs(moved - anchor);
    const x = () => (moved >= anchor ? anchor : anchor - w);
    if (!ratio) return { x: x(), y: top, w, h: start.h };
    const cy = top + start.h / 2;
    const maxH = 2 * Math.min(cy, height - cy);
    w = Math.min(w, maxH * ratio);
    const h = w / ratio;
    return { x: x(), y: cy - h / 2, w, h };
  }
  const anchor = handle === "s" ? top : bottom;
  const moved = clamp((handle === "s" ? bottom : top) + dy, 0, height);
  let h = Math.abs(moved - anchor);
  const y = () => (moved >= anchor ? anchor : anchor - h);
  if (!ratio) return { x: left, y: y(), w: start.w, h };
  const cx = left + start.w / 2;
  const maxW = 2 * Math.min(cx, width - cx);
  h = Math.min(h, maxW / ratio);
  const w = h * ratio;
  return { x: cx - w / 2, y: y(), w, h };
}

/** The largest centred rectangle of `ratio` (w / h) inside the image. */
export function fitAspectRect(width: number, height: number, ratio: number | null): Rect {
  if (!ratio || ratio <= 0) return { x: 0, y: 0, w: width, h: height };
  let w = width;
  let h = w / ratio;
  if (h > height) {
    h = height;
    w = h * ratio;
  }
  return { x: (width - w) / 2, y: (height - h) / 2, w, h };
}

/** Snap a crop rectangle to whole pixels inside the image, at least 1x1. */
export function roundRect(rect: Rect, width: number, height: number): Rect {
  const x = clamp(Math.round(rect.x), 0, Math.max(0, width - 1));
  const y = clamp(Math.round(rect.y), 0, Math.max(0, height - 1));
  const w = clamp(Math.round(rect.w), 1, width - x);
  const h = clamp(Math.round(rect.h), 1, height - y);
  return { x, y, w, h };
}

// ---------------------------------------------------------------------------
// Edit note
// ---------------------------------------------------------------------------

export type EditOp =
  | { op: "crop"; w: number; h: number }
  | { op: "rotate"; deg: 90 | -90 }
  | { op: "flip"; axis: "h" | "v" }
  | { op: "resize"; w: number; h: number };

/** Keep the note small enough for a PNG text chunk and a stealth payload. */
const MAX_NOTE_OPS = 40;

/**
 * The compact JSON list stored as `mooshie_edits`: the pixel operations in
 * order, then one entry for the adjustment sliders that are not neutral.
 */
export function buildEditNote(ops: EditOp[], adjustments: Adjustments, earlier?: string): string {
  const list: Record<string, unknown>[] = [];
  // Editing an already edited image appends to its note.
  if (earlier) {
    try {
      const parsed: unknown = JSON.parse(earlier);
      if (Array.isArray(parsed)) {
        for (const item of parsed) {
          if (item && typeof item === "object" && !Array.isArray(item)) list.push(item as Record<string, unknown>);
        }
      }
    } catch {
      // A note that does not parse is dropped rather than carried along.
    }
  }
  for (const op of ops) list.push({ ...op });
  if (!isNeutral(adjustments)) {
    const entry: Record<string, unknown> = { op: "adjust" };
    for (const key of ADJUSTMENT_KEYS) {
      if (adjustments[key] !== 0) entry[key] = adjustments[key];
    }
    if (!isIdentityCurve(adjustments.curve)) entry.curve = adjustments.curve;
    list.push(entry);
  }
  return JSON.stringify(list.slice(-MAX_NOTE_OPS));
}

/**
 * The metadata map written with an edited image: the source's generation
 * parameters untouched, plus the edit note. `null` when the source carried no
 * metadata, so an edited photo is not given an empty parameter block.
 */
export function editedMetadata(
  source: Record<string, string> | null | undefined,
  ops: EditOp[],
  adjustments: Adjustments,
  size: { width: number; height: number },
  editedFrom: string | null,
  date: string,
): Record<string, string> | null {
  if (!source || Object.keys(source).length === 0) return null;
  const out: Record<string, string> = { ...source };
  out.mooshie_edits = buildEditNote(ops, adjustments, source.mooshie_edits);
  out.mooshie_edit_size = `${size.width}x${size.height}`;
  out.mooshie_edit_date = date;
  // An edit of an edit keeps pointing at the first original.
  if (!out.mooshie_edited_from && editedFrom) out.mooshie_edited_from = editedFrom;
  return out;
}
