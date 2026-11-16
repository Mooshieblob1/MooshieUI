// Comic page builder: the pure parts. Layouts, templates, reading order,
// panel sizes, prompt assembly and the continuity comparison. No stores and no
// DOM, so all of it is covered by scripts/tests/comicLayout.test.ts.
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
}

export function newPanel(id: string): ComicPanel {
  return { id, beat: "", prompt: "", negative: "", enabled: true, cast: [], setting: "" };
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

/** Positive prompt for one panel generated on its own. */
export function panelPrompt(panel: ComicPanel, page: { style: string; setting: string; cast: CastMember[] }): string {
  const cast = page.cast.filter((c) => panel.cast.includes(c.id));
  return joinTags(page.style, panel.prompt, ...cast.map((c) => c.tags), panelSetting(panel, page.setting));
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

/** Base prompt for a one-image NovelAI page. */
export function oneShotBasePrompt(page: { style: string; setting: string }, panels: number): string {
  return joinTags(page.style, "comic", panels > 1 ? `${panels} panels, multiple views` : "", page.setting);
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
  "You get the page setting, the cast with their looks, and each panel in reading order with its story beat, prompt, cast and setting.",
  "Find contradictions between panels: an object that appears or disappears without reason, a changed outfit or hairstyle, a time of day or place that jumps, a character present who was not in the scene, an action that does not follow from the panel before.",
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

export function parseDraftPrompts(text: string, panelCount: number): Map<number, string> {
  const out = new Map<number, string>();
  const data = parseJsonReply(text) as { panels?: unknown } | null;
  if (!data || !Array.isArray(data.panels)) return out;
  for (const raw of data.panels) {
    const p = raw as { panel?: unknown; prompt?: unknown };
    if (!Number.isInteger(p.panel) || typeof p.prompt !== "string") continue;
    const n = p.panel as number;
    if (n >= 1 && n <= panelCount && p.prompt.trim()) out.set(n, p.prompt.trim());
  }
  return out;
}
