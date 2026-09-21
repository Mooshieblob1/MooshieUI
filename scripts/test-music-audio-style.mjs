// Exercise the production request lifecycle with synthetic IPC. No cloud calls.
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import ts from "typescript";
const module = { exports: {} };
const calls = [];
let current = true, replies = [], delay, startHook = () => {};
const profile = { description: "Keys and a quiet singer.", style: "Instrumental soul, warm keys, compact ending.", estimates: ["Exact tempo is uncertain."], audio_sha256: "a".repeat(64), backend_id: "chosen-model", duration_seconds: 60 };
const api = {
  async analyzeMusicAudioStyle(audio, target, backend) { calls.push({ audio, target, backend }); startHook(); return "job"; },
  async getMusicAudioStyle(id, cancel = false) {
    calls.push({ id, cancel });
    if (cancel) return { status: "cancelled" };
    if (delay) await delay;
    return replies.shift() ?? { status: "completed", profile };
  },
};
const source = fs.readFileSync("src/lib/utils/musicAudioStyle.ts", "utf8");
vm.runInNewContext(ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText, {
  module, exports: module.exports,
  require: name => name === "./api.js" ? api : { blobBase64: async () => "real-audio-bytes" },
  setTimeout: fn => { fn(); },
});
const { describeAudio, validateAudioStyle } = module.exports;
const blob = { size: 100 }, target = { instrumental: true, max_duration: 45, language: "en" };
const run = () => describeAudio(blob, target, "chosen-model", () => current, () => {});
const reset = () => { calls.length = 0; current = true; replies = []; delay = null; startHook = () => {}; };
assert.equal((await run()).style, profile.style);
assert.deepEqual(calls[0], { audio: "real-audio-bytes", target, backend: "chosen-model" });
assert.equal(calls.filter(c => c.cancel).length, 0, "Completed result consumed without cancelling a different job");
for (const bad of [{ ...profile, backend_id: "other-model" }, { ...profile, audio_sha256: "" }, { ...profile, duration_seconds: 361 }, { ...profile, style: "" }, { ...profile, estimates: [] }]) {
  assert.throws(() => validateAudioStyle(bad, "chosen-model"), /invalid_audio_style/);
}
reset(); startHook = () => { current = false; };
assert.equal(await run(), null);
assert.equal(calls.at(-1).cancel, true, "A job accepted after source replacement is cancelled");
reset(); let finish;
delay = new Promise(resolve => { finish = resolve; });
const pending = run();
for (let i = 0; i < 8; i++) await Promise.resolve();
current = false; finish();
assert.equal(await pending, null, "Late response after account/source changes cannot become a draft");
assert.equal(calls.at(-1).cancel, true);
reset(); current = false; assert.equal(await run(), null); assert.equal(calls.length, 0);
reset(); replies = [{ status: "running" }, { status: "error", error: "provider unavailable" }];
await assert.rejects(run, /provider unavailable/);
assert.equal(calls.filter(c => c.audio).length, 1, "No automatic paid retries or text-only fallback");
reset();
await assert.rejects(() => describeAudio({ size: 64 * 1024 * 1024 + 1 }, target, "chosen-model", () => true, () => {}), /music_audio_size/);
assert.equal(calls.length, 0);
console.log("PASS: audio payload, instrumental target, output validation, stale source/account cancellation, polling, size limit and no paid retries");
