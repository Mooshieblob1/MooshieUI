import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  clampOutpaintFeather,
  clampOutpaintPadding,
  computeOutpaintDimensions,
  computeOutpaintMaskValue,
  hasOutpaintPadding,
} from "../../src/lib/utils/outpaintCanvas.ts";

describe("outpaintCanvas helpers", () => {
  it("clamps padding and feather within supported bounds", () => {
    assert.equal(clampOutpaintPadding(-32), 0);
    assert.equal(clampOutpaintPadding(128.4), 128);
    assert.equal(clampOutpaintPadding(9999), 2048);
    assert.equal(clampOutpaintFeather(-5), 0);
    assert.equal(clampOutpaintFeather(40.6), 41);
    assert.equal(clampOutpaintFeather(1000), 512);
  });

  it("computes expanded canvas geometry and source offsets", () => {
    const pad = { top: 64, bottom: 128, left: 256, right: 0, feather: 40 };
    const geom = computeOutpaintDimensions(800, 600, pad);
    assert.equal(geom.width, 1056);
    assert.equal(geom.height, 792);
    assert.equal(geom.srcX, 256);
    assert.equal(geom.srcY, 64);
    assert.equal(geom.hasPadding, true);
    assert.equal(hasOutpaintPadding({ top: 0, bottom: 0, left: 0, right: 0, feather: 40 }), false);
  });

  it("feathers mask inward only along padded sides", () => {
    const pad = { top: 0, bottom: 0, left: 128, right: 0, feather: 40 };
    const geom = computeOutpaintDimensions(512, 512, pad);
    // Padded left margin is fully masked (255)
    assert.equal(computeOutpaintMaskValue(10, 200, geom, pad), 255);
    // Just inside padded left boundary is heavily masked
    assert.equal(computeOutpaintMaskValue(128, 200, geom, pad), 255);
    // Halfway through 40px feather band is ~128
    assert.equal(computeOutpaintMaskValue(148, 200, geom, pad), 128);
    // Center is preserved (0)
    assert.equal(computeOutpaintMaskValue(300, 200, geom, pad), 0);
    // Unpadded top and right edges stay preserved (0)
    assert.equal(computeOutpaintMaskValue(300, 0, geom, pad), 0);
    assert.equal(computeOutpaintMaskValue(geom.width - 1, 200, geom, pad), 0);
  });
});
