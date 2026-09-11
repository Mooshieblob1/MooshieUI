/** Regression checks for ComfyUI prompt validation errors (issue #647).
 * Run with: node scripts/test-generation-errors.mjs
 */
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import ts from "typescript";

// Compile the actual pure classifier with the project's existing TypeScript
// dependency, so this self-check also runs on the Node 20 used by CI.
const source = await readFile(new URL("../src/lib/utils/generationErrors.ts", import.meta.url), "utf8");
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
});
const { classifyGenerationError } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);

function promptError(details, message = "Value not in list", type = "value_not_in_list") {
  return `API error (400): ${JSON.stringify({
    error: { type: "prompt_outputs_failed_validation", message: "Prompt outputs failed validation" },
    node_errors: {
      "1": {
        class_type: "OTUNetLoaderW8A8",
        errors: [{ type, message, details }],
      },
    },
  })}`;
}

let checked = 0;
for (const details of [
  "lora_mode: 'default' not in ['None', 'Stochastic', 'Dynamic']",
  "weight_dtype: 'invalid' not in ['default', 'fp16', 'bf16', 'fp32']",
  "sampler_name: 'removed_sampler' not in ['euler']",
]) {
  const plain = `Value not in list: ${details}`;
  for (const [input, expectedMessage] of [
    [plain, plain],
    [{ exception_message: plain, node_type: "OTUNetLoaderW8A8" }, plain],
    [promptError(details), details],
    [{ error: promptError(details) }, details],
  ]) {
    assert.deepEqual(classifyGenerationError(input), {
      messageKey: "generation.toast.failed_detail",
      params: { message: expectedMessage },
      durationMs: 8000,
    }, `Enum validation should show the rejected setting: ${JSON.stringify(input)}`);
    checked++;
  }
}

// Real missing models must retain the model-specific recovery advice, including
// numbered text-encoder inputs used by DualCLIPLoader/TripleCLIPLoader.
for (const field of ["ckpt_name", "unet_name", "vae_name", "lora_name", "clip_name", "clip_name2", "control_net_name", "model_name", "upscale_model"]) {
  for (const quoted of [true, false]) {
    const model = "models/missing.safetensors";
    const details = `${field}: ${quoted ? `'${model}'` : model} not in []`;
    for (const input of [`Value not in list: ${details}`, promptError(details)]) {
      assert.deepEqual(classifyGenerationError(input), {
        messageKey: "generation.error.model_not_found",
        params: { model },
        durationMs: 8000,
      });
      checked++;
    }
  }
}

// A filename appearing in other validation context is not proof it is missing.
const unrelatedModel = promptError("lora_mode: 'default' not in ['None']");
const withModelContext = unrelatedModel.replace('"errors":', '"inputs":{"unet_name":"valid.safetensors"},"errors":');
assert.equal(classifyGenerationError(withModelContext).messageKey, "generation.toast.failed_detail");
checked++;

assert.deepEqual(classifyGenerationError(promptError("lora_mode", "Required input is missing", "required_input_missing")), {
  messageKey: "generation.toast.failed_detail",
  params: { message: "lora_mode" },
  durationMs: 8000,
});
checked++;

assert.deepEqual(classifyGenerationError('API error (400): {"error":{"type":"prompt_outputs_failed_validation","message":"Prompt outputs failed validation"}}'), {
  messageKey: "generation.toast.failed_detail",
  params: { message: "Prompt outputs failed validation" },
  durationMs: 8000,
});
checked++;

for (const [input, messageKey] of [
  ["Unknown quantization format for layer double_blocks.0.img_attn.qkv", "generation.error.int8_fast_required"],
  ["CUDA out of memory", "generation.error.out_of_memory"],
  ["RFInversion is not installed", "generation.style_transfer.execution_failed"],
  ["An unrelated error", "generation.toast.failed"],
]) {
  assert.equal(classifyGenerationError(input).messageKey, messageKey);
  checked++;
}

console.log(`Generation error regression checks passed (${checked} cases).`);
