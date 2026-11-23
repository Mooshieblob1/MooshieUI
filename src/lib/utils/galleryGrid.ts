/**
 * Pure layout and selection maths for the gallery grid.
 *
 * The grid is windowed: a gallery of thousands of images is laid out as rows
 * of known height, and only the rows near the viewport are in the DOM.
 */

export interface GalleryGroup<T> {
  label: string;
  images: T[];
}

export type GalleryRow<T> =
  | { kind: "header"; key: string; label: string; images: T[] }
  | { kind: "tiles"; key: string; images: T[] };

/** Flatten groups into header rows and rows of at most `columns` tiles. */
export function buildGalleryRows<T>(
  groups: GalleryGroup<T>[],
  columns: number,
  showHeaders: boolean,
): GalleryRow<T>[] {
  const perRow = Math.max(1, Math.floor(columns));
  const rows: GalleryRow<T>[] = [];
  groups.forEach((group, g) => {
    if (group.images.length === 0) return;
    if (showHeaders) rows.push({ kind: "header", key: `h${g}`, label: group.label, images: group.images });
    for (let i = 0; i < group.images.length; i += perRow) {
      rows.push({ kind: "tiles", key: `t${g}:${i}`, images: group.images.slice(i, i + perRow) });
    }
  });
  return rows;
}

/**
 * Top offset of every row plus the total height as the last entry, so
 * `offsets[i + 1] - offsets[i]` is row i's height.
 */
export function rowOffsets<T>(rows: GalleryRow<T>[], tileRowHeight: number, headerHeight: number): number[] {
  const offsets = new Array<number>(rows.length + 1);
  let top = 0;
  for (let i = 0; i < rows.length; i++) {
    offsets[i] = top;
    top += rows[i]!.kind === "header" ? headerHeight : tileRowHeight;
  }
  offsets[rows.length] = top;
  return offsets;
}

/**
 * The half-open range of rows to render for a viewport that starts `top`
 * pixels into the list and is `height` tall, padded by `overscan` pixels on
 * both sides so a scroll does not show blank rows before the next render.
 */
export function visibleRowRange(
  offsets: number[],
  top: number,
  height: number,
  overscan: number,
): { start: number; end: number } {
  const count = offsets.length - 1;
  if (count <= 0) return { start: 0, end: 0 };
  const from = top - overscan;
  const to = top + height + overscan;
  // First row whose bottom edge is below `from`.
  let lo = 0;
  let hi = count;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (offsets[mid + 1]! <= from) lo = mid + 1;
    else hi = mid;
  }
  const start = Math.min(lo, count - 1);
  let end = start;
  while (end < count && offsets[end]! < to) end++;
  return { start, end: Math.max(end, start + 1) };
}

/**
 * The items between `anchor` and `target` in `order`, both included, for a
 * Shift+click. Falls back to just `target` when the anchor is gone (filtered
 * out or deleted since it was clicked).
 */
export function selectionRange<T>(order: T[], anchor: T | null, target: T): T[] {
  const to = order.indexOf(target);
  if (to === -1) return [];
  const from = anchor === null ? -1 : order.indexOf(anchor);
  if (from === -1) return [target];
  return order.slice(Math.min(from, to), Math.max(from, to) + 1);
}
