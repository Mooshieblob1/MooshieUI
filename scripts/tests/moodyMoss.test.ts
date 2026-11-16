import assert from "node:assert/strict";
import test from "node:test";
import {
  MOODY_MOSS_SAMPLING,
  MOSS_IMAGE_SAMPLING,
  isMoodyMossFilename,
  recommendedSamplingFor,
} from "../../src/lib/utils/samplingRecommendation.ts";
import {
  MOODY_MOSS_FILES,
  MOODY_MOSS_FINISH_DEFAULTS,
  missingMoodyMossFiles,
  moodyMossFinishPayload,
  moodyMossLongEdgeSize,
  moodyMossRequiredFiles,
  restoreMoodyMossSettings,
} from "../../src/lib/utils/moodyMoss.ts";

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

test("finishing settings go to Rust in snake_case with LoRA lists", () => {
  const payload = moodyMossFinishPayload({
    ...MOODY_MOSS_FINISH_DEFAULTS,
    faceDetail: true,
    faceLora: " face.safetensors ",
    faceLoraStrength: 0.8,
    seedvr2: true,
    seedvr2Large: true,
  });
  assert.deepEqual(payload.face_loras, [
    { name: "face.safetensors", strength_model: 0.8, strength_clip: 0.8 },
  ]);
  assert.deepEqual(payload.area_loras, []);
  assert.equal(payload.seedvr2_7b, true);
  assert.equal(payload.seedvr2_mix, 0.4);
  assert.equal(payload.long_edge, 4096);
  assert.equal(payload.face_denoise, 0.35);
  assert.equal(payload.area_denoise, 0.45);
  assert.equal(payload.skin_contrast_strength, 0.2);
});

test("saved finishing settings keep the defaults for missing or mistyped fields", () => {
  const restored = restoreMoodyMossSettings({ faceDetail: true, longEdge: "huge", extra: 1 });
  assert.equal(restored.faceDetail, true);
  assert.equal(restored.longEdge, 4096);
  assert.equal("extra" in restored, false);
  assert.deepEqual(restoreMoodyMossSettings(null), MOODY_MOSS_FINISH_DEFAULTS);
});

test("the SeedVR2 finish scales the long edge", () => {
  assert.deepEqual(moodyMossLongEdgeSize(1248, 1872, 4096), { width: 2731, height: 4096 });
  assert.deepEqual(moodyMossLongEdgeSize(1872, 1248, 3072), { width: 3072, height: 2048 });
});

test("only the turned-on steps need models, and installed ones are skipped", () => {
  assert.deepEqual(moodyMossRequiredFiles(MOODY_MOSS_FINISH_DEFAULTS), []);
  const all = { ...MOODY_MOSS_FINISH_DEFAULTS, faceDetail: true, seedvr2: true };
  assert.deepEqual(
    moodyMossRequiredFiles(all).map((f) => f.filename),
    [
      MOODY_MOSS_FILES.faceDetector.filename,
      MOODY_MOSS_FILES.seedvr2Small.filename,
      MOODY_MOSS_FILES.seedvr2Vae.filename,
      MOODY_MOSS_FILES.nomos.filename,
    ],
  );
  assert.equal(
    moodyMossRequiredFiles({ ...all, seedvr2Mix: 0 }).some((f) => f === MOODY_MOSS_FILES.nomos),
    false,
  );
  const missing = missingMoodyMossFiles(moodyMossRequiredFiles(all), {
    diffusionModels: ["seedvr2/seedvr2_3b_int8_convrot.safetensors"],
    vaes: ["seedvr2_ema_vae_fp16.safetensors"],
    upscaleModels: [],
    ultralyticsModels: ["face_yolov8m.pt"],
  });
  assert.deepEqual(missing, [MOODY_MOSS_FILES.nomos]);
});
