import { test } from "node:test";
import assert from "node:assert/strict";
import {
  CASCADE_STAGE_A,
  CASCADE_STAGE_B_CHECKPOINT,
  isCascadeEffnet,
  isCascadeStageA,
  isCascadeStageB,
  isCascadeTextEncoder,
  planCascadeStageB,
  sameFiles,
  withCascadeFiles,
} from "../../src/lib/utils/cascade.ts";

const none = { checkpoints: [], diffusionModels: [], vaes: [] };

test("stage file names are recognised", () => {
  assert.ok(isCascadeStageB("stable_cascade_stage_b.safetensors"));
  assert.ok(isCascadeStageB("cascade\\stage_b_lite_bf16.safetensors"));
  assert.ok(!isCascadeStageB("stable_cascade_stage_c.safetensors"));
  assert.ok(isCascadeStageA("stage_a.safetensors"));
  assert.ok(!isCascadeStageA("stage_b.safetensors"));
  assert.ok(isCascadeEffnet("effnet_encoder.safetensors"));
  assert.ok(isCascadeTextEncoder("stable_cascade_text_encoder.safetensors"));
  assert.ok(isCascadeTextEncoder("clip_g.safetensors"));
  assert.ok(!isCascadeTextEncoder("t5xxl_fp16.safetensors"));
});

test("nothing installed downloads the all-in-one stage B checkpoint", () => {
  assert.deepEqual(planCascadeStageB(none, {}), { files: null, download: [CASCADE_STAGE_B_CHECKPOINT] });
});

test("an installed stage B checkpoint wins over split files", () => {
  const plan = planCascadeStageB(
    {
      checkpoints: ["sdxl.safetensors", "stable_cascade_stage_b.safetensors"],
      diffusionModels: ["stage_b_bf16.safetensors"],
      vaes: ["stage_a.safetensors"],
    },
    {},
  );
  assert.deepEqual(plan.files, { stage_b: "stable_cascade_stage_b.safetensors" });
});

test("split stage B needs stage A, and prefers the full model over lite", () => {
  const unets = ["stage_b_lite_bf16.safetensors", "stage_b_bf16.safetensors"];
  assert.deepEqual(planCascadeStageB({ ...none, diffusionModels: unets }, {}), {
    files: null,
    download: [CASCADE_STAGE_A],
  });
  const plan = planCascadeStageB({ ...none, diffusionModels: unets, vaes: ["stage_a.safetensors"] }, {});
  assert.deepEqual(plan.files, { stage_b_unet: "stage_b_bf16.safetensors", stage_a: "stage_a.safetensors" });
  // A file the user already uses stays chosen.
  const kept = planCascadeStageB(
    { ...none, diffusionModels: unets, vaes: ["stage_a.safetensors"] },
    { stage_b_unet: "stage_b_lite_bf16.safetensors" },
  );
  assert.equal(kept.files?.stage_b_unet, "stage_b_lite_bf16.safetensors");
});

test("switching layouts clears the other Cascade slots and keeps unrelated ones", () => {
  const next = withCascadeFiles(
    { stage_b_unet: "stage_b_bf16.safetensors", stage_a: "stage_a.safetensors", other: "x" },
    { stage_b: "stable_cascade_stage_b.safetensors" },
  );
  assert.deepEqual(next, { other: "x", stage_b: "stable_cascade_stage_b.safetensors" });
  assert.ok(sameFiles(next, { stage_b: "stable_cascade_stage_b.safetensors", other: "x" }));
  assert.ok(!sameFiles(next, { other: "x" }));
});
