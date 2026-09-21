// Run real prompt/validation and component handlers against synthetic IPC/LLM.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

function load(source, imports = {}, globals = {}) {
  const module = { exports: {} };
  const code = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
  vm.runInNewContext(code, { module, exports: module.exports, require: name => imports[name] ?? {}, $state: value => value, $derived: value => value, ...globals });
  return module.exports;
}
const writing = load(fs.readFileSync('src/lib/utils/yue2Skill.ts', 'utf8'));
const helper = load(fs.readFileSync('src/lib/utils/musicReference.ts', 'utf8'), { './yue2Skill.js': writing });
const song = { id: 1, title: 'Reference (Live)', artist: 'Artist', album: 'Live Album', genre: 'Pop', year: '2020', url: 'https://music.apple.com/us/song/1', duration_seconds: 120 };
const reference = { song, sources: [{ title: 'Catalog', url: song.url, text: 'Public catalog information' }] };
const context = { style: 'old style', lyrics: 'original lyrics', maxDuration: 40, language: 'en', planning: 'off' };
const request = helper.referenceStyleRequest(reference, context);
const payload = JSON.parse(request.prompt);
assert.equal(payload.reference.song.title, 'Reference (Live)');
assert.equal(payload.target.style, '', 'Old style must not contaminate a reference replacement');
assert.equal(payload.target.lyrics_or_idea, 'original lyrics');
assert.equal(payload.target.maximum_seconds, 40);
assert.ok(request.system.includes('untrusted reference data'));
assert.ok(request.system.includes('different live/remix/cover version'));
assert.ok(request.system.includes('No audio has been supplied'));
for (const lyrics of ['', ' \n\t ']) {
  const instrumental = helper.referenceStyleRequest(reference, { ...context, lyrics });
  assert.equal(JSON.parse(instrumental.prompt).target.generation.instrumental, true);
  assert.ok(instrumental.system.includes('purely instrumental music with no vocals'));
  assert.ok(instrumental.system.includes('Reassign any reference vocal melody to a suitable instrument'));
  assert.ok(!instrumental.system.includes('vocal delivery and production texture'));
}
const draft = { status: 'ok', style: 'English upbeat pop with warm bass, a steady groove and an immediate hook, followed by a compact ending.', estimates: ['Instrumentation and arrangement are suggestions, not audio analysis.'] };
assert.equal(helper.validateReferenceStyle(JSON.stringify(draft), 40).style, draft.style);
assert.throws(() => helper.validateReferenceStyle('{"status":"unknown"}', 40), /music_reference_unknown/);
for (const value of [null, {}, { ...draft, estimates: [] }, { ...draft, estimates: ['https://invented.test'] }, { ...draft, style: 'x'.repeat(1201) }, { ...draft, style: '[Verse]\nLyrics' }]) {
  assert.throws(() => helper.validateReferenceStyle(JSON.stringify(value), 40), /invalid_music_reference/);
}

let account = 'alice', destroy, effect, finishSearch, finishContext, finishLlm, saveCount = 0, llmCount = 0;
const music = { busy: false, params: { style: 'old style', lyrics: 'original lyrics', max_duration: 40, planning: 'off', abc: '' }, saveSettings() { saveCount++; } };
const assistant = { isGenerating: false, isAvailable: true, async refreshStatus() {}, styleFromReference() { llmCount++; return new Promise(resolve => { finishLlm = resolve; }); } };
const script = fs.readFileSync('src/lib/components/music/MusicStyleReference.svelte', 'utf8').match(/<script lang="ts">([\s\S]*?)<\/script>/)[1];
const events = load(script + '\nexport const test = { search, describe, apply, undoStyle, reset, enable() {enabled=true; title="Reference";}, select() {selected="1";}, get draft() {return draft;}, get phase() {return phase;}, get error() {return error;}, get matches() {return matches;}, get reference() {return reference;} };', {
  svelte: { onDestroy(fn) { destroy = fn; }, untrack: fn => fn() },
  '../../stores/music.svelte.js': { music },
  '../../stores/promptAssistant.svelte.js': { promptAssistant: assistant },
  '../../stores/locale.svelte.js': { locale: { t: key => key, intlTag: 'en' } },
  '../../utils/ipc.js': { getAuthUser: () => account },
  '../../utils/api.js': { searchMusicReference: () => new Promise(resolve => { finishSearch = resolve; }), getMusicReference: () => new Promise(resolve => { finishContext = resolve; }) },
  '../../utils/llmError.js': { mapLlmError: String },
}, { $effect: fn => { effect = fn; } }).test;
const flush = async () => { for (let i=0; i<10; i++) await Promise.resolve(); };
async function choose() { events.enable(); const task = events.search(); finishSearch([song, { ...song, id: 2 }]); await task; events.select(); }
async function preview() { const task = events.describe(); await flush(); finishContext(reference); await flush(); finishLlm(draft); await task; }
await choose(); await preview();
assert.equal(music.params.style, 'old style', 'Preview never applies itself');
events.apply(); assert.equal(music.params.style, draft.style); assert.equal(saveCount, 1);
events.undoStyle(); assert.equal(music.params.style, 'old style'); assert.equal(saveCount, 2);
await preview(); music.params.style = 'manual edit'; events.apply();
assert.equal(music.params.style, 'manual edit'); assert.equal(events.error, 'music.assistant_changed');
events.reset(); await choose();
const stale = events.describe(); await flush(); music.params.lyrics = 'changed lyrics'; finishContext(reference); await stale;
assert.equal(events.draft, null); assert.equal(llmCount, 2, 'Edits during lookup prevent an LLM call');
assert.equal(events.phase, null);
await preview(); events.apply(); music.params.style = 'after apply edit'; events.undoStyle();
assert.equal(music.params.style, 'after apply edit', 'Undo preserves subsequent edits');
const staleQuery = events.search(); events.reset(); finishSearch([song]); await staleQuery;
assert.equal(events.matches.length, 0);
await choose(); const oldAccount = events.describe(); await flush(); account = 'bob'; finishContext(reference); await oldAccount;
assert.equal(events.draft, null); effect(); assert.equal(events.matches.length, 0); assert.equal(events.reference, null);
await choose(); const closing = events.describe(); await flush(); destroy(); finishContext(reference); await closing;
assert.equal(events.draft, null);
console.log('PASS: recording identity, source contract, unknown/invalid output, preview/apply/undo, edits during lookup, late search, account switch and teardown');
