// Run: node scripts/test-music-writing.mjs
// Exercise the real writing skill and store with a synthetic LLM, without API
// credentials, song generation, or a frontend test framework.
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import ts from "typescript";

function loadSource(path, resolveImport = () => ({}), globals = {}, sourceOverride) {
  const source = sourceOverride ?? fs.readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
  const { outputText } = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  });
  const module = { exports: {} };
  vm.runInNewContext(outputText, {
    module, exports: module.exports, require: resolveImport, $state: (value) => value,
    ...globals,
  }, { filename: path });
  return module.exports;
}

const skill = loadSource("src/lib/utils/yue2Skill.ts");
const context = { style: "English, acoustic folk, 90 BPM", lyrics: "A hopeful song about going home", maxDuration: 30, language: "en-AU" };
const topicRequest = skill.musicWritingRequest("lyrics", { ...context, brief: "An astronaut missing her family, in French" });
assert.equal(JSON.parse(topicRequest.prompt).topic_or_story, "An astronaut missing her family, in French");
assert.equal(Object.hasOwn(JSON.parse(topicRequest.prompt), "lyrics_or_idea"), false);
assert.ok(!topicRequest.prompt.includes(context.lyrics), "Fresh requests must not include existing lyrics");
assert.ok(topicRequest.system.includes("Write a fresh, original"));
const editingRequest = skill.musicWritingRequest("lyrics", { ...context, brief: "Make the chorus more hopeful", useExistingLyrics: true });
assert.equal(JSON.parse(editingRequest.prompt).lyrics_or_idea, context.lyrics);
assert.equal(JSON.parse(editingRequest.prompt).topic_or_story, "Make the chorus more hopeful");
assert.ok(editingRequest.system.includes("Revise the supplied lyrics_or_idea"));
assert.ok(editingRequest.system.includes("30 seconds"));
for (const useExistingLyrics of [undefined, false]) {
  const request = skill.musicWritingRequest("lyrics", { ...context, useExistingLyrics });
  assert.equal(Object.hasOwn(JSON.parse(request.prompt), "lyrics_or_idea"), false);
}
const emptyDraftRequest = skill.musicWritingRequest("lyrics", { ...context, lyrics: "  ", useExistingLyrics: true });
assert.equal(Object.hasOwn(JSON.parse(emptyDraftRequest.prompt), "lyrics_or_idea"), false);
assert.ok(emptyDraftRequest.system.includes("Write a fresh, original"));
const verse = "[Verse]\nMorning light across the sea\nBrings the road back home to me";

for (const bad of [NaN, Infinity, -1, 0, 361, undefined]) {
  assert.throws(() => skill.musicLyricBudget(bad), /invalid_music_duration/);
}
let previous = 0;
for (let seconds = 1; seconds <= 360; seconds++) {
  const budget = skill.musicLyricBudget(seconds);
  assert.ok(budget.maxUnits >= previous);
  assert.ok(budget.targetUnits > 0 && budget.targetUnits <= budget.maxUnits);
  assert.ok(budget.vocalSeconds < seconds, "Leave space for instruments and the ending");
  const request = skill.musicWritingRequest("lyrics", { ...context, maxDuration: seconds });
  assert.equal(JSON.parse(request.prompt).maximum_seconds, seconds);
  assert.ok(request.system.includes(`${seconds} seconds`));
  assert.ok(request.system.includes(`${budget.maxUnits} is the ceiling`));
  assert.ok(request.maxTokens >= 64 && request.maxTokens <= 4096);
  previous = budget.maxUnits;
}
assert.ok(skill.musicLyricBudget(30).maxUnits < skill.musicLyricBudget(120).maxUnits);
assert.equal(skill.musicWritingProblems(verse, "lyrics", 30).length, 0);
assert.ok(skill.musicWritingProblems(verse, "lyrics", 5).length > 0);
assert.equal(skill.musicLyricUnits("[Verse 1]\n月光照我回家\n\n[Chorus]\nCome home"), 5);
assert.ok(skill.musicWritingProblems(`[Verse]\n${"月".repeat(100)}`, "lyrics", 30).length > 0);
assert.ok(skill.musicWritingProblems(`${verse}\n\n${verse}\n\n${verse}`, "lyrics", 30).length > 0, "Count repeated sections in full");
for (const invalid of ["", "[Verse]", ' {"lyrics":"hello"}', "Here are your lyrics:\n" + verse, "[Verse]\n[Whisper softly]\nHome", "<think>Write a song</think>\n" + verse]) {
  assert.ok(skill.musicWritingProblems(invalid, "lyrics", 30).length > 0, invalid);
}
assert.equal(skill.cleanMusicWritingResponse(`\n\u0060\u0060\u0060text\r\n${verse}\r\n\u0060\u0060\u0060\n`), verse);
assert.ok(skill.musicWritingProblems(skill.cleanMusicWritingResponse("```text\n" + verse), "lyrics", 30).length > 0);
assert.equal(skill.musicWritingProblems(context.style, "style", 30).length, 0);
for (const invalid of [verse, "- folk\n- piano", "a".repeat(1201), '{"style":"folk"}']) {
  assert.ok(skill.musicWritingProblems(invalid, "style", 30).length > 0);
}
const styleRequest = skill.musicWritingRequest("style", context);
assert.equal(JSON.parse(styleRequest.prompt).lyrics_or_idea, context.lyrics);
assert.ok(styleRequest.system.includes("do not rewrite or quote them"));
assert.ok(styleRequest.system.includes("30 seconds"));
console.log("PASS: duration limits, multilingual/repeated lyric budgets, field separation and output validation");

let replies = [];
let calls = [];
let unlistens = 0;
let listenError = false;
const { promptAssistant } = loadSource("src/lib/stores/promptAssistant.svelte.ts", (name) => {
  if (name === "../utils/yue2Skill.js") return skill;
  if (name === "../utils/api.js") return {
    callExternalLlm: async (...args) => {
      calls.push(args);
      const reply = replies.shift();
      if (reply instanceof Error) throw reply;
      return await reply;
    },
  };
  if (name === "../utils/ipc.js") return {
    ipcListen: async () => {
      if (listenError) throw new Error("SSE unavailable");
      return () => { unlistens++; };
    },
  };
  // Other prompt formats are imported by the shared store but never used here.
  assert.match(name, /^\.\.\/utils\/(h3|nai)/);
  return {};
});

replies = [verse];
assert.equal(await promptAssistant.writeForMusic("lyrics", context), verse);
assert.equal(calls.length, 1);
assert.equal(calls[0].length, 3, "No images or extra data sent to the text assistant");
assert.ok(calls[0][0].includes("YuE2"));
assert.equal(Object.hasOwn(JSON.parse(calls[0][1]), "lyrics_or_idea"), false);
assert.equal(promptAssistant.isGenerating, false);
assert.equal(unlistens, 1);

calls = [];
replies = ["[Verse]\n" + "home ".repeat(100), verse];
assert.equal(await promptAssistant.writeForMusic("lyrics", context), verse);
assert.equal(calls.length, 2);
assert.ok(calls[1][1].startsWith(calls[0][1]), "Retry retains the original idea, style and duration");
assert.ok(calls[1][1].includes("Shorten the lyrics"));
assert.ok(calls.every((call) => !call[1].includes(context.lyrics)), "Neither fresh attempt sends the old draft");

calls = [];
replies = ["", verse];
assert.equal(await promptAssistant.writeForMusic("lyrics", { ...context, useExistingLyrics: true }), verse);
assert.equal(JSON.parse(calls[0][1]).lyrics_or_idea, context.lyrics);
assert.ok(calls[1][1].startsWith(calls[0][1]), "Editing retry retains the opted-in draft");

calls = [];
replies = ["", "[Verse]"];
await assert.rejects(promptAssistant.writeForMusic("lyrics", context), /invalid_music_writing/);
assert.equal(calls.length, 2, "At most one correction request");
assert.equal(promptAssistant.isGenerating, false);
replies = [new Error("provider offline")];
await assert.rejects(promptAssistant.writeForMusic("style", context), /provider offline/);
assert.equal(promptAssistant.isGenerating, false);
listenError = true;
await assert.rejects(promptAssistant.writeForMusic("style", context), /SSE unavailable/);
assert.equal(promptAssistant.isGenerating, false);
listenError = false;

let complete;
replies = [new Promise((resolve) => { complete = resolve; })];
const pending = promptAssistant.writeForMusic("style", context);
await assert.rejects(promptAssistant.writeForMusic("lyrics", context), /busy_generation/);
complete(context.style);
assert.equal(await pending, context.style);
assert.equal(promptAssistant.isGenerating, false);
console.log("PASS: actual store LLM dispatch, single retry, original context, errors, listener cleanup and concurrency guard");

// Run the component's real event handlers with a dialog/LLM boundary stub.
// These checks do not claim browser focus, rendering or interactive coverage.
const pagePath = "src/lib/components/music/MusicPage.svelte";
const pageSource = fs.readFileSync(new URL(`../${pagePath}`, import.meta.url), "utf8");
const script = pageSource.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1];
const pageMusic = { params: { style: context.style, lyrics: "Original draft", max_duration: 90, abc: "" }, busy: false, saveSettings() {} };
const dialog = { open: false, showModal() { this.open = true; }, close() { this.open = false; } };
let pageCalls = [];
let resolveDraft;
const pageAssistant = {
  isGenerating: false, isAvailable: true, refreshStatus: async () => {},
  writeForMusic: async (...args) => { pageCalls.push(args); return await new Promise(resolve => { resolveDraft = resolve; }); },
};
const { events } = loadSource(pagePath, (name) => {
  if (name === "svelte") return { onDestroy() {}, onMount() {}, tick: async () => {}, untrack: fn => fn() };
  if (name.includes("/music.svelte")) return { music: pageMusic };
  if (name.includes("/promptAssistant.svelte")) return { promptAssistant: pageAssistant };
  if (name.includes("/locale.svelte")) return { locale: { t: key => key, intlTag: "en-AU" } };
  if (name.includes("/llmError")) return { mapLlmError: String };
  return {};
}, { $props: () => ({}), $derived: value => value, $effect: () => {} },
`${script}\nexport const events = { openLyricsDialog, closeLyricsDialog, writeMusic, undoWriting, setDialog(value) { lyricsDialog = value; }, setUseExistingLyrics(value) { useExistingLyrics = value; }, getUseExistingLyrics() { return useExistingLyrics; }, getError() { return writingError; } };`);
events.setDialog(dialog);
events.openLyricsDialog();
assert.equal(dialog.open, true);
assert.equal(pageCalls.length, 0, "Opening the topic modal must not spend an LLM request");
assert.equal(pageMusic.params.lyrics, "Original draft");
let pagePending = events.writeMusic("lyrics", "An astronaut missing her family");
await new Promise(resolve => setImmediate(resolve));
assert.equal(pageCalls[0][1].brief, "An astronaut missing her family");
assert.equal(pageCalls[0][1].style, context.style);
assert.equal(pageCalls[0][1].maxDuration, 90);
assert.equal(pageCalls[0][1].lyrics, "Original draft");
assert.equal(pageCalls[0][1].useExistingLyrics, false);
assert.equal(Object.hasOwn(JSON.parse(skill.musicWritingRequest(...pageCalls[0]).prompt), "lyrics_or_idea"), false);
resolveDraft(verse);
await pagePending;
assert.equal(dialog.open, false);
assert.equal(pageMusic.params.lyrics, verse);
events.undoWriting("lyrics");
assert.equal(pageMusic.params.lyrics, "Original draft");
events.openLyricsDialog();
events.setUseExistingLyrics(true);
pagePending = events.writeMusic("lyrics", "Make the chorus more hopeful");
await new Promise(resolve => setImmediate(resolve));
assert.equal(pageCalls.at(-1)[1].useExistingLyrics, true);
assert.equal(JSON.parse(skill.musicWritingRequest(...pageCalls.at(-1)).prompt).lyrics_or_idea, "Original draft");
resolveDraft(verse);
await pagePending;
events.undoWriting("lyrics");
events.openLyricsDialog();
assert.equal(events.getUseExistingLyrics(), false, "Reopening the popup defaults to a fresh draft");
pagePending = events.writeMusic("lyrics", "A different topic");
await new Promise(resolve => setImmediate(resolve));
events.closeLyricsDialog();
resolveDraft("Must not replace the cancelled draft");
await pagePending;
assert.equal(pageMusic.params.lyrics, "Original draft");
events.openLyricsDialog();
pagePending = events.writeMusic("lyrics", "A third topic");
await new Promise(resolve => setImmediate(resolve));
pageMusic.params.max_duration = 60;
resolveDraft(verse);
await pagePending;
assert.equal(pageMusic.params.lyrics, "Original draft");
assert.equal(events.getError(), "music.assistant_changed");
console.log("PASS: popup opens without generating, draft context is opt-in and resets, topic/style/duration reach LLM, success closes, Undo, cancel and changed settings preserve the draft");
