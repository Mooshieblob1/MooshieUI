// Comic page builder: the pure parts. Layouts, templates, reading order,
// panel sizes, prompt assembly and the continuity comparison. No stores and no
// DOM, so all of it is covered by scripts/tests/comicLayout.test.ts.
// The MCP server's create_comic tool mirrors the layouts, sizes and prompt
// rules in src-tauri/src/mcp/comic.rs; change them together.
//
// A layout is a binary split tree. A leaf is a panel; a split divides its box
// into two at `ratio`, side by side ("row", a vertical divider) or stacked
// ("col", a horizontal divider). Splitting a panel, merging it away and
// dragging a divider are all small tree edits, and any layout a template can
// make the editor can make too.

export type SplitDir = "row" | "col";

export type LayoutNode =
  | { type: "panel"; id: string }
  | { type: "split"; id: string; dir: SplitDir; ratio: number; a: LayoutNode; b: LayoutNode };

/** A box in page units: 0..1 on both axes, top-left origin. */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Divider {
  splitId: string;
  dir: SplitDir;
  /** The box the split divides, for turning a drag into a ratio. */
  box: Rect;
  /** Where the divider line sits, in page units. */
  pos: number;
}

export const MIN_RATIO = 0.12;
export const MAX_PANELS = 9;
/** NovelAI takes at most six character prompts, so a one-image page has six panels at most. */
export const ONE_SHOT_MAX_PANELS = 6;

let idCounter = 0;
/** Short unique id. Random part so ids from a saved page never collide with new ones. */
export function newId(prefix = "p"): string {
  idCounter = (idCounter + 1) % 1_000_000;
  return `${prefix}${Date.now().toString(36)}${idCounter.toString(36)}${Math.random().toString(36).slice(2, 6)}`;
}

// ---------------------------------------------------------------------------
// Tree helpers
// ---------------------------------------------------------------------------

export function panelIds(node: LayoutNode): string[] {
  return node.type === "panel" ? [node.id] : [...panelIds(node.a), ...panelIds(node.b)];
}

export function panelRects(node: LayoutNode, box: Rect = { x: 0, y: 0, w: 1, h: 1 }): Map<string, Rect> {
  const out = new Map<string, Rect>();
  const walk = (n: LayoutNode, r: Rect) => {
    if (n.type === "panel") {
      out.set(n.id, r);
      return;
    }
    const [ra, rb] = splitRect(r, n.dir, n.ratio);
    walk(n.a, ra);
    walk(n.b, rb);
  };
  walk(node, box);
  return out;
}

export function splitRect(r: Rect, dir: SplitDir, ratio: number): [Rect, Rect] {
  if (dir === "row") {
    const wa = r.w * ratio;
    return [
      { x: r.x, y: r.y, w: wa, h: r.h },
      { x: r.x + wa, y: r.y, w: r.w - wa, h: r.h },
    ];
  }
  const ha = r.h * ratio;
  return [
    { x: r.x, y: r.y, w: r.w, h: ha },
    { x: r.x, y: r.y + ha, w: r.w, h: r.h - ha },
  ];
}

export function dividers(node: LayoutNode, box: Rect = { x: 0, y: 0, w: 1, h: 1 }): Divider[] {
  if (node.type === "panel") return [];
  const [ra, rb] = splitRect(box, node.dir, node.ratio);
  const pos = node.dir === "row" ? box.x + box.w * node.ratio : box.y + box.h * node.ratio;
  return [{ splitId: node.id, dir: node.dir, box, pos }, ...dividers(node.a, ra), ...dividers(node.b, rb)];
}

function mapTree(node: LayoutNode, fn: (n: LayoutNode) => LayoutNode | null): LayoutNode {
  const replaced = fn(node);
  if (replaced) return replaced;
  if (node.type === "panel") return node;
  return { ...node, a: mapTree(node.a, fn), b: mapTree(node.b, fn) };
}

/** Split panel `id` in two. The new panel goes after it (right or below). */
export function splitPanel(node: LayoutNode, id: string, dir: SplitDir, newPanelId: string): LayoutNode {
  return mapTree(node, (n) =>
    n.type === "panel" && n.id === id
      ? { type: "split", id: newId("s"), dir, ratio: 0.5, a: n, b: { type: "panel", id: newPanelId } }
      : null,
  );
}

/** Remove panel `id`; its sibling takes the whole box. The last panel stays. */
export function removePanel(node: LayoutNode, id: string): LayoutNode {
  if (node.type === "panel") return node;
  if (node.a.type === "panel" && node.a.id === id) return node.b;
  if (node.b.type === "panel" && node.b.id === id) return node.a;
  return { ...node, a: removePanel(node.a, id), b: removePanel(node.b, id) };
}

export function setRatio(node: LayoutNode, splitId: string, ratio: number): LayoutNode {
  const clamped = Math.min(1 - MIN_RATIO, Math.max(MIN_RATIO, ratio));
  return mapTree(node, (n) => (n.type === "split" && n.id === splitId ? { ...n, ratio: clamped } : null));
}

/** Ratio for a divider dragged to `pointer` (page units on the divider's axis). */
export function ratioFromPointer(divider: Divider, pointer: number): number {
  const { box, dir } = divider;
  const start = dir === "row" ? box.x : box.y;
  const size = dir === "row" ? box.w : box.h;
  return size > 0 ? (pointer - start) / size : 0.5;
}

/** Whether a parsed value is a usable layout, for pages loaded from storage. */
export function isLayout(value: unknown, depth = 0): value is LayoutNode {
  if (!value || typeof value !== "object" || depth > 12) return false;
  const n = value as Record<string, unknown>;
  if (n.type === "panel") return typeof n.id === "string";
  return (
    n.type === "split" &&
    typeof n.id === "string" &&
    (n.dir === "row" || n.dir === "col") &&
    typeof n.ratio === "number" &&
    isLayout(n.a, depth + 1) &&
    isLayout(n.b, depth + 1)
  );
}

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

/** A template in shorthand: a panel is "P", a split is [dir, ratio, a, b]. */
type Shape = "P" | [SplitDir, number, Shape, Shape];

export interface LayoutTemplate {
  id: string;
  panels: number;
  shape: Shape;
}

const P = "P" as const;

export const LAYOUT_TEMPLATES: LayoutTemplate[] = [
  { id: "full", panels: 1, shape: P },
  { id: "two_rows", panels: 2, shape: ["col", 0.5, P, P] },
  { id: "two_columns", panels: 2, shape: ["row", 0.5, P, P] },
  { id: "three_rows", panels: 3, shape: ["col", 1 / 3, P, ["col", 0.5, P, P]] },
  { id: "one_over_two", panels: 3, shape: ["col", 0.5, P, ["row", 0.5, P, P]] },
  { id: "two_over_one", panels: 3, shape: ["col", 0.5, ["row", 0.5, P, P], P] },
  { id: "tall_left_two_right", panels: 3, shape: ["row", 0.55, P, ["col", 0.5, P, P]] },
  { id: "four_rows", panels: 4, shape: ["col", 0.25, P, ["col", 1 / 3, P, ["col", 0.5, P, P]]] },
  { id: "grid_2x2", panels: 4, shape: ["col", 0.5, ["row", 0.5, P, P], ["row", 0.5, P, P]] },
  { id: "one_two_one", panels: 4, shape: ["col", 0.3, P, ["col", 0.55, ["row", 0.5, P, P], P]] },
  { id: "one_one_two", panels: 4, shape: ["col", 0.3, P, ["col", 0.45, P, ["row", 0.5, P, P]]] },
  { id: "two_one_two", panels: 5, shape: ["col", 0.32, ["row", 0.6, P, P], ["col", 0.5, P, ["row", 0.4, P, P]]] },
  { id: "one_two_two", panels: 5, shape: ["col", 0.3, P, ["col", 0.5, ["row", 0.5, P, P], ["row", 0.6, P, P]]] },
  { id: "grid_2x3", panels: 6, shape: ["col", 1 / 3, ["row", 0.5, P, P], ["col", 0.5, ["row", 0.5, P, P], ["row", 0.5, P, P]]] },
  { id: "two_one_three", panels: 6, shape: ["col", 0.3, ["row", 0.6, P, P], ["col", 0.45, P, ["row", 1 / 3, P, ["row", 0.5, P, P]]]] },
];

export function buildTemplate(template: LayoutTemplate, ids: () => string = () => newId("p")): LayoutNode {
  const build = (shape: Shape): LayoutNode =>
    shape === "P"
      ? { type: "panel", id: ids() }
      : { type: "split", id: newId("s"), dir: shape[0], ratio: shape[1], a: build(shape[2]), b: build(shape[3]) };
  return build(template.shape);
}

function chainNodes(nodes: LayoutNode[], dir: SplitDir): LayoutNode {
  if (nodes.length === 1) return nodes[0];
  return {
    type: "split",
    id: newId("s"),
    dir,
    ratio: 1 / nodes.length,
    a: nodes[0],
    b: chainNodes(nodes.slice(1), dir),
  };
}

/** Pick a grid layout for `count` panels (1..MAX_PANELS), using templates up to 6 and rows above. */
export function layoutForCount(count: number, ids: () => string = () => newId("p")): LayoutNode {
  const clamped = Math.min(MAX_PANELS, Math.max(1, Math.round(count)));
  const preferred: Record<number, string> = {
    1: "full",
    2: "two_rows",
    3: "one_over_two",
    4: "grid_2x2",
    5: "two_one_two",
    6: "grid_2x3",
  };
  const id = preferred[clamped];
  const tpl = id ? LAYOUT_TEMPLATES.find((t) => t.id === id) : undefined;
  if (tpl) return buildTemplate(tpl, ids);
  const rows = clamped === 7 ? [1, 2, 2, 2] : clamped === 8 ? [2, 2, 2, 2] : [3, 3, 3];
  return chainNodes(
    rows.map((n) => chainNodes(Array.from({ length: n }, () => ({ type: "panel" as const, id: ids() })), "row")),
    "col",
  );
}

// ---------------------------------------------------------------------------
// Freeform layouts
// ---------------------------------------------------------------------------

export type LayoutMode = "grid" | "freeform";

export interface FreeformBox {
  id: string;
  x: number;
  y: number;
  w: number;
  h: number;
  z: number;
}

export const MIN_BOX_SIZE = 0.12;

export type ResizeHandle = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";

/** Convert a split tree into pixel-equivalent freeform boxes sorted in reading order. */
export function treeToFreeform(
  node: LayoutNode,
  page: { width: number; height: number },
  gutter: number,
  rightToLeft = false,
): FreeformBox[] {
  const rects = panelRects(node);
  const order = readingOrder(rects, rightToLeft);
  const pw = Math.max(1, page.width);
  const ph = Math.max(1, page.height);
  return order.map((id, i) => {
    const px = panelPixelRect(rects.get(id)!, { width: pw, height: ph }, gutter);
    return {
      id,
      x: px.x / pw,
      y: px.y / ph,
      w: px.w / pw,
      h: px.h / ph,
      z: i,
    };
  });
}

export function freeformRects(boxes: FreeformBox[]): Map<string, Rect> {
  return new Map(boxes.map((b) => [b.id, { x: b.x, y: b.y, w: b.w, h: b.h }]));
}

export function clampFreeformBox(box: FreeformBox): FreeformBox {
  const rawW = Number.isFinite(box.w) ? box.w : MIN_BOX_SIZE;
  const rawH = Number.isFinite(box.h) ? box.h : MIN_BOX_SIZE;
  const w = Math.min(1, Math.max(MIN_BOX_SIZE, rawW));
  const h = Math.min(1, Math.max(MIN_BOX_SIZE, rawH));
  const rawX = Number.isFinite(box.x) ? box.x : 0;
  const rawY = Number.isFinite(box.y) ? box.y : 0;
  const x = Math.min(1 - w, Math.max(0, rawX));
  const y = Math.min(1 - h, Math.max(0, rawY));
  return { ...box, x, y, w, h };
}

function snapAxisPos(
  pos: number,
  size: number,
  targets: number[],
  threshold: number,
): { pos: number; guide: number | null } {
  const offsets = [0, size / 2, size];
  let bestDist = threshold + 1e-12;
  let bestPos = pos;
  let bestGuide: number | null = null;
  for (const offset of offsets) {
    const edge = pos + offset;
    for (const target of targets) {
      const dist = Math.abs(target - edge);
      if (dist > bestDist) continue;
      const candidate = target - offset;
      if (candidate < -1e-9 || candidate + size > 1 + 1e-9) continue;
      bestDist = dist;
      bestPos = Math.min(1 - size, Math.max(0, candidate));
      bestGuide = target;
    }
  }
  return { pos: bestPos, guide: bestGuide };
}

function snapTargets(others: FreeformBox[], excludeId: string): { xs: number[]; ys: number[] } {
  const xs = [0, 0.5, 1];
  const ys = [0, 0.5, 1];
  for (const o of others) {
    if (o.id === excludeId) continue;
    xs.push(o.x, o.x + o.w / 2, o.x + o.w);
    ys.push(o.y, o.y + o.h / 2, o.y + o.h);
  }
  return { xs, ys };
}

export function snapFreeformBox(
  box: FreeformBox,
  others: FreeformBox[],
  threshold = 0.015,
): { box: FreeformBox; guides: Array<{ axis: "x" | "y"; pos: number }> } {
  const base = clampFreeformBox(box);
  const { xs, ys } = snapTargets(others, base.id);
  const sx = snapAxisPos(base.x, base.w, xs, threshold);
  const sy = snapAxisPos(base.y, base.h, ys, threshold);
  const guides: Array<{ axis: "x" | "y"; pos: number }> = [];
  if (sx.guide !== null) guides.push({ axis: "x", pos: sx.guide });
  if (sy.guide !== null) guides.push({ axis: "y", pos: sy.guide });
  return { box: clampFreeformBox({ ...base, x: sx.pos, y: sy.pos }), guides };
}

function snapEdge(
  val: number,
  min: number,
  max: number,
  targets: number[],
  snap: boolean,
  threshold: number,
): { val: number; guide: number | null } {
  const clamped = Math.min(max, Math.max(min, val));
  if (!snap) return { val: clamped, guide: null };
  let bestDist = threshold + 1e-12;
  let bestVal = clamped;
  let bestGuide: number | null = null;
  for (const t of targets) {
    if (t < min - 1e-9 || t > max + 1e-9) continue;
    const dist = Math.abs(t - clamped);
    if (dist <= bestDist) {
      bestDist = dist;
      bestVal = Math.min(max, Math.max(min, t));
      bestGuide = t;
    }
  }
  return { val: bestVal, guide: bestGuide };
}

export function resizeFreeformBox(
  orig: FreeformBox,
  handle: ResizeHandle,
  dx: number,
  dy: number,
  others: FreeformBox[],
  snap = true,
  threshold = 0.015,
): { box: FreeformBox; guides: Array<{ axis: "x" | "y"; pos: number }> } {
  let x0 = orig.x;
  let y0 = orig.y;
  let x1 = orig.x + orig.w;
  let y1 = orig.y + orig.h;
  const { xs, ys } = snapTargets(others, orig.id);
  const guides: Array<{ axis: "x" | "y"; pos: number }> = [];
  if (handle.includes("w")) {
    const res = snapEdge(orig.x + dx, 0, x1 - MIN_BOX_SIZE, xs, snap, threshold);
    x0 = res.val;
    if (res.guide !== null) guides.push({ axis: "x", pos: res.guide });
  }
  if (handle.includes("e")) {
    const res = snapEdge(orig.x + orig.w + dx, x0 + MIN_BOX_SIZE, 1, xs, snap, threshold);
    x1 = res.val;
    if (res.guide !== null) guides.push({ axis: "x", pos: res.guide });
  }
  if (handle.includes("n")) {
    const res = snapEdge(orig.y + dy, 0, y1 - MIN_BOX_SIZE, ys, snap, threshold);
    y0 = res.val;
    if (res.guide !== null) guides.push({ axis: "y", pos: res.guide });
  }
  if (handle.includes("s")) {
    const res = snapEdge(orig.y + orig.h + dy, y0 + MIN_BOX_SIZE, 1, ys, snap, threshold);
    y1 = res.val;
    if (res.guide !== null) guides.push({ axis: "y", pos: res.guide });
  }
  return { box: clampFreeformBox({ ...orig, x: x0, y: y0, w: x1 - x0, h: y1 - y0 }), guides };
}

export function reorderZ(
  boxes: FreeformBox[],
  id: string,
  action: "forward" | "backward" | "front" | "back",
): FreeformBox[] {
  const sorted = boxes
    .map((b, idx) => ({ b, idx }))
    .sort((a, b) => a.b.z - b.b.z || a.idx - b.idx)
    .map((entry) => entry.b);
  const at = sorted.findIndex((b) => b.id === id);
  if (at < 0) return boxes;
  const [item] = sorted.splice(at, 1);
  const target =
    action === "forward"
      ? Math.min(sorted.length, at + 1)
      : action === "backward"
        ? Math.max(0, at - 1)
        : action === "front"
          ? sorted.length
          : 0;
  sorted.splice(target, 0, item);
  return sorted.map((b, i) => ({ ...b, z: i }));
}

export function isFreeformBoxes(value: unknown): value is FreeformBox[] {
  if (!Array.isArray(value) || value.length > MAX_PANELS) return false;
  const seen = new Set<string>();
  for (const item of value) {
    if (!item || typeof item !== "object") return false;
    const b = item as Record<string, unknown>;
    if (typeof b.id !== "string" || b.id.length === 0 || seen.has(b.id)) return false;
    seen.add(b.id);
    if (
      typeof b.x !== "number" || !Number.isFinite(b.x) ||
      typeof b.y !== "number" || !Number.isFinite(b.y) ||
      typeof b.w !== "number" || !Number.isFinite(b.w) ||
      typeof b.h !== "number" || !Number.isFinite(b.h) ||
      typeof b.z !== "number" || !Number.isFinite(b.z)
    ) {
      return false;
    }
    if (b.w <= 0 || b.h <= 0 || b.x < -1e-6 || b.y < -1e-6 || b.x + b.w > 1 + 1e-6 || b.y + b.h > 1 + 1e-6) {
      return false;
    }
  }
  return true;
}

// ---------------------------------------------------------------------------
// Page geometry
// ---------------------------------------------------------------------------

export type PageShape = "portrait" | "square" | "landscape";

/** Page width over height. Portrait is the ISO paper ratio most manga pages use. */
export function pageAspect(shape: PageShape): number {
  return shape === "portrait" ? 1 / Math.SQRT2 : shape === "landscape" ? Math.SQRT2 : 1;
}

/** Output page size in pixels: the long edge is `longEdge`. */
export function pageSize(shape: PageShape, longEdge = 2048): { width: number; height: number } {
  const aspect = pageAspect(shape);
  return aspect >= 1
    ? { width: longEdge, height: Math.round(longEdge / aspect) }
    : { width: Math.round(longEdge * aspect), height: longEdge };
}

/**
 * Reading order: top to bottom, and within a band left to right or right to
 * left (manga). Two panels share a band when their tops are within 4% of the
 * page, so a tall side panel reads before the stacked ones beside it.
 */
export function readingOrder(rects: Map<string, Rect>, rightToLeft: boolean): string[] {
  const items = [...rects.entries()].map(([id, r]) => ({ id, r }));
  items.sort((p, q) => {
    if (Math.abs(p.r.y - q.r.y) > 0.04) return p.r.y - q.r.y;
    return rightToLeft ? q.r.x + q.r.w - (p.r.x + p.r.w) : p.r.x - q.r.x;
  });
  return items.map((i) => i.id);
}

/**
 * Size to generate a panel at: its on-page aspect (clamped to 1:3..3:1, the
 * composite crops the rest), the same pixel count as `budget`, multiples of 64.
 */
export function panelGenerationSize(
  rect: Rect,
  page: { width: number; height: number },
  budget: { width: number; height: number },
): { width: number; height: number } {
  const aspectRaw = (rect.w * page.width) / Math.max(1, rect.h * page.height);
  const aspect = Math.min(3, Math.max(1 / 3, aspectRaw));
  const pixels = Math.max(256 * 256, budget.width * budget.height);
  const snap = (v: number) => Math.max(256, Math.round(v / 64) * 64);
  return { width: snap(Math.sqrt(pixels * aspect)), height: snap(Math.sqrt(pixels / aspect)) };
}

/** A panel's box on the output page, inset for gutters, in whole pixels. */
export function panelPixelRect(rect: Rect, page: { width: number; height: number }, gutter: number): Rect {
  const half = gutter / 2;
  const x0 = rect.x * page.width + half + (rect.x <= 1e-6 ? half : 0);
  const y0 = rect.y * page.height + half + (rect.y <= 1e-6 ? half : 0);
  const x1 = (rect.x + rect.w) * page.width - half - (rect.x + rect.w >= 1 - 1e-6 ? half : 0);
  const y1 = (rect.y + rect.h) * page.height - half - (rect.y + rect.h >= 1 - 1e-6 ? half : 0);
  return {
    x: Math.round(x0),
    y: Math.round(y0),
    w: Math.max(1, Math.round(x1 - x0)),
    h: Math.max(1, Math.round(y1 - y0)),
  };
}

/** Source crop that fills `target` from an image of `src` size (object-fit: cover). */
export function coverCrop(src: { width: number; height: number }, target: { w: number; h: number }): Rect {
  const scale = Math.max(target.w / src.width, target.h / src.height);
  const w = target.w / scale;
  const h = target.h / scale;
  return { x: (src.width - w) / 2, y: (src.height - h) / 2, w, h };
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

export interface CastMember {
  id: string;
  name: string;
  /** Appearance tags: hair, eyes, outfit. Added to every panel they are in. */
  tags: string;
}

export interface ComicPanel {
  id: string;
  /** One line of story, for drafting the prompt and the story check. */
  beat: string;
  prompt: string;
  negative: string;
  enabled: boolean;
  /** Cast member ids in this panel. */
  cast: string[];
  /** Replaces the page setting for this panel when not blank (a flashback, a cut away). */
  setting: string;
  /**
   * What is said in the panel, one speech bubble per line, exactly as it should
   * be lettered. Drawn only on NovelAI, which letters a prompt's Text: block.
   */
  dialogue: string;
}

export function newPanel(id: string): ComicPanel {
  return { id, beat: "", prompt: "", negative: "", enabled: true, cast: [], setting: "", dialogue: "" };
}

/** True when a panel holds any user-authored beat, prompt, dialogue, setting, or cast assignment. */
export function isPanelNonEmpty(panel: ComicPanel): boolean {
  return (
    panel.beat.trim().length > 0 ||
    panel.prompt.trim().length > 0 ||
    panel.negative.trim().length > 0 ||
    panel.dialogue.trim().length > 0 ||
    panel.setting.trim().length > 0 ||
    panel.cast.length > 0
  );
}

/**
 * Re-map active + stashed overflow panels onto a template's reading-order ids.
 * Panels beyond the new layout's slot count that still hold content are kept in
 * overflow (with index-aligned results) so switching to a larger layout restores them.
 */
export function applyTemplateToPanels<T>(
  orderedPanels: ComicPanel[],
  orderedResults: Array<T | undefined>,
  overflowPanels: ComicPanel[],
  overflowResults: Array<T | undefined>,
  newIds: string[],
): {
  panels: Record<string, ComicPanel>;
  results: Record<string, T>;
  overflowPanels: ComicPanel[];
  overflowResults: Array<T | undefined>;
} {
  const pool = [...orderedPanels, ...overflowPanels];
  const resultPool: Array<T | undefined> = [...orderedResults, ...overflowResults];
  const panels: Record<string, ComicPanel> = {};
  const results: Record<string, T> = {};

  newIds.forEach((id, i) => {
    const old = pool[i];
    panels[id] = old ? { ...old, id } : newPanel(id);
    const res = resultPool[i];
    if (res !== undefined) results[id] = res;
  });

  const nextOverflowPanels: ComicPanel[] = [];
  const nextOverflowResults: Array<T | undefined> = [];
  for (let i = newIds.length; i < pool.length; i++) {
    const panel = pool[i];
    if (isPanelNonEmpty(panel)) {
      nextOverflowPanels.push(panel);
      nextOverflowResults.push(resultPool[i]);
    }
  }

  return {
    panels,
    results,
    overflowPanels: nextOverflowPanels,
    overflowResults: nextOverflowResults,
  };
}

/** A panel's speech bubbles: one per non-blank line. */
export function dialogueLines(text: string): string[] {
  return text
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);
}

/** The language tag NovelAI wants beside lettering, guessed from the script it is written in. */
export function letteringLanguage(lines: string[]): string {
  const all = lines.join("");
  if (/[\u3040-\u30ff]/.test(all)) return "japanese text";
  if (/[\uac00-\ud7af]/.test(all)) return "korean text";
  if (/[\u4e00-\u9fff]/.test(all)) return "chinese text";
  return "english text";
}

/**
 * Tags lettering needs that `prompt` does not have yet: a bubble, and the text
 * tag for its language. A prompt that already names a bubble or a text tag
 * (the V5 rewrite writes both) keeps its own.
 */
export function letteringTags(prompt: string, lines: string[]): string {
  if (lines.length === 0) return "";
  const tags = splitTags(prompt);
  const out: string[] = [];
  if (!tags.some((t) => t.endsWith("bubble"))) out.push("speech bubble");
  if (!tags.some((t) => /\btext$/.test(t) && t !== "text")) out.push(letteringLanguage(lines));
  return out.join(", ");
}

/** The Text: block NovelAI letters: the words only, a blank line between bubbles. Always last. */
export function textBlock(lines: string[]): string {
  return lines.length > 0 ? `\n\nText:\n${lines.join("\n\n")}` : "";
}

/**
 * Split a prompt into its tag line and the rest. A V5 prompt is a tag line, a
 * blank line, then prose; the cast and setting tags belong on the tag line, not
 * after the prose. A plain tag prompt is all tag line.
 */
export function splitTagLine(prompt: string): { tags: string; prose: string } {
  const text = prompt.trim();
  const gap = text.search(/\n\s*\n/);
  if (gap < 0) return { tags: text, prose: "" };
  return { tags: text.slice(0, gap).trim(), prose: text.slice(gap).trim() };
}

/**
 * Take a Text: block off a prompt, for a model that wrote the lettering into
 * the prompt instead of the dialogue field. Returns the prompt without it and
 * the bubbles it held (a blank line or a new line between them).
 */
export function takeTextBlock(prompt: string): { prompt: string; lines: string[] } {
  const m = /(^|\n)\s*Text:\s*(\n|$)/.exec(prompt);
  if (!m) return { prompt: prompt.trim(), lines: [] };
  return { prompt: prompt.slice(0, m.index).trim(), lines: dialogueLines(prompt.slice(m.index + m[0].length)) };
}

export function joinTags(...parts: string[]): string {
  return parts
    .map((p) => p.trim().replace(/^[,\s]+|[,\s]+$/g, ""))
    .filter(Boolean)
    .join(", ");
}

export function panelSetting(panel: ComicPanel, pageSetting: string): string {
  return panel.setting.trim() || pageSetting.trim();
}

/**
 * Positive prompt for one panel generated on its own. With `lettering` (NovelAI)
 * the panel's dialogue is drawn: its tags join the tag line and a Text: block
 * goes last, where NovelAI reads lettering from.
 */
export function panelPrompt(
  panel: ComicPanel,
  page: { style: string; setting: string; cast: CastMember[] },
  lettering = false,
): string {
  const cast = page.cast.filter((c) => panel.cast.includes(c.id));
  const { tags, prose } = splitTagLine(panel.prompt);
  const lines = lettering ? dialogueLines(panel.dialogue ?? "") : [];
  const line = joinTags(
    page.style,
    tags,
    ...cast.map((c) => c.tags),
    panelSetting(panel, page.setting),
    letteringTags(panel.prompt, lines),
  );
  const body = [line, prose].filter(Boolean).join("\n\n");
  return body ? body + textBlock(lines) : "";
}

/** Character prompt for one panel of a one-image NovelAI page. */
export function oneShotPanelPrompt(
  panel: ComicPanel,
  number: number,
  page: { setting: string; cast: CastMember[] },
): string {
  const cast = page.cast.filter((c) => panel.cast.includes(c.id));
  const own = panel.setting.trim();
  return joinTags(`panel ${number}`, panel.prompt, ...cast.map((c) => c.tags), own);
}

/**
 * Base prompt for a one-image NovelAI page. `dialogue` is every bubble on the
 * page in reading order: NovelAI reads lettering from the base prompt only.
 */
export function oneShotBasePrompt(page: { style: string; setting: string }, panels: number, dialogue: string[] = []): string {
  const base = joinTags(page.style, "comic", panels > 1 ? `${panels} panels, multiple views` : "", page.setting);
  return joinTags(base, letteringTags(base, dialogue)) + textBlock(dialogue);
}

/** Panel centre for NovelAI's character position, rounded to two places, kept off the edges. */
export function oneShotCentre(rect: Rect): { x: number; y: number } {
  const clamp = (v: number) => Math.min(0.9, Math.max(0.1, Math.round(v * 100) / 100));
  return { x: clamp(rect.x + rect.w / 2), y: clamp(rect.y + rect.h / 2) };
}

// ---------------------------------------------------------------------------
// Continuity
// ---------------------------------------------------------------------------

export function splitTags(text: string): string[] {
  return text
    .split(/[,\n]/)
    .map((t) =>
      t
        .trim()
        .replace(/^[([{]+|[)\]}]+$/g, "")
        .replace(/:\s*-?\d+(?:\.\d+)?$/, "")
        .replace(/_/g, " ")
        .toLowerCase()
        .trim(),
    )
    .filter(Boolean);
}

const COLOURS = [
  "black", "white", "grey", "gray", "silver", "blonde", "yellow", "orange", "red", "pink",
  "purple", "violet", "blue", "aqua", "light blue", "green", "brown", "light brown", "dark",
  "multicolored", "two-tone", "gradient", "streaked",
];
const COLOURED_PARTS = ["hair", "eyes", "skin"];

/** `brown hair` -> { colour: "brown", part: "hair" }, or null for a non-colour tag. */
export function colourAttribute(tag: string): { colour: string; part: string } | null {
  for (const part of COLOURED_PARTS) {
    if (!tag.endsWith(` ${part}`)) continue;
    const colour = tag.slice(0, -part.length - 1).trim();
    if (COLOURS.includes(colour)) return { colour, part };
  }
  return null;
}

export interface ContinuityIssue {
  kind: "missing" | "changed";
  /** What the panel should show. */
  expected: string;
  /** What the tagger saw instead, for "changed". */
  got?: string;
  /** Cast member name, or null for the setting. */
  who: string | null;
}

/**
 * Compare what a panel should show with what the tagger saw.
 *
 * `expected` lists checkable tags per cast member (and `who: null` for the
 * setting). A colour attribute (`brown hair`) that came out as another colour
 * of the same part is "changed"; any other expected tag the tagger did not see
 * is "missing". Callers pass only tags the tagger can know.
 */
export function continuityIssues(
  expected: Array<{ who: string | null; tags: string[] }>,
  detected: string[],
): ContinuityIssue[] {
  const seen = new Set(detected.map((t) => t.replace(/_/g, " ").toLowerCase().trim()));
  const issues: ContinuityIssue[] = [];
  for (const group of expected) {
    for (const tag of group.tags) {
      if (seen.has(tag)) continue;
      const attr = colourAttribute(tag);
      if (attr) {
        const other = [...seen].find((t) => {
          const a = colourAttribute(t);
          return a !== null && a.part === attr.part && a.colour !== attr.colour;
        });
        if (other) {
          issues.push({ kind: "changed", expected: tag, got: other, who: group.who });
          continue;
        }
      }
      issues.push({ kind: "missing", expected: tag, who: group.who });
    }
  }
  return issues;
}

// ---------------------------------------------------------------------------
// Story check and drafting (Prompt Assistant)
// ---------------------------------------------------------------------------

export const STORY_CHECK_SYSTEM = [
  "You check one comic page for continuity before it is drawn.",
  "You get the page setting, the cast with their looks, and each panel in reading order with its story beat, prompt, dialogue, cast and setting.",
  "Find contradictions between panels: an object that appears or disappears without reason, a changed outfit or hairstyle, a time of day or place that jumps, a character present who was not in the scene, an action that does not follow from the panel before, a line of dialogue that answers something nobody said.",
  "Do not comment on style, quality or word choice. If nothing is wrong, return an empty list.",
  'Reply with JSON only: {"issues":[{"panel":<number>,"issue":"<one short sentence>"}]}',
].join("\n");

export const DRAFT_SYSTEM = [
  "You write image prompts for the panels of one comic page.",
  "You get the page style, setting, cast and each panel's story beat in reading order.",
  "For each panel write a comma separated tag prompt (Danbooru style): framing and camera (close-up, cowboy shot, from above), who is in it by count (1girl, 1boy), pose, action, expression, important objects, background.",
  "Do not repeat the cast's appearance tags or the page setting; they are added for you. Do not write dialogue.",
  'Reply with JSON only: {"panels":[{"panel":<number>,"prompt":"<tags>"}]}',
].join("\n");

/**
 * The user turn for rewriting one panel: the whole page for context, then
 * which panel to rewrite. The answer has the same shape as a full draft.
 */
export function enhancePanelInstruction(number: number): string {
  return `Rewrite panel ${number} only. Build on its beat and its current prompt, keep everything they say, and fill in framing, pose, expression and background where they are thin. Answer with that one panel in the same JSON shape.`;
}

export function storyBrief(
  ordered: ComicPanel[],
  page: { style: string; setting: string; cast: CastMember[] },
): string {
  const cast = page.cast.map((c) => `- ${c.name || "?"}: ${c.tags || "(no tags)"}`).join("\n");
  const panels = ordered
    .map((p, i) => {
      const who = page.cast.filter((c) => p.cast.includes(c.id)).map((c) => c.name).join(", ");
      return [
        `Panel ${i + 1}:`,
        p.beat.trim() ? `  beat: ${p.beat.trim()}` : "",
        p.prompt.trim() ? `  prompt: ${p.prompt.trim()}` : "",
        ...dialogueLines(p.dialogue ?? "").map((l) => `  says: "${l}"`),
        who ? `  cast: ${who}` : "",
        p.setting.trim() ? `  setting: ${p.setting.trim()}` : "",
      ]
        .filter(Boolean)
        .join("\n");
    })
    .join("\n");
  return [
    page.style.trim() ? `Style: ${page.style.trim()}` : "",
    `Setting: ${page.setting.trim() || "(not set)"}`,
    `Cast:\n${cast || "(none)"}`,
    panels,
  ]
    .filter(Boolean)
    .join("\n\n");
}

/** The first JSON object in a model reply, tolerating code fences and chatter. */
export function parseJsonReply(text: string): unknown {
  const start = text.indexOf("{");
  const end = text.lastIndexOf("}");
  if (start < 0 || end <= start) return null;
  try {
    return JSON.parse(text.slice(start, end + 1));
  } catch {
    return null;
  }
}

export function parseStoryIssues(text: string, panelCount: number): Array<{ panel: number; issue: string }> {
  const data = parseJsonReply(text) as { issues?: unknown } | null;
  if (!data || !Array.isArray(data.issues)) return [];
  return data.issues
    .map((i) => i as { panel?: unknown; issue?: unknown })
    .filter((i) => typeof i.issue === "string" && i.issue.trim() !== "")
    .map((i) => ({
      panel: Number.isInteger(i.panel) && (i.panel as number) >= 1 && (i.panel as number) <= panelCount ? (i.panel as number) : 0,
      issue: (i.issue as string).trim(),
    }));
}

export interface PanelDraft {
  prompt: string;
  /** The panel's bubbles, or null when the reply left the dialogue alone. */
  dialogue: string[] | null;
}

export function parseDraftPrompts(text: string, panelCount: number): Map<number, PanelDraft> {
  const out = new Map<number, PanelDraft>();
  const data = parseJsonReply(text) as { panels?: unknown } | null;
  if (!data || !Array.isArray(data.panels)) return out;
  for (const raw of data.panels) {
    const p = raw as { panel?: unknown; prompt?: unknown; dialogue?: unknown };
    if (!Number.isInteger(p.panel) || typeof p.prompt !== "string") continue;
    const n = p.panel as number;
    // A Text: block written into the prompt is lettering, not prompt.
    const { prompt, lines } = takeTextBlock(p.prompt);
    if (n < 1 || n > panelCount || !prompt) continue;
    let dialogue: string[] | null = lines.length > 0 ? lines : null;
    if (Array.isArray(p.dialogue)) {
      dialogue = p.dialogue
        .filter((l): l is string => typeof l === "string")
        .flatMap((l) => dialogueLines(l.replace(/^\s*"|"\s*$/g, "")));
    } else if (typeof p.dialogue === "string") {
      dialogue = dialogueLines(p.dialogue);
    }
    out.set(n, { prompt, dialogue });
  }
  return out;
}
