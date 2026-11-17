import assert from "node:assert/strict";
import test from "node:test";
import {
  encoderMatches,
  findFamily,
  registeredMode,
  registeredSampling,
  setArchitectures,
  slotMatches,
  type ArchitectureSpec,
} from "../../src/lib/utils/architectures.ts";
import { familyRequiresSeparateClip, familyUsesTagPrompts, isKnownModelFamily, modelFamilyLabel } from "../../src/lib/utils/modelFamily.ts";
import { splitVaeFitsFamily } from "../../src/lib/utils/splitVae.ts";
import { recommendedSamplingFor } from "../../src/lib/utils/samplingRecommendation.ts";

const sampling = { steps: 25, cfg: 1, sampler: "euler", scheduler: "simple", width: 1024, height: 1024 };

const fixture: ArchitectureSpec = {
  id: "fixture",
  label: "Fixture",
  families: [
    {
      family: "fixture",
      label: "Fixture Image",
      turbo: false,
      sampling,
      recommended: [],
      text_encoder: null,
      vae: null,
    },
  ],
  split_only: true,
  prompt_style: "natural",
  tensor_architecture: "fixture_dit",
  text_encoder: {
    clip_type: "fixture_clip",
    markers: ["fixtenc"],
    strict: true,
    download: {
      filename: "fixture_te_8b.safetensors",
      url: "https://example.invalid/te",
      category: "text_encoders",
      size: null,
      sha256: null,
    },
  },
  vae: { basenames: ["fixture_vae.safetensors"], markers: ["fixture_vae"], strict: true, download: null },
  model_slots: [
    { key: "second", label: "second expert", category: "diffusion_models", markers: ["second"], strict: true, download: null },
  ],
  core_nodes: [],
  min_comfyui: null,
  int8_fast_model_type: null,
  modes: [
    { mode: "txt2img", max_references: 0, required_files: [], sampling: null },
    {
      mode: "image_edit",
      max_references: 4,
      required_files: [],
      sampling: { ...sampling, steps: 30, cfg: 4 },
    },
  ],
  negative_prompt: true,
  resolution_multiple: 16,
  controlnet: true,
};

test("registered families are known, labelled and split-only", () => {
  setArchitectures([]);
  assert.equal(isKnownModelFamily("fixture"), false);
  setArchitectures([fixture]);
  assert.equal(isKnownModelFamily("fixture"), true);
  assert.equal(isKnownModelFamily("sdxl"), true);
  assert.equal(modelFamilyLabel("fixture"), "Fixture Image");
  assert.equal(modelFamilyLabel("krea2"), "Krea 2");
  assert.equal(familyRequiresSeparateClip("fixture"), true);
  assert.equal(familyUsesTagPrompts("fixture"), false);
  assert.equal(findFamily("nope"), null);
});

test("mode sampling overrides the family's", () => {
  setArchitectures([fixture]);
  assert.equal(registeredSampling("fixture")?.steps, 25);
  assert.equal(registeredSampling("fixture", "txt2img")?.steps, 25);
  assert.equal(registeredSampling("fixture", "image_edit")?.cfg, 4);
  assert.equal(registeredMode("fixture", "image_edit")?.max_references, 4);
  assert.equal(registeredMode("fixture", "img2img"), null);
  assert.equal(recommendedSamplingFor("fixture_bf16.safetensors", "fixture")?.samplerName, "euler");
});

test("encoder, VAE and slot matching follow the declared markers", () => {
  setArchitectures([fixture]);
  const encoder = fixture.text_encoder!;
  assert.equal(encoderMatches(encoder, "sub/FixtEnc_int8.safetensors"), true);
  assert.equal(encoderMatches(encoder, "fixture_te_8b.safetensors"), true);
  assert.equal(encoderMatches(encoder, "umt5.safetensors"), false);
  assert.equal(splitVaeFitsFamily("x/Fixture_VAE.safetensors", "fixture"), true);
  assert.equal(splitVaeFitsFamily("sdxl_vae.safetensors", "fixture"), false);
  assert.equal(slotMatches(fixture.model_slots[0], "wan_second_expert.safetensors"), true);
  assert.equal(slotMatches(fixture.model_slots[0], "other.safetensors"), false);
});
