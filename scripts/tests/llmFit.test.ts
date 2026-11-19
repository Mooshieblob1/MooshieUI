import assert from "node:assert/strict";
import test from "node:test";
import { llmFit } from "../../src/lib/utils/llmFit.ts";

const dense = { size_mb: 5520, vram_mb: 7000, cpu_moe_vram_mb: null };
const moe = { size_mb: 17770, vram_mb: 19500, cpu_moe_vram_mb: 2500 };

test("a model that fits the GPU runs there", () => {
  assert.equal(llmFit(dense, { total_vram_mb: 8192, system_ram_mb: 16384 }), "gpu");
  assert.equal(llmFit(moe, { total_vram_mb: 24576, system_ram_mb: 16384 }), "gpu");
});

test("a mixture of experts too big for the GPU keeps its experts in RAM when RAM allows", () => {
  assert.equal(llmFit(moe, { total_vram_mb: 12288, system_ram_mb: 32768 }), "experts_in_ram");
  assert.equal(llmFit(moe, { total_vram_mb: 12288, system_ram_mb: 16384 }), null);
  assert.equal(llmFit(moe, { total_vram_mb: 3072, system_ram_mb: 65536 }), null);
});

test("a dense model too big for the GPU is greyed out; no GPU means RAM decides", () => {
  assert.equal(llmFit(dense, { total_vram_mb: 6144, system_ram_mb: 65536 }), null);
  assert.equal(llmFit(dense, { total_vram_mb: 0, system_ram_mb: 16384 }), "cpu");
  assert.equal(llmFit(dense, { total_vram_mb: 0, system_ram_mb: 8192 }), null);
  assert.equal(llmFit(dense, null), null);
});
