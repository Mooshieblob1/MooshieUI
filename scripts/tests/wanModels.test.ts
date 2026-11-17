import assert from "node:assert/strict";
import test from "node:test";
import {
  findWanLowNoisePartner,
  isWan21Vae,
  isWan22Vae,
  isWanEncoder,
  wanVariant,
} from "../../src/lib/utils/wanModels.ts";

test("variants match the Rust rules in templates/wan.rs", () => {
  const cases: [string, string][] = [
    ["wan2.1_t2v_14B_fp8_e4m3fn.safetensors", "wan21"],
    ["wan2.1_t2v_1.3B_fp16.safetensors", "wan21"],
    ["Wan2.1-T2V-14B-Q8_0.gguf", "wan21"],
    ["wan2.2_ti2v_5B_fp16.safetensors", "ti2v5b"],
    ["wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors", "a14bHigh"],
    ["wan2.2_t2v_low_noise_14B_fp16.safetensors", "a14bLow"],
    ["Wan2.2-T2V-A14B-HighNoise-Q6_K.gguf", "a14bHigh"],
    ["smoothMixWan22_lowNoise.safetensors", "a14bLow"],
    ["wan22/t2v_high_fp8.safetensors", "a14bHigh"],
    ["wan2.2_i2v_high_noise_14B_fp8_scaled.safetensors", "notTextToImage"],
    ["wan2.1_i2v_480p_14B_fp16.safetensors", "notTextToImage"],
    ["wan2.2_fun_control_5B_bf16.safetensors", "notTextToImage"],
    ["wan2.1_vace_14B_fp16.safetensors", "notTextToImage"],
    ["chrono_edit_14B_fp16.safetensors", "notTextToImage"],
  ];
  for (const [name, expected] of cases) assert.equal(wanVariant(name), expected, name);
});

test("the high-noise model pairs with its own low-noise file", () => {
  const installed = [
    "wan2.2_t2v_low_noise_14B_fp16.safetensors",
    "wan2.2_t2v_low_noise_14B_fp8_scaled.safetensors",
    "wan2.2_i2v_low_noise_14B_fp8_scaled.safetensors",
    "wan2.1_t2v_14B_fp16.safetensors",
  ];
  assert.equal(
    findWanLowNoisePartner("wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors", installed),
    "wan2.2_t2v_low_noise_14B_fp8_scaled.safetensors",
  );
  assert.equal(
    findWanLowNoisePartner("Wan2.2/wan2.2_t2v_high_noise_14B_fp16.safetensors", installed),
    "wan2.2_t2v_low_noise_14B_fp16.safetensors",
  );
  // No same-name partner and two candidates: no guess.
  assert.equal(findWanLowNoisePartner("smoothMixWan22_highNoise.safetensors", installed), null);
  // One candidate: use it.
  assert.equal(
    findWanLowNoisePartner("smoothMixWan22_highNoise.safetensors", ["smoothMixWan22_lowNoise_v2.safetensors"]),
    "smoothMixWan22_lowNoise_v2.safetensors",
  );
  assert.equal(findWanLowNoisePartner("wan2.1_t2v_14B_fp16.safetensors", installed), null);
  assert.equal(findWanLowNoisePartner("wan2.2_t2v_high_noise_14B_fp16.safetensors", []), null);
});

test("encoder and VAE names", () => {
  assert.equal(isWanEncoder("umt5_xxl_fp8_e4m3fn_scaled.safetensors"), true);
  assert.equal(isWanEncoder("qwen_2.5_vl_7b_fp8_scaled.safetensors"), false);
  assert.equal(isWan21Vae("wan_2.1_vae.safetensors"), true);
  assert.equal(isWan21Vae("qwen_image_vae.safetensors"), true);
  assert.equal(isWan21Vae("wan2.2_vae.safetensors"), false);
  assert.equal(isWan22Vae("wan2.2_vae.safetensors"), true);
  assert.equal(isWan22Vae("wan_2.1_vae.safetensors"), false);
});
