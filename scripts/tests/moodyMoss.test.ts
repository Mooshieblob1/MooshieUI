import assert from "node:assert/strict";
import test from "node:test";
import {
  MOODY_MOSS_SAMPLING,
  MOSS_IMAGE_SAMPLING,
  isMoodyMossFilename,
  recommendedSamplingFor,
} from "../../src/lib/utils/samplingRecommendation.ts";

test("Moody Moss Mix files are recognised by name", () => {
  for (const name of [
    "Moody-Moss-Mix-V4.1_00001__clean-fp8.safetensors",
    "Moody-Moss-Mix-V4.1_00001__clean-int8_convrot.safetensors",
    "Moody-Moss-Mix-V4.1_00001__clean.safetensors",
    "Moody-Moss-Mix-V2-25STEPS-CFG3_00001__clean-fp8.safetensors",
    "Moody-Moss-Mix-V1-25STEPS-CFG3_00001__clean.safetensors",
    "moody_moss_mix_v3.safetensors",
  ]) {
    assert.equal(isMoodyMossFilename(name), true, name);
  }
});

test("the V1 Turbo edition and other MOSS files are not Moody Moss Mix", () => {
  for (const name of [
    "Moody-Moss-Mix-V1_00001__clean.safetensors",
    "Moody-Moss-Mix-V1_00001__clean-fp8.safetensors",
    "moody-moss-turbo.safetensors",
    "moss_image_sft_int8_convrot.safetensors",
    "",
    null,
  ]) {
    assert.equal(isMoodyMossFilename(name), false, String(name));
  }
});

test("Moody Moss Mix gets its own sampling, other MOSS files keep MOSS's", () => {
  assert.equal(
    recommendedSamplingFor("Moody-Moss-Mix-V4.1_00001__clean-fp8.safetensors", "mossimage"),
    MOODY_MOSS_SAMPLING,
  );
  assert.equal(recommendedSamplingFor("moss_image_sft_int8_convrot.safetensors", "mossimage"), MOSS_IMAGE_SAMPLING);
  assert.equal(recommendedSamplingFor("Moody-Moss-Mix-V1_00001__clean.safetensors", "mossimage"), MOSS_IMAGE_SAMPLING);
  assert.deepEqual(
    { ...MOODY_MOSS_SAMPLING },
    { steps: 25, cfg: 3.0, samplerName: "euler", scheduler: "simple" },
  );
});
