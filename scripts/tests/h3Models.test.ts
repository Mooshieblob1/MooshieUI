import test from "node:test";
import assert from "node:assert/strict";
import {
  h3Stack,
  h3Tier,
  h3TierForDiffusionModel,
  isH3TinyDiffusionModel,
} from "../../src/lib/utils/h3Models.js";

test("isH3TinyDiffusionModel matches h3_tiny and h3-tiny case-insensitively", () => {
  assert.equal(
    isH3TinyDiffusionModel("minimax_h3_tiny_17b_fl2va_int8_convrot.safetensors"),
    true,
  );
  assert.equal(isH3TinyDiffusionModel("sub/H3-Tiny_14B_ref2va.safetensors"), true);
  assert.equal(isH3TinyDiffusionModel("minimax_h3_fl2va_pruned_int8_convrot.safetensors"), false);
  assert.equal(isH3TinyDiffusionModel(null), false);
});

test("h3TierForDiffusionModel classifies H3-Tiny INT8 as tiny_int8 before int8", () => {
  assert.equal(
    h3TierForDiffusionModel("minimax_h3_tiny_17b_fl2va_int8_convrot.safetensors"),
    "tiny_int8",
  );
  assert.equal(
    h3TierForDiffusionModel("minimax_h3_tiny_17b_ref2va_int8_convrot.safetensors"),
    "tiny_int8",
  );
  assert.equal(
    h3TierForDiffusionModel("minimax_h3_fl2va_pruned_int8_convrot.safetensors"),
    "int8",
  );
});

test("tiny_int8 tier resolves pinned H3-Tiny 17B checkpoints and exact hashes", () => {
  assert.ok(h3Tier("tiny_int8"));
  const fl = h3Stack("tiny_int8", "fl2va");
  const rf = h3Stack("tiny_int8", "ref2va");
  assert.ok(fl);
  assert.ok(rf);
  assert.equal(fl.diffusion.filename, "minimax_h3_tiny_17b_fl2va_int8_convrot.safetensors");
  assert.equal(rf.diffusion.filename, "minimax_h3_tiny_17b_ref2va_int8_convrot.safetensors");
  assert.equal(fl.diffusion.sizeBytes, 18_075_272_736);
  assert.equal(rf.diffusion.sizeBytes, 18_075_272_736);
  assert.ok(
    fl.diffusion.url.includes(
      "H3-Tiny/MiniMax-H3-Tiny-ComfyUI/resolve/d2fcb8e74751895630ffa54107b9f0cd0c952ee0/",
    ),
  );
  assert.equal(
    fl.diffusion.sha256,
    "7f9032a32bc44cbe135c2803faf100745d394c3ebb94522d23717678a7a0be96",
  );
  assert.equal(
    rf.diffusion.sha256,
    "88ebaf43a09a48b501503bd0db56750f11267a2f951b6e79d4c05d264245af68",
  );
});
