import assert from "node:assert/strict";
import test from "node:test";
import {
  NEUTRAL_ADJUSTMENTS,
  applyAdjustments,
  buildEditNote,
  colorMatrix,
  dragCropRect,
  editedMetadata,
  fitAspectRect,
  gaussianBlur,
  isNeutral,
  rectFromAnchor,
  roundRect,
} from "../../src/lib/utils/imageEditorOps.ts";

const adjust = (patch: Partial<typeof NEUTRAL_ADJUSTMENTS>) => ({ ...NEUTRAL_ADJUSTMENTS, ...patch });
const pixels = (...rgba: number[]) => new Uint8ClampedArray(rgba);

test("neutral adjustments leave pixels untouched", () => {
  const data = pixels(10, 120, 250, 255, 0, 0, 0, 128);
  const before = Array.from(data);
  applyAdjustments(data, 2, 1, adjust({}), 1);
  assert.deepEqual(Array.from(data), before);
  assert.ok(isNeutral(adjust({})));
  assert.ok(!isNeutral(adjust({ hue: 1 })));
});

test("brightness shifts, contrast pivots on mid grey, alpha is never touched", () => {
  const bright = pixels(100, 100, 100, 77);
  applyAdjustments(bright, 1, 1, adjust({ brightness: 40 }), 1);
  assert.deepEqual(Array.from(bright), [151, 151, 151, 77]);

  const contrast = pixels(128, 128, 128, 255, 28, 228, 128, 255);
  applyAdjustments(contrast, 2, 1, adjust({ contrast: -100 }), 1);
  assert.deepEqual(Array.from(contrast), [128, 128, 128, 255, 128, 128, 128, 255]);
});

test("saturation -100 is greyscale and keeps greys grey", () => {
  const data = pixels(255, 0, 0, 255, 90, 90, 90, 255);
  applyAdjustments(data, 2, 1, adjust({ saturation: -100 }), 1);
  assert.equal(data[0], data[1]);
  assert.equal(data[1], data[2]);
  assert.deepEqual(Array.from(data.slice(4, 7)), [90, 90, 90]);
});

test("hue rotation keeps greys and a full turn is the identity", () => {
  const grey = pixels(77, 77, 77, 255);
  applyAdjustments(grey, 1, 1, adjust({ hue: 120 }), 1);
  assert.deepEqual(Array.from(grey), [77, 77, 77, 255]);
  const m = colorMatrix(adjust({ hue: 180 }));
  const back = colorMatrix(adjust({ hue: -180 }));
  m.forEach((v, i) => assert.ok(Math.abs(v - back[i]) < 1e-9));
});

test("temperature warms: more red, less blue", () => {
  const data = pixels(100, 100, 100, 255);
  applyAdjustments(data, 1, 1, adjust({ temperature: 40 }), 1);
  assert.ok(data[0] > 100 && data[2] < 100 && data[1] === 100);
});

test("blur keeps a flat image flat and spreads a single bright pixel", () => {
  const flat = new Uint8ClampedArray(9 * 9 * 4).fill(60);
  gaussianBlur(flat, 9, 9, 3);
  assert.ok(flat.every((v) => v === 60));

  const dot = new Uint8ClampedArray(9 * 9 * 4);
  dot[(4 * 9 + 4) * 4] = 255;
  gaussianBlur(dot, 9, 9, 1.5);
  assert.ok(dot[(4 * 9 + 4) * 4] < 255);
  assert.ok(dot[(4 * 9 + 5) * 4] > 0);
});

test("a free crop drag stays inside the image", () => {
  const start = { x: 10, y: 10, w: 50, h: 40 };
  assert.deepEqual(dragCropRect(start, "se", 500, 500, 100, 80, null), { x: 10, y: 10, w: 90, h: 70 });
  assert.deepEqual(dragCropRect(start, "move", -500, 500, 100, 80, null), { x: 0, y: 40, w: 50, h: 40 });
  assert.deepEqual(dragCropRect(start, "w", -500, 0, 100, 80, null), { x: 0, y: 10, w: 60, h: 40 });
});

test("a ratio-locked crop keeps its ratio at the image edge", () => {
  const start = { x: 0, y: 0, w: 40, h: 40 };
  const dragged = dragCropRect(start, "se", 500, 500, 100, 60, 1);
  assert.deepEqual(dragged, { x: 0, y: 0, w: 60, h: 60 });
  const edge = dragCropRect({ x: 10, y: 20, w: 20, h: 10 }, "e", 500, 0, 100, 60, 2);
  assert.ok(Math.abs(edge.w / edge.h - 2) < 1e-9);
  assert.ok(edge.y >= 0 && edge.y + edge.h <= 60 && edge.x + edge.w <= 100);
});

test("dragging past the anchor flips the rectangle instead of going negative", () => {
  const rect = rectFromAnchor(50, 50, 20, 30, 100, 100, null);
  assert.deepEqual(rect, { x: 20, y: 30, w: 30, h: 20 });
});

test("aspect presets centre the largest fitting rectangle", () => {
  assert.deepEqual(fitAspectRect(200, 100, 1), { x: 50, y: 0, w: 100, h: 100 });
  assert.deepEqual(fitAspectRect(100, 200, 1), { x: 0, y: 50, w: 100, h: 100 });
  assert.deepEqual(fitAspectRect(100, 200, null), { x: 0, y: 0, w: 100, h: 200 });
  assert.deepEqual(roundRect({ x: -3, y: 99.6, w: 500, h: 0.2 }, 100, 100), { x: 0, y: 99, w: 100, h: 1 });
});

test("the edit note lists pixel ops, then only the sliders that moved", () => {
  const note = buildEditNote(
    [
      { op: "crop", w: 896, h: 1152 },
      { op: "rotate", deg: 90 },
    ],
    adjust({ saturation: 12 }),
  );
  assert.deepEqual(JSON.parse(note), [
    { op: "crop", w: 896, h: 1152 },
    { op: "rotate", deg: 90 },
    { op: "adjust", saturation: 12 },
  ]);
  assert.equal(buildEditNote([], adjust({})), "[]");
});

test("edited metadata keeps the recipe and adds the note", () => {
  const source = { positive_prompt: "1girl", seed: "42", size: "1024x1024" };
  const out = editedMetadata(source, [{ op: "flip", axis: "h" }], adjust({}), { width: 512, height: 768 }, "a.png", "2026-11-13T00:00:00.000Z");
  assert.ok(out);
  assert.equal(out.positive_prompt, "1girl");
  assert.equal(out.size, "1024x1024");
  assert.equal(out.mooshie_edited_from, "a.png");
  assert.equal(out.mooshie_edit_size, "512x768");
  assert.deepEqual(JSON.parse(out.mooshie_edits), [{ op: "flip", axis: "h" }]);
  assert.deepEqual(source, { positive_prompt: "1girl", seed: "42", size: "1024x1024" });
});

test("editing an edit appends to its note and keeps the first original", () => {
  const first = editedMetadata({ seed: "1" }, [{ op: "flip", axis: "v" }], adjust({}), { width: 8, height: 8 }, "orig.png", "d1");
  const second = editedMetadata(first, [{ op: "rotate", deg: -90 }], adjust({}), { width: 8, height: 8 }, "edit_orig.png", "d2");
  assert.ok(second);
  assert.equal(second.mooshie_edited_from, "orig.png");
  assert.deepEqual(JSON.parse(second.mooshie_edits), [
    { op: "flip", axis: "v" },
    { op: "rotate", deg: -90 },
  ]);
});

test("an image with no metadata gets none invented", () => {
  assert.equal(editedMetadata(null, [{ op: "flip", axis: "h" }], adjust({}), { width: 1, height: 1 }, null, "d"), null);
  assert.equal(editedMetadata({}, [], adjust({}), { width: 1, height: 1 }, null, "d"), null);
});
