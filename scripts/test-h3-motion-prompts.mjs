// Run: node scripts/test-h3-motion-prompts.mjs
// Check concise outputs against the production validators and rewrite contracts.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import ts from "typescript";

const cache = new Map();
function load(file) {
  file = path.resolve(file);
  if (cache.has(file)) return cache.get(file);
  const module = { exports: {} };
  const js = ts.transpileModule(fs.readFileSync(file, "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText;
  vm.runInNewContext(js, { module, exports: module.exports,
    require: spec => load(path.resolve(path.dirname(file), spec.replace(/\.js$/, ".ts"))),
  });
  cache.set(file, module.exports);
  return module.exports;
}
const h3 = load("src/lib/utils/h3Prompt.ts");
const idle = load("src/lib/utils/h3Idle.ts");
const skill = load("src/lib/utils/h3Skill.ts");
const context = taskType => ({ taskType, frames: 192, durationSeconds: "8.00", referenceImageCount: 1 });
const motion = "A fixed camera frames the seated character. Gentle breathing moves her shoulders slightly, while a light breeze sways her hair. She makes quick natural blinks with her eyes open between them. Her clothing, overall pose and the illustrated setting remain consistent. Soft leaves rustle in the background throughout the shot.";
function output(ctx) {
  if (ctx.taskType === "ref2va") return `subject_definitions: <Subject 1> is the character in <Picture 1>.
summary: [reference generation] <Subject 1> rests in the same setting.
retention_analysis: <Subject 1>: fully_preserved - appearance and overall pose.
detailed_description: Illustrated animation. [Shot 1] ${motion}
overall_soundscape: Soft rustling leaves.
non_diegetic_music: N/A`;
  const line = h3.h3InstructionLine(ctx);
  return `${line ? line + "\n\n" : ""}integrated_multimodal_description: [Shot 1] Illustrated animation. ${motion}
overall_soundscape: Soft rustling leaves.
non_diegetic_music: N/A`;
}

for (const task of ["t2va", "i2va", "fl2va", "l2va", "ref2va"]) {
  const ctx = context(task);
  assert.equal(h3.validateH3Response(output(ctx), ctx).ok, true, `${task}: concise prompt accepted`);
  assert.equal(idle.validateH3IdleResponse(output(ctx), ctx).ok, true, `${task}: concise idle accepted`);
  assert.equal(h3.validateH3Response(output(ctx).replace("overall_soundscape:", "sound:"), ctx).ok, false, `${task}: required fields retained`);
  const system = h3.h3RewriteSystemPrompt(ctx, task === "i2va" || task === "fl2va");
  assert.match(system, /quick natural blinks/);
  assert.doesNotMatch(system, /normally 350 to 500|runs 200 to 350|runs 250 to 450/);
  assert.match(idle.h3IdleRewriteSystemPrompt(ctx), /40 to 90 words/);
}
const ref = context("ref2va");
const conciseRef = output(ref);
assert.equal(h3.validateH3Response(conciseRef.replace("Illustrated animation. [Shot 1]", "[Shot 1] Illustrated animation."), ref).ok, false, "reference style remains before first shot");
assert.equal(h3.validateH3Response(conciseRef.replaceAll("<Picture 1>", "<Picture 2>"), ref).ok, false, "unavailable reference rejected");
const firstLast = context("fl2va");
assert.equal(h3.validateH3Response(output(firstLast).replace("8.00-second mark", "3.04-second mark"), firstLast).ok, false, "stale endpoint duration rejected");

const first = context("i2va");
const prompt = output(first);
assert.equal(idle.validateH3IdleResponse(prompt.replace(motion, "The camera zooms in on the seated character."), first).ok, false, "idle camera motion rejected");
assert.equal(idle.validateH3IdleResponse(prompt.replace(motion, "The camera stays fixed with no zoom. The character breathes gently."), first).ok, true, "negated camera motion and no blink remain valid");
assert.equal(idle.validateH3IdleResponse(prompt.replace(motion, "A sleeping cat breathes gently with its eyes closed."), first).ok, true, "closed-eye subject does not trigger a forced-blink retry");
assert.equal(idle.validateH3IdleResponse(prompt.replace(motion, "[Shot 2] The character turns away."), first).ok, false, "idle remains one shot");
assert.equal(idle.detectH3IdleIntent("live2D this image"), true);
assert.equal(idle.detectH3IdleIntent("The engine sits idle."), false);
assert.equal(idle.h3IdleUserPrompt("Live2D: keep her eyes closed", first), "keep her eyes closed", "explicit motion request survives trigger removal");
assert.doesNotMatch(idle.h3IdleRewriteSystemPrompt(first), /Slow, complete blinks|every 3 to 4 seconds/);
assert.match(idle.h3IdleRewriteSystemPrompt(context("t2va")), /no reference image is supplied/);
assert.match(idle.h3IdleRewriteSystemPrompt(context("l2va")), /connected reference is the last frame/);
assert.match(idle.h3IdleRewriteSystemPrompt(ref), /without inventing first- or last-frame constraints/);
assert.match(skill.h3SkillKey("test/model", first, true), /\.v2\./, "old expansive working notes are invalidated");
assert.match(skill.h3SkillAuthoringUser(first, true), /concise motion description/);
console.log("H3 motion prompts: concise output accepted for all five task types; format, idle, intent and cache regressions passed.");
