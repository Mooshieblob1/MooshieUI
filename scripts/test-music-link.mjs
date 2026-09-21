// Synthetic IPC exercises the real component handlers without downloading media.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

function load(source, imports = {}, globals = {}) {
  const module = { exports: {} };
  const state = value => value; state.raw = state;
  const code = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
  vm.runInNewContext(code, { module, exports: module.exports, require(name) { assert.ok(name in imports, name); return imports[name]; },
    $state: state, $props: () => ({}), $derived: value => value, TextEncoder, URL, File, Uint8Array, atob, console, ...globals });
  return module.exports;
}
const helper = load(fs.readFileSync('src/lib/utils/musicLink.ts', 'utf8'));
assert.equal(helper.needsSongMatch('https://open.spotify.com/track/abc'), true);
assert.equal(helper.needsSongMatch('https://www.youtube.com/watch?v=abc'), false);
assert.equal(helper.needsSongMatch('https://open.spotify.com.evil.test/track/abc'), false);
const mp3 = helper.importedSongFile({ status: 'completed', audio_base64: btoa('audio'), title: '../song:name' });
assert.equal(mp3.type, 'audio/mpeg'); assert.equal(mp3.name, '.._song_name.mp3'); assert.equal(await mp3.text(), 'audio');
assert.throws(() => helper.importedSongFile({ status: 'error' }));

let user = 'alice', destroyed, effect, timer, nextStart, rejectStart, nextStatus, sequence = 0;
let capabilities = { available: false, status: 'downloading', tool: 'yt-dlp', completed: 0, total: 4, percent: 25, error: null };
let setupRetries = 0;
const revoked = [], cancelled = [];
const music = { params: { max_duration: 240, abc: '', planning: 'melody', cover: true }, busy: false, reviewSource: null, saveSettings() {} };
const script = fs.readFileSync('src/lib/components/music/MusicCover.svelte', 'utf8').match(/<script lang="ts">([\s\S]*?)<\/script>/)[1];
const events = load(`${script}\nexport const test = { importLink, chooseSource, cancelLink, clearSource, refreshLinkCapabilities, get capabilities() { return linkCapabilities; }, get source() { return source; }, get url() { return sourceUrl; }, get busy() { return linkBusy; }, get info() { return linkInfo; }, get error() { return linkError; }, set link(value) { songLink = value; } };`, {
  svelte: { onMount() {}, onDestroy(fn) { destroyed = fn; }, untrack: fn => fn() },
  '../../stores/music.svelte.js': { music },
  '../../stores/musicCover.svelte.js': { musicCover: { busy: false } },
  '../../stores/locale.svelte.js': { locale: { t: key => key } },
  '../../utils/musicCover.js': { coverScoreError: () => null },
  '../../utils/ipc.js': { getAuthUser: () => user },
  '../../utils/musicScore.js': {},
  '../../utils/musicLink.js': helper,
  '../../utils/api.js': {
    getMusicLinkCapabilities: async () => capabilities,
    prepareMusicLinkTools: async () => { setupRetries++; return capabilities; },
    importMusicLink: () => new Promise((resolve, reject) => { nextStart = resolve; rejectStart = reject; }),
    getMusicLinkImport: (id, cancel) => cancel ? (cancelled.push(id), Promise.resolve({ status: 'cancelled' })) : new Promise(resolve => { nextStatus = resolve; }),
  },
}, {
  $effect: fn => { effect = fn; },
  setTimeout: fn => { timer = fn; return 1; }, clearTimeout: () => { timer = null; },
  URL: { createObjectURL: () => `blob:${++sequence}`, revokeObjectURL: url => revoked.push(url) },
}).test;
const flush = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };
const result = { status: 'completed', audio_base64: btoa('audio'), title: 'Song', source_url: 'https://youtube.com/watch?v=dQw4w9WgXcQ', matched: true };
events.link = 'https://youtube.com/watch?v=dQw4w9WgXcQ';
await events.importLink(); assert.equal(nextStart, undefined, 'Import waits for all prerequisites');
await events.refreshLinkCapabilities(); assert.ok(timer, 'Automatic setup progress is polled');
assert.equal(events.capabilities.percent, 25);
capabilities = { ...capabilities, status: 'error', error: 'Connection failed' };
await events.refreshLinkCapabilities(); assert.equal(timer, null, 'Final errors stop polling and allow a manual retry');
capabilities = { ...capabilities, status: 'checking', error: null };
await events.refreshLinkCapabilities(true); assert.equal(setupRetries, 1); assert.ok(timer);
capabilities = { ...capabilities, status: 'ready', available: true };
await events.refreshLinkCapabilities(); assert.equal(timer, null, 'No setup polling remains after readiness');
const first = events.importLink(); nextStart('one'); await flush(); nextStatus(result); await first;
assert.equal(events.source.name, 'Song.mp3'); assert.equal(events.busy, false);
assert.equal(music.reviewSource, events.source); assert.equal(events.info.audio_base64, undefined);
const oldUrl = events.url;
const replacement = events.importLink();
assert.equal(events.source, null); assert.equal(music.reviewSource, null); assert.ok(revoked.includes(oldUrl));
// Manual selection wins even while start IPC is in flight.
const manual = new File(['manual'], 'local.wav'); events.chooseSource(manual);
nextStart('two'); await replacement; assert.ok(cancelled.includes('two')); assert.equal(events.source, manual);

const inFlight = events.importLink(); nextStart('three'); await flush();
events.chooseSource(manual); nextStatus(result); await inFlight;
assert.ok(cancelled.includes('three')); assert.equal(events.source, manual, 'A stale result cannot replace a manual upload');

const cancelledImport = events.importLink(); nextStart('four'); await flush();
nextStatus({ status: 'running' }); await cancelledImport; assert.ok(timer);
events.cancelLink(); assert.ok(cancelled.includes('four')); assert.equal(timer, null); assert.equal(events.busy, false);

const failed = events.importLink(); nextStart('five'); await flush(); nextStatus({ status: 'error', error: 'Unavailable' }); await failed;
assert.equal(events.source, null); assert.equal(events.error, 'Unavailable'); assert.equal(events.busy, false);

const account = events.importLink(); nextStart('six'); await flush(); user = 'bob';
nextStatus(result); await account; assert.equal(events.source, null, 'Previous account audio is ignored');
assert.equal(events.busy, false, 'Account changes cannot strand the importer in a busy state');
const accountFailure = events.importLink(); user = 'carol'; rejectStart(new Error('Old account detail')); await accountFailure;
assert.equal(events.busy, false); assert.equal(events.error, '', 'An old account error is not shown to the new account');
events.chooseSource(manual); effect(); assert.equal(events.source, manual);
music.selectedResult = { prompt_id: 'another-song' }; effect();
assert.equal(events.source, null); assert.equal(music.reviewSource, null, 'Selecting another song drops the shared review source');
events.chooseSource(manual); music.reviewSource = null; effect(); assert.equal(events.source, null, 'An account reset releases the local preview');

const closing = events.importLink();
capabilities = { ...capabilities, available: false, status: 'retrying' };
await events.refreshLinkCapabilities(); assert.ok(timer);
destroyed(); assert.equal(timer, null, 'Closing cancels setup polling'); nextStart('seven'); await closing;
assert.ok(cancelled.includes('seven')); assert.equal(events.source, null); assert.equal(music.reviewSource, null);
console.log('PASS: automatic setup progress/retry/readiness/teardown, link conversion, single Blob retention, replacement, late-start and late-result races, cancel, error, account/song changes and closing cleanup');
