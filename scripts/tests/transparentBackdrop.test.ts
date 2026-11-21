import assert from "node:assert/strict";
import test from "node:test";
import {
  CHECKERBOARD_CLASS,
  isMingLayerImage,
  isTransparentImage,
  mingLayerBadgeInfo,
} from "../../src/lib/utils/transparentBackdrop.ts";
import type { OutputImage } from "../../src/lib/types/index.ts";

function img(filename: string, metadata?: Record<string, string>): OutputImage {
  return {
    filename,
    subfolder: "",
    type: "output",
    prompt_id: "p1",
    metadata,
  };
}

test("CHECKERBOARD_CLASS uses Tailwind utility classes only", () => {
  assert.ok(CHECKERBOARD_CLASS.includes("bg-[conic-gradient("));
  assert.ok(!CHECKERBOARD_CLASS.includes("<style"));
});

test("isTransparentImage detects NAI, Ming-Image, and Ming-Image Layer outputs", () => {
  assert.equal(isTransparentImage(img("a.png")), false);
  assert.equal(
    isTransparentImage(img("a.png", { mooshie_transparent_background: "true" })),
    true,
  );
  assert.equal(
    isTransparentImage(img("a.png", { mooshie_novelai_transparent_background: "true" })),
    true,
  );
  assert.equal(
    isTransparentImage(img("a.png", { mooshie_model_architecture: "mingimagelayer" })),
    true,
  );
});

test("mingLayerBadgeInfo resolves index and total from metadata or sibling order", () => {
  const a = img("p1_0.png", {
    mooshie_model_architecture: "mingimagelayer",
    mooshie_layer_index: "1",
    mooshie_layer_count: "3",
  });
  const b = img("p1_1.png", { mooshie_model_architecture: "mingimagelayer" });
  const c = img("p1_0.png", { mooshie_model_architecture: "mingimagelayer" });
  assert.equal(isMingLayerImage(a), true);
  assert.deepEqual(mingLayerBadgeInfo(a), { index: 1, total: 3 });
  assert.deepEqual(mingLayerBadgeInfo(b, [b, c]), { index: 2, total: 2 });
});
