// Run: node scripts/test-video-acceleration.mjs
// Exercise settings migration and submission readiness without a browser framework.
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import ts from "typescript";

const source = fs.readFileSync(new URL("../src/lib/utils/videoParams.ts", import.meta.url), "utf8");
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
});
const module = { exports: {} };
vm.runInNewContext(outputText, { module, exports: module.exports });
const { resolveVideoAcceleration } = module.exports;

for (const [saved, legacy, expected] of [
  [undefined, undefined, "standard"], [undefined, false, "standard"],
  [undefined, true, "turbo"], [null, true, "turbo"],
  ["standard", true, "standard"], ["turbo", false, "turbo"],
  ["vdn", true, "standard"], ["vdn", false, "standard"],
  ["invalid", false, "standard"], [false, "true", "standard"],
]) {
  const actual = resolveVideoAcceleration(saved, legacy);
  assert.equal(actual, expected);
  const restored = JSON.parse(JSON.stringify({ videoAcceleration: actual, videoTurboEnabled: actual === "turbo" }));
  assert.equal(resolveVideoAcceleration(restored.videoAcceleration, restored.videoTurboEnabled), expected);
}
console.log("Video acceleration: 10 migration and round-trip cases passed.");

const presetsSource = fs.readFileSync(new URL("../src/lib/utils/h3Models.ts", import.meta.url), "utf8");
const presetsJs = ts.transpileModule(presetsSource, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
const presetModule = { exports: {} };
vm.runInNewContext(presetsJs, { module: presetModule, exports: presetModule.exports });
const { h3TurboPreset, H3_TURBO_PRESETS } = presetModule.exports;
assert.equal(h3TurboPreset(undefined, "fl2va").id, "larryvrh");
assert.equal(h3TurboPreset("unknown", "ref2va").id, "larryvrh");
assert.equal(h3TurboPreset("larryvrh", "ref2va").id, "larryvrh");
assert.equal(h3TurboPreset("lightx2v_fl2v_4", "ref2va").id, "lightx2v_ref2v_8");
assert.equal(h3TurboPreset("lightx2v_ref2v_8", "fl2va").id, "lightx2v_fl2v_8");
for (const preset of H3_TURBO_PRESETS.filter(p => p.id !== "larryvrh")) {
  assert.equal(h3TurboPreset(preset.id, preset.variant).id, preset.id);
  assert.match(preset.file.sha256, /^[a-f0-9]{64}$/);
  assert.match(preset.file.url, /resolve\/[a-f0-9]{40}\//);
  assert.equal(preset.file.sizeBytes, 1956193000);
}
console.log("Turbo presets: legacy defaults, task switching and three pinned download manifests passed.");

// Run the production accessors with controlled model/server state. Extracting
// them avoids booting the full rune store or copying its readiness conditions.
const storeSource = ts.createSourceFile("generation.svelte.ts", fs.readFileSync(
  new URL("../src/lib/stores/generation.svelte.ts", import.meta.url), "utf8",
), ts.ScriptTarget.Latest, true);
const storeClass = storeSource.statements.find(node => ts.isClassDeclaration(node) && node.name?.text === "GenerationStore");
assert.ok(storeClass);
const readinessGetters = ["canGenerate", "videoReady", "videoModelsReady", "videoDiffusionModelLooksLikeH3", "videoDiffusionModelMatchesVariant", "videoRefImageFilenames"];
const getters = readinessGetters.map(name => {
  const getter = storeClass.members.find(node => ts.isGetAccessor(node) && node.name.getText(storeSource) === name);
  assert.ok(getter, `Missing production getter: ${name}`);
  return getter.getText(storeSource);
}).join("\n");
let timelineActive = false;
const Readiness = vm.runInNewContext(ts.transpileModule(`class Readiness { ${getters} } Readiness;`, {
  compilerOptions: { target: ts.ScriptTarget.ES2022 },
}).outputText, {
  H3_DIFFUSION_MARKERS: module.exports.H3_DIFFUSION_MARKERS,
  isTimelineActive: () => timelineActive,
});
function readyVideo(overrides = {}) {
  return Object.assign(new Readiness(), {
    mode: "video", checkpoint: null,
    videoAcceleration: "turbo", videoAccelerationInstalling: false, videoAccelerationReady: true,
    videoDiffusionModel: "minimax_h3_fl2va_pruned_nvfp4.safetensors",
    videoClipModel: "text_encoder.safetensors", videoVaeModel: "video_vae.safetensors",
    videoAudioVaeModel: "audio_vae.safetensors", videoVariant: "fl2va", videoRefImages: [],
    videoSaveDraft: false, videoDraftFits: true, videoDraftNodesReady: true,
  }, overrides);
}
const readinessCases = [
  ["ready Turbo without an image checkpoint", {}, true],
  ["Turbo node unavailable despite an image checkpoint", { checkpoint: "image.safetensors", videoAccelerationReady: false }, false],
  ["Turbo installation still running", { videoAccelerationInstalling: true }, false],
  ["Turbo GGUF selection", { videoDiffusionModel: "minimax_h3_fl2va.GGUF" }, true],
  ["stale VDN state cannot generate", { videoAcceleration: "vdn" }, false],
  ["migrated VDN generates with Standard", { videoAcceleration: resolveVideoAcceleration("vdn", true), videoAccelerationReady: false }, true],
  ["missing video encoder", { videoClipModel: null }, false],
  ["missing video VAE", { videoVaeModel: "" }, false],
  ["missing audio VAE", { videoAudioVaeModel: "  " }, false],
  ["non-H3 diffusion model", { videoDiffusionModel: "image.safetensors" }, false],
  ["mismatched H3 variant", { videoDiffusionModel: "minimax_h3_ref2va.safetensors" }, false],
  ["draft nodes unavailable", { videoSaveDraft: true, videoDraftNodesReady: false }, false],
  ["draft resolution too large", { videoSaveDraft: true, videoDraftFits: false }, false],
  ["ready draft", { videoSaveDraft: true }, true],
  ["standard without acceleration nodes", { videoAcceleration: "standard", videoAccelerationReady: false }, true],
  ["Turbo nodes unavailable", { videoAcceleration: "turbo", videoAccelerationReady: false }, false],
  ["ready Turbo", { videoAcceleration: "turbo" }, true],
  ["image mode needs checkpoint", { mode: "txt2img" }, false],
  ["image mode independent of video readiness", { mode: "txt2img", checkpoint: "image.safetensors", videoAccelerationReady: false }, true],
];
for (const [name, overrides, expected] of readinessCases) {
  assert.equal(readyVideo(overrides).canGenerate, expected, name);
}
const referenceVideo = readyVideo({ videoVariant: "ref2va", videoDiffusionModel: "minimax_h3_ref2va.safetensors" });
assert.equal(referenceVideo.canGenerate, false, "references are required without a timeline");
timelineActive = true;
assert.equal(referenceVideo.canGenerate, true, "timeline provides its own references");
timelineActive = false;
referenceVideo.videoRefImages = ["reference.png"];
assert.equal(referenceVideo.canGenerate, true, "uploaded reference enables generation");

const buttonSource = fs.readFileSync(new URL("../src/lib/components/generation/GenerateButton.svelte", import.meta.url), "utf8");
const buttonScript = ts.createSourceFile("GenerateButton.ts", buttonSource.match(/<script[^>]*>([\s\S]*?)<\/script>/)[1], ts.ScriptTarget.Latest, true);
const generateHandler = buttonScript.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === "handleGenerate");
assert.ok(generateHandler);
const handlerJs = ts.transpileModule(generateHandler.getText(buttonScript), {
  compilerOptions: { target: ts.ScriptTarget.ES2022 },
}).outputText;
for (const state of [{ videoAccelerationReady: false }, { videoAccelerationInstalling: true }]) {
  // No submission dependencies are supplied: a disabled shortcut must return
  // before queue state, prompt preparation, or backend calls are touched.
  await vm.runInNewContext(`${handlerJs}\nhandleGenerate();`, { generation: readyVideo(state) });
}
console.log(`Video readiness: ${readinessCases.length + 3} model/server/reference cases and both blocked submission paths passed.`);
