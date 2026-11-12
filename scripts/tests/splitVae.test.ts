import assert from "node:assert/strict";
import test from "node:test";
import { shouldKeepSplitVae, splitVaeFitsFamily } from "../../src/lib/utils/splitVae.ts";

const installed = [
  "qwen_image_sharp_vae_bf16.safetensors",
  "qwen_image_sharp_vae_x1.5_bf16.safetensors",
  "qwen_image_vae.safetensors",
  "sdxl_vae.safetensors",
  "ae.safetensors",
  "flux2-vae.safetensors",
];

test("a sharpened Qwen Image VAE stays picked on Anima and Krea 2", () => {
  for (const family of ["anima", "krea2"] as const) {
    assert.equal(shouldKeepSplitVae("qwen_image_sharp_vae_bf16.safetensors", family, installed), true);
    assert.equal(shouldKeepSplitVae("qwen_image_sharp_vae_x1.5_bf16.safetensors", family, installed), true);
    assert.equal(shouldKeepSplitVae("qwen_image_vae.safetensors", family, installed), true);
  }
});

test("a VAE of another latent format is replaced", () => {
  assert.equal(shouldKeepSplitVae("sdxl_vae.safetensors", "anima", installed), false);
  assert.equal(shouldKeepSplitVae("ae.safetensors", "krea2", installed), false);
  assert.equal(shouldKeepSplitVae("qwen_image_vae.safetensors", "flux3klein", installed), false);
  assert.equal(shouldKeepSplitVae("flux2-vae.safetensors", "zit", installed), false);
  assert.equal(shouldKeepSplitVae("wan2.2_vae.safetensors", "anima", ["wan2.2_vae.safetensors"]), false);
});

test("an empty or uninstalled pick is replaced", () => {
  assert.equal(shouldKeepSplitVae("", "anima", installed), false);
  assert.equal(shouldKeepSplitVae("  ", "anima", installed), false);
  assert.equal(shouldKeepSplitVae("qwen_image_sharp_vae_x0.5_bf16.safetensors", "anima", installed), false);
});

test("an inventory that has not loaded yet is no evidence the file is gone", () => {
  assert.equal(shouldKeepSplitVae("qwen_image_sharp_vae_bf16.safetensors", "anima", []), true);
});

test("subfolder separators do not matter", () => {
  assert.equal(shouldKeepSplitVae("Anima/qwen_image_sharp_vae_bf16.safetensors", "anima", ["Anima\\qwen_image_sharp_vae_bf16.safetensors"]), true);
});

test("families without a VAE rule keep the pick", () => {
  assert.equal(splitVaeFitsFamily("anything.safetensors", "unknown"), null);
  assert.equal(shouldKeepSplitVae("sdxl_vae.safetensors", "unknown", installed), true);
});

test("the Wan 2.1 VAE fits the Qwen latent families", () => {
  assert.equal(splitVaeFitsFamily("wan_2.1_vae.safetensors", "anima"), true);
  assert.equal(splitVaeFitsFamily("ae.safetensors", "flux1d"), true);
  assert.equal(splitVaeFitsFamily("flux2-vae_small.safetensors", "mossimage"), true);
});
