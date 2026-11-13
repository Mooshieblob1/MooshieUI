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
});

export const ADJUSTMENT_KEYS = Object.keys(NEUTRAL_ADJUSTMENTS) as (keyof Adjustments)[];

export function isNeutral(a: Adjustments): boolean {
  return ADJUSTMENT_KEYS.every((k) => a[k] === 0);
}

export function sameAdjustments(a: Adjustments, b: Adjustments): boolean {
  return ADJUSTMENT_KEYS.every((k) => a[k] === b[k]);
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
