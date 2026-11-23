import assert from "node:assert/strict";
import test from "node:test";
import { buildGalleryRows, rowOffsets, selectionRange, visibleRowRange } from "../../src/lib/utils/galleryGrid.ts";

const range = (n: number) => Array.from({ length: n }, (_, i) => i);

test("rows hold at most `columns` tiles and keep every image once, in order", () => {
  const rows = buildGalleryRows([{ label: "all", images: range(11) }], 4, false);
  assert.deepEqual(rows.map((r) => r.images.length), [4, 4, 3]);
  assert.deepEqual(rows.flatMap((r) => r.images), range(11));
  assert.ok(rows.every((r) => r.kind === "tiles"));
});

test("grouped galleries get a header row per non-empty group", () => {
  const rows = buildGalleryRows(
    [
      { label: "a", images: [1, 2, 3] },
      { label: "empty", images: [] },
      { label: "b", images: [4] },
    ],
    2,
    true,
  );
  assert.deepEqual(rows.map((r) => r.kind), ["header", "tiles", "tiles", "header", "tiles"]);
  assert.equal(new Set(rows.map((r) => r.key)).size, rows.length);
  // A header carries its whole group, for "select group".
  assert.deepEqual(rows[0]!.images, [1, 2, 3]);
});

test("offsets add up header and tile heights", () => {
  const rows = buildGalleryRows([{ label: "a", images: range(5) }], 2, true);
  assert.deepEqual(rowOffsets(rows, 100, 30), [0, 30, 130, 230, 330]);
});

test("only rows near the viewport are rendered", () => {
  const rows = buildGalleryRows([{ label: "all", images: range(10_000) }], 5, false);
  const offsets = rowOffsets(rows, 200, 30);
  assert.deepEqual(visibleRowRange(offsets, 0, 800, 0), { start: 0, end: 4 });
  assert.deepEqual(visibleRowRange(offsets, 1000, 800, 0), { start: 5, end: 9 });
  // A partly visible row at each edge is included.
  assert.deepEqual(visibleRowRange(offsets, 1050, 800, 0), { start: 5, end: 10 });
  assert.deepEqual(visibleRowRange(offsets, 1000, 800, 200), { start: 4, end: 10 });
});

test("the range stays inside the list when scrolled past either end", () => {
  const offsets = rowOffsets(buildGalleryRows([{ label: "all", images: range(20) }], 5, false), 200, 30);
  assert.deepEqual(visibleRowRange(offsets, -5000, 800, 100), { start: 0, end: 1 });
  assert.deepEqual(visibleRowRange(offsets, 5000, 800, 100), { start: 3, end: 4 });
  assert.deepEqual(visibleRowRange([0], 0, 800, 100), { start: 0, end: 0 });
});

test("Shift+click selects everything between the anchor and the target, either direction", () => {
  const order = ["a", "b", "c", "d", "e"];
  assert.deepEqual(selectionRange(order, "b", "d"), ["b", "c", "d"]);
  assert.deepEqual(selectionRange(order, "d", "b"), ["b", "c", "d"]);
  assert.deepEqual(selectionRange(order, "c", "c"), ["c"]);
});

test("Shift+click without a usable anchor selects just the target", () => {
  const order = ["a", "b", "c"];
  assert.deepEqual(selectionRange(order, null, "b"), ["b"]);
  assert.deepEqual(selectionRange(order, "gone", "b"), ["b"]);
  assert.deepEqual(selectionRange(order, "a", "gone"), []);
});
