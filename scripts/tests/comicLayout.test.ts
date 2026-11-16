import { test } from "node:test";
import assert from "node:assert/strict";
import {
  LAYOUT_TEMPLATES,
  buildTemplate,
  colourAttribute,
  continuityIssues,
  coverCrop,
  dividers,
  isLayout,
  oneShotBasePrompt,
  oneShotCentre,
  oneShotPanelPrompt,
  pageSize,
  panelGenerationSize,
  panelIds,
  panelPixelRect,
  panelPrompt,
  panelRects,
  parseDraftPrompts,
  parseStoryIssues,
  ratioFromPointer,
  readingOrder,
  removePanel,
  setRatio,
  splitPanel,
  newPanel,
  type LayoutNode,
} from "../../src/lib/utils/comicLayout.ts";

const counter = () => {
  let n = 0;
  return () => `p${++n}`;
};

test("every template builds the number of panels it says, covering the page", () => {
  for (const t of LAYOUT_TEMPLATES) {
    const layout = buildTemplate(t, counter());
    assert.equal(panelIds(layout).length, t.panels, t.id);
    const area = [...panelRects(layout).values()].reduce((sum, r) => sum + r.w * r.h, 0);
    assert.ok(Math.abs(area - 1) < 1e-9, t.id);
    assert.ok(isLayout(JSON.parse(JSON.stringify(layout))), t.id);
  }
  assert.equal(new Set(LAYOUT_TEMPLATES.map((t) => t.id)).size, LAYOUT_TEMPLATES.length);
});

test("split, resize and remove edit the tree", () => {
  let layout: LayoutNode = { type: "panel", id: "a" };
  layout = splitPanel(layout, "a", "col", "b");
  layout = splitPanel(layout, "b", "row", "c");
  assert.deepEqual(panelIds(layout), ["a", "b", "c"]);
  const rects = panelRects(layout);
  assert.deepEqual(rects.get("c"), { x: 0.5, y: 0.5, w: 0.5, h: 0.5 });

  const [top] = dividers(layout);
  assert.equal(top.dir, "col");
  layout = setRatio(layout, top.splitId, ratioFromPointer(top, 0.3));
  assert.ok(Math.abs(panelRects(layout).get("a")!.h - 0.3) < 1e-9);
  layout = setRatio(layout, top.splitId, 0.01);
  assert.ok(panelRects(layout).get("a")!.h >= 0.12 - 1e-9);

  layout = removePanel(layout, "b");
  assert.deepEqual(panelIds(layout), ["a", "c"]);
  assert.deepEqual(removePanel({ type: "panel", id: "x" }, "x"), { type: "panel", id: "x" });
  assert.equal(isLayout({ type: "split", id: "s", dir: "row", ratio: 0.5, a: { type: "panel", id: "a" } }), false);
});

test("reading order runs by band, left to right or right to left", () => {
  const layout = buildTemplate(LAYOUT_TEMPLATES.find((t) => t.id === "two_over_one")!, counter());
  const rects = panelRects(layout);
  assert.deepEqual(readingOrder(rects, false), ["p1", "p2", "p3"]);
  assert.deepEqual(readingOrder(rects, true), ["p2", "p1", "p3"]);
  const tall = buildTemplate(LAYOUT_TEMPLATES.find((t) => t.id === "tall_left_two_right")!, counter());
  assert.deepEqual(readingOrder(panelRects(tall), false), ["p1", "p2", "p3"]);
});

test("panel sizes keep the pixel budget, snap to 64 and clamp the aspect", () => {
  const page = pageSize("portrait");
  assert.deepEqual(page, { width: 1448, height: 2048 });
  const wide = panelGenerationSize({ x: 0, y: 0, w: 1, h: 0.25 }, page, { width: 1024, height: 1024 });
  assert.equal(wide.width % 64, 0);
  assert.equal(wide.height % 64, 0);
  assert.ok(wide.width > wide.height);
  assert.ok(Math.abs(wide.width * wide.height - 1024 * 1024) / (1024 * 1024) < 0.15);
  const sliver = panelGenerationSize({ x: 0, y: 0, w: 1, h: 0.02 }, page, { width: 1024, height: 1024 });
  assert.ok(sliver.width / sliver.height <= 3.2);
});

test("gutters inset panels, doubling at the page edge", () => {
  const page = { width: 1000, height: 1000 };
  assert.deepEqual(panelPixelRect({ x: 0, y: 0, w: 0.5, h: 1 }, page, 20), { x: 20, y: 20, w: 470, h: 960 });
  assert.deepEqual(panelPixelRect({ x: 0.5, y: 0, w: 0.5, h: 1 }, page, 20), { x: 510, y: 20, w: 470, h: 960 });
  assert.deepEqual(coverCrop({ width: 200, height: 100 }, { w: 100, h: 100 }), { x: 50, y: 0, w: 100, h: 100 });
});

test("prompts carry style, cast looks and the setting, with a panel override", () => {
  const cast = [
    { id: "c1", name: "Aki", tags: "1girl, long black hair, sailor uniform" },
    { id: "c2", name: "Ren", tags: "1boy, short brown hair, gakuran" },
  ];
  const panel = { ...newPanel("p1"), prompt: "close-up, smile", cast: ["c1"] };
  const page = { style: "manga, monochrome", setting: "classroom, sunset", cast };
  assert.equal(panelPrompt(panel, page), "manga, monochrome, close-up, smile, 1girl, long black hair, sailor uniform, classroom, sunset");
  assert.equal(panelPrompt({ ...panel, setting: "night, rooftop" }, page).endsWith("night, rooftop"), true);
  assert.equal(oneShotPanelPrompt(panel, 2, page), "panel 2, close-up, smile, 1girl, long black hair, sailor uniform");
  assert.equal(oneShotBasePrompt(page, 4), "manga, monochrome, comic, 4 panels, multiple views, classroom, sunset");
  assert.deepEqual(oneShotCentre({ x: 0.66, y: 0, w: 0.34, h: 0.34 }), { x: 0.83, y: 0.17 });
  assert.deepEqual(oneShotCentre({ x: 0, y: 0.95, w: 0.05, h: 0.05 }), { x: 0.1, y: 0.9 });
});

test("continuity flags changed colours and missing tags", () => {
  assert.deepEqual(colourAttribute("brown hair"), { colour: "brown", part: "hair" });
  assert.equal(colourAttribute("hair ribbon"), null);
  const issues = continuityIssues(
    [
      { who: "Ren", tags: ["brown hair", "gakuran", "green eyes"] },
      { who: null, tags: ["classroom"] },
    ],
    ["1boy", "blonde_hair", "gakuran", "outdoors"],
  );
  assert.deepEqual(issues, [
    { kind: "changed", expected: "brown hair", got: "blonde hair", who: "Ren" },
    { kind: "missing", expected: "green eyes", who: "Ren" },
    { kind: "missing", expected: "classroom", who: null },
  ]);
});

test("story and draft replies parse from chatty JSON", () => {
  const issues = parseStoryIssues('Sure!\n```json\n{"issues":[{"panel":2,"issue":"The letter vanishes."},{"panel":9,"issue":"x"},{"issue":""}]}\n```', 3);
  assert.deepEqual(issues, [
    { panel: 2, issue: "The letter vanishes." },
    { panel: 0, issue: "x" },
  ]);
  assert.deepEqual(parseStoryIssues("no json here", 3), []);
  const drafts = parseDraftPrompts('{"panels":[{"panel":1,"prompt":"close-up, hand"},{"panel":4,"prompt":"x"}]}', 3);
  assert.deepEqual([...drafts.entries()], [[1, "close-up, hand"]]);
});
