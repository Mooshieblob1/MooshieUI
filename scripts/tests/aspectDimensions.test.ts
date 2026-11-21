import assert from "node:assert/strict";
import test from "node:test";
import { BUCKET_DIMENSION_STEP, dimsForAspect } from "../../src/lib/utils/aspectDimensions.ts";

test("SDXL/NovelAI 64px under-area buckets at 1024 match standard training resolutions", () => {
  const cases: Array<[number, number, number, number]> = [
    [1, 1, 1024, 1024],
    [4, 3, 1152, 896],
    [3, 2, 1216, 832],
    [16, 9, 1344, 768],
    [21, 9, 1536, 640],
    [3, 4, 896, 1152],
    [2, 3, 832, 1216],
    [9, 16, 768, 1344],
  ];

  for (const [aw, ah, expectedW, expectedH] of cases) {
    assert.deepEqual(
      dimsForAspect(aw, ah, 1024, BUCKET_DIMENSION_STEP, true),
      { w: expectedW, h: expectedH },
      `${aw}:${ah} at 1024`,
    );
  }
});

test("non-bucket 8px mode preserves area-faithful rounding", () => {
  assert.deepEqual(dimsForAspect(1, 1, 1024, 8, false), { w: 1024, h: 1024 });
  assert.deepEqual(dimsForAspect(3, 2, 1024, 8, false), { w: 1248, h: 840 });
  assert.deepEqual(dimsForAspect(2, 3, 1024, 8, false), { w: 840, h: 1248 });
});
