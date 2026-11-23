import assert from "node:assert/strict";
import test from "node:test";
import {
  NEUTRAL_ADJUSTMENTS,
  applyAdjustments,
  applyTone,
  appendOp,
  strokeStamps,
  curveLut,
  normalizeAdjustments,
  sameAdjustments,
  buildEditNote,
  colorMatrix,
  alphaBounds,
  carriesNovelAiChunks,
  copyPngTextChunks,
  dragCropRect,
  scaleRect,
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

test("an identity curve maps every level to itself and points are hit exactly", () => {
  const identity = curveLut([[0, 0], [255, 255]]);
  assert.ok(Array.from(identity).every((v, i) => v === i));
  const lut = curveLut([[0, 0], [64, 40], [192, 220], [255, 255]]);
  assert.equal(lut[64], 40);
  assert.equal(lut[192], 220);
  // Monotone input stays monotone: an S-curve never doubles back.
  for (let i = 1; i < 256; i++) assert.ok(lut[i] >= lut[i - 1]);
});

test("a curve holds its end values outside the outer points", () => {
  const lut = curveLut([[50, 20], [200, 240]]);
  assert.equal(lut[0], 20);
  assert.equal(lut[255], 240);
});

test("shadows and highlights leave pure black and white alone", () => {
  const data = pixels(0, 0, 0, 255, 255, 255, 255, 255, 80, 80, 80, 255, 180, 180, 180, 255);
  applyTone(data, 4, 1, adjust({ shadows: 100, highlights: -100 }));
  assert.deepEqual(Array.from(data.slice(0, 8)), [0, 0, 0, 255, 255, 255, 255, 255]);
  assert.ok(data[8] > 80, "shadows lifted");
  assert.ok(data[12] < 180, "highlights pulled down");
});

test("vibrance boosts muted colours more than saturated ones and keeps greys", () => {
  const data = pixels(120, 110, 100, 255, 250, 20, 20, 255, 90, 90, 90, 255);
  applyTone(data, 3, 1, adjust({ vibrance: 100 }));
  const mutedGain = (data[0] - data[2]) / 20;
  const vividGain = (data[4] - data[5]) / 230;
  assert.ok(mutedGain > vividGain);
  assert.deepEqual(Array.from(data.slice(8, 11)), [90, 90, 90]);
});

test("vignette darkens the corners and leaves the centre", () => {
  const w = 21;
  const data = new Uint8ClampedArray(w * w * 4).fill(200);
  applyTone(data, w, w, adjust({ vignette: -100 }));
  assert.equal(data[(10 * w + 10) * 4], 200);
  assert.ok(data[0] < 60);
  const light = new Uint8ClampedArray(w * w * 4).fill(100);
  applyTone(light, w, w, adjust({ vignette: 100 }));
  assert.ok(light[0] > 200);
});

test("a curve counts as an adjustment and is written into the edit note", () => {
  const curved = adjust({ curve: [[0, 0], [128, 160], [255, 255]] });
  assert.ok(!isNeutral(curved));
  assert.ok(!sameAdjustments(curved, adjust({})));
  assert.deepEqual(JSON.parse(buildEditNote([], curved)), [{ op: "adjust", curve: [[0, 0], [128, 160], [255, 255]] }]);
});

test("stored presets are cleaned up before use", () => {
  const preset = normalizeAdjustments({
    brightness: 500,
    hue: -999,
    blur: -5,
    bogus: 1,
    contrast: "12",
    curve: [[300, -4], [0, 10], "x", [0, 99]],
  });
  assert.equal(preset.brightness, 100);
  assert.equal(preset.hue, -180);
  assert.equal(preset.blur, 0);
  assert.equal(preset.contrast, 0);
  assert.deepEqual(preset.curve, [[0, 10], [255, 0]]);
  assert.ok(!("bogus" in preset));
  assert.ok(isNeutral(normalizeAdjustments(null)));
  assert.ok(isNeutral(normalizeAdjustments({ curve: [[5, 5]] })));
});

test("repeated paint strokes fold into one counted entry", () => {
  let ops = appendOp([], { op: "brush", n: 1 });
  ops = appendOp(ops, { op: "brush", n: 1 });
  ops = appendOp(ops, { op: "eraser", n: 1 });
  ops = appendOp(ops, { op: "crop", w: 4, h: 4 });
  ops = appendOp(ops, { op: "brush", n: 1 });
  assert.deepEqual(ops, [
    { op: "brush", n: 2 },
    { op: "eraser", n: 1 },
    { op: "crop", w: 4, h: 4 },
    { op: "brush", n: 1 },
  ]);
});

test("brush stamps keep even spacing across pointer segments", () => {
  const first = strokeStamps(0, 0, 10, 0, 4, 0);
  assert.deepEqual(first.points, [[4, 0], [8, 0]]);
  assert.equal(first.carry, 2);
  const second = strokeStamps(10, 0, 20, 0, 4, first.carry);
  assert.deepEqual(second.points, [[12, 0], [16, 0], [20, 0]]);
  assert.equal(second.carry, 0);
  assert.deepEqual(strokeStamps(5, 5, 5, 5, 4, 1), { points: [], carry: 1 });
});

test("scaling a layer keeps the opposite side fixed and its proportions", () => {
  const start = { x: 10, y: 20, w: 100, h: 50 };
  assert.deepEqual(scaleRect(start, "move", 7, -3, true), { x: 17, y: 17, w: 100, h: 50 });
  assert.deepEqual(scaleRect(start, "se", 100, 0, true), { x: 10, y: 20, w: 200, h: 100 });
  assert.deepEqual(scaleRect(start, "nw", -100, 0, true), { x: -90, y: -30, w: 200, h: 100 });
  // An edge handle grows the other axis around the centre.
  assert.deepEqual(scaleRect(start, "e", 100, 0, true), { x: 10, y: -5, w: 200, h: 100 });
  assert.deepEqual(scaleRect(start, "e", 100, 0, false), { x: 10, y: 20, w: 200, h: 50 });
  assert.deepEqual(scaleRect(start, "se", 30, 10, false), { x: 10, y: 20, w: 130, h: 60 });
});

test("scaling never flips or collapses a layer", () => {
  const start = { x: 10, y: 20, w: 100, h: 50 };
  const free = scaleRect(start, "se", -500, -500, false);
  assert.deepEqual(free, { x: 10, y: 20, w: 1, h: 1 });
  const west = scaleRect(start, "w", 500, 0, false);
  assert.deepEqual(west, { x: 109, y: 20, w: 1, h: 50 });
});

test("alpha bounds box the opaque pixels and report an empty layer", () => {
  const data = new Uint8ClampedArray(6 * 4 * 4);
  assert.equal(alphaBounds(data, 6, 4), null);
  data[(1 * 6 + 2) * 4 + 3] = 255;
  data[(3 * 6 + 4) * 4 + 3] = 1;
  assert.deepEqual(alphaBounds(data, 6, 4), { x: 2, y: 1, w: 3, h: 3 });
});

/** A structurally valid PNG (CRCs are not checked by the splice). */
function fakePng(chunks: [string, number[]][]): Uint8Array {
  const out = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
  for (const [kind, data] of chunks) {
    const n = data.length;
    out.push((n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255);
    out.push(...[...kind].map((c) => c.charCodeAt(0)), ...data, 1, 2, 3, 4);
  }
  return new Uint8Array(out);
}
const textChunk = (key: string, value: string): [string, number[]] => [
  "tEXt",
  [...[...key].map((c) => c.charCodeAt(0)), 0, ...new TextEncoder().encode(value)],
];

test("NovelAI text chunks are recognised and carried onto an edited PNG byte for byte", () => {
  const source = fakePng([
    ["IHDR", [1, 2, 3]],
    textChunk("Software", "NovelAI"),
    textChunk("Comment", '{"prompt":"青い髪"}'),
    ["IDAT", [9, 9]],
    ["IEND", []],
  ]);
  const edited = fakePng([["IHDR", [4, 5, 6]], ["sRGB", [0]], ["IDAT", [7, 7, 7]], ["IEND", []]]);
  assert.equal(carriesNovelAiChunks(source), true);
  assert.equal(carriesNovelAiChunks(edited), false);

  const spliced = copyPngTextChunks(source, edited)!;
  assert.equal(carriesNovelAiChunks(spliced), true);
  // The text sits right before IDAT, unchanged, and nothing else moved.
  const idat = fakePng([["IDAT", [7, 7, 7]], ["IEND", []]]).subarray(8);
  const text = source.subarray(8 + 15, source.length - 14 - 12);
  assert.deepEqual([...spliced.subarray(spliced.length - idat.length)], [...idat]);
  assert.deepEqual([...spliced.subarray(spliced.length - idat.length - text.length, spliced.length - idat.length)], [...text]);
  assert.equal(spliced.length, edited.length + text.length);
});

test("other PNGs and broken bytes are left alone", () => {
  const plain = fakePng([["IHDR", [1]], textChunk("Software", "AUTOMATIC1111"), ["IDAT", [1]], ["IEND", []]]);
  const bare = fakePng([["IHDR", [1]], ["IDAT", [1]], ["IEND", []]]);
  assert.equal(carriesNovelAiChunks(plain), false);
  assert.equal(carriesNovelAiChunks(new Uint8Array([1, 2, 3])), false);
  assert.equal(copyPngTextChunks(bare, plain), null);
  assert.equal(copyPngTextChunks(plain, new Uint8Array([1, 2, 3])), null);
  assert.equal(copyPngTextChunks(plain.subarray(0, plain.length - 5), bare), null);
  assert.equal(carriesNovelAiChunks(fakePng([["IHDR", [1]], textChunk("Source", "NovelAI Diffusion V4.5 4BDE2A90"), ["IDAT", [1]], ["IEND", []]])), true);
});
