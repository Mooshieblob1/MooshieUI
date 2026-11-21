import assert from "node:assert/strict";
import test from "node:test";
import {
  autoQualityTagsForImport,
  inferQualityFamilyFromModelName,
  type AutoQualityTagSets,
} from "../../src/lib/utils/promptQualityStrip.ts";

const SETS: AutoQualityTagSets = {
  animaPositive: "newest, masterpiece, best quality, score_9, score_8",
  animaNegative: "worst quality, low quality, score_1",
  illustriousPositive: "best quality, masterpiece, absurdres, newest, very aesthetic",
  illustriousNegative: "worst quality, bad quality, low quality, lowres, watermark",
  ponyPositive: "score_9, score_8_up, score_7_up",
  ponyNegative: "score_1, score_2, score_3, worst quality",
  nanosaurPositive: "newest, masterpiece, best quality, absurdres",
  nanosaurNegative: "oldest, low quality, blurry",
};

test("autoQualityTagsForImport strips nothing when autoQualityEnabled is false or family injects none", () => {
  assert.equal(
    autoQualityTagsForImport("illustrious", "positive", false, SETS).size,
    0,
  );
  assert.equal(
    autoQualityTagsForImport("unknown", "positive", true, SETS).size,
    0,
  );
  assert.equal(
    autoQualityTagsForImport("sdxl", "negative", true, SETS).size,
    0,
  );
});

test("autoQualityTagsForImport returns only the matching family and side", () => {
  const illuPos = autoQualityTagsForImport("illustrious", "positive", true, SETS);
  assert.ok(illuPos.has("best quality"));
  assert.ok(illuPos.has("masterpiece"));
  assert.ok(!illuPos.has("score_9"));
  assert.ok(!illuPos.has("worst quality"));

  const illuNeg = autoQualityTagsForImport("illustrious", "negative", true, SETS);
  assert.ok(illuNeg.has("worst quality"));
  assert.ok(!illuNeg.has("score_1"));
  assert.ok(!illuNeg.has("masterpiece"));

  const ponyPos = autoQualityTagsForImport("pony", "positive", true, SETS);
  assert.ok(ponyPos.has("score_9"));
  assert.ok(!ponyPos.has("masterpiece"));

  const nanoNeg = autoQualityTagsForImport("nanosaur", "negative", true, SETS);
  assert.ok(nanoNeg.has("oldest"));
  assert.ok(!nanoNeg.has("watermark"));
});

test("inferQualityFamilyFromModelName detects redLilyIllu_v10 and avoids false positives", () => {
  assert.equal(
    inferQualityFamilyFromModelName("redLilyIllu_v10.safetensors"),
    "illustrious",
  );
  assert.equal(inferQualityFamilyFromModelName("wai_ilxl_v14.safetensors"), "illustrious");
  assert.equal(inferQualityFamilyFromModelName("noob_vpred.safetensors"), "illustrious");
  assert.equal(inferQualityFamilyFromModelName("illusion_xl.safetensors"), "unknown");
  assert.equal(inferQualityFamilyFromModelName("illustration_mix.safetensors"), "unknown");
  assert.equal(inferQualityFamilyFromModelName("animayume_v05.safetensors"), "anima");
  assert.equal(inferQualityFamilyFromModelName("animagine_xl.safetensors"), "unknown");
});
