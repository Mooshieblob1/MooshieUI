// Run: node scripts/test-music-cover.mjs. Synthetic IPC; no model downloads.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

function load(path, imports = {}, globals = {}, sourceOverride) {
  const state = value => value;
  state.raw = state;
  const module = { exports: {} };
  const code = ts.transpileModule(sourceOverride ?? fs.readFileSync(path, 'utf8'), { compilerOptions: {
    target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS,
  } }).outputText;
  vm.runInNewContext(code, { module, exports: module.exports, require: name => {
    assert.ok(name in imports, `Unexpected import ${name}`); return imports[name];
  }, $state: state, TextEncoder, console, crypto, localStorage: { getItem() { return null; }, setItem() {}, removeItem() {} }, ...globals });
  return module.exports;
}

const cover = load('src/lib/utils/musicCover.ts');
const { COVER_EXAMPLE, melodyOnlyAbc, coverScoreError } = cover;
assert.equal(coverScoreError(COVER_EXAMPLE), null);
assert.equal(coverScoreError(''), 'cover_score_required');
for (const abc of ['folk melody', 'X:1\nK:C\n% CDEF', 'X:1\nK:C\n"Am"', 'X:1\nK:C\n[V:Vocal] z8', COVER_EXAMPLE + 'é'.repeat(70000)]) {
  assert.equal(coverScoreError(abc), 'cover_score_invalid');
}
const harmony = 'X:1\r\nT:An "original" song\r\nV:Vocal name="Voice"\r\nK:C\r\n"Am"C2 "G7/B"D2 [V:Ins name="Piano"] "^softly"E2 % keep "C" comment';
const melody = melodyOnlyAbc(harmony);
assert.equal(melody, 'X:1\nT:An "original" song\nV:Vocal name="Voice"\nK:C\nC2 D2 [V:Ins name="Piano"] "^softly"E2 % keep "C" comment');
assert.equal(coverScoreError(harmony), 'cover_chords');
assert.equal(coverScoreError(melody), null);
assert.equal(melodyOnlyAbc(melody), melody);
assert.equal(melodyOnlyAbc('X:1\nK:C\n[CEG]2 "F#m7"F2'), 'X:1\nK:C\n[CEG]2 F2');
console.log('PASS: chord removal preserves notes, headers, inline voices, annotations and comments; score size/structure checks');

let user = null;
let submitted;
let timer;
const { music } = load('src/lib/stores/music.svelte.ts', {
  '../utils/api.js': { generateMusic: async params => { submitted = { ...params }; return { prompt_id: 'cover', worker_id: 0, seed: '42' }; } },
  '../utils/ipc.js': { getAuthUser: () => user },
  './locale.svelte.js': { locale: { t: key => key } },
  '../utils/musicCover.js': cover,
  '../utils/musicScore.js': load('src/lib/utils/musicScore.ts'),
  '../utils/musicSettings.js': load('src/lib/utils/musicSettings.ts'),
  '../utils/musicAudio.js': {}, '../utils/musicLibrary.js': {},
}, { setTimeout: fn => { timer = fn; return 1; }, clearTimeout: () => {}, localStorage: { setItem() {} } });
music.capabilities = { missing_nodes: [], checkpoints: ['yue2'] };
music.params.checkpoint = 'yue2'; music.params.seed = '42';
music.params.abc = COVER_EXAMPLE; music.setCover(true);
assert.equal(music.params.planning, 'melody');
await music.generate();
assert.equal(submitted, undefined, 'Unreviewed scores do not submit');
music.reviewedCoverAbc = COVER_EXAMPLE;
music.params.abc += '\nC4';
assert.equal(music.coverReady, false, 'Editing a reviewed score invalidates review');
music.params.abc = harmony; music.reviewedCoverAbc = harmony;
await music.generate(); assert.equal(submitted, undefined, 'Harmony cannot bypass cover validation');
music.params.abc = COVER_EXAMPLE; music.reviewedCoverAbc = COVER_EXAMPLE;
music.params.planning = 'full';
await music.generate();
assert.equal(submitted.planning, 'full'); assert.equal(submitted.abc, COVER_EXAMPLE); assert.equal(submitted.cover, true);
music.phase = 'idle'; music.setCover(false); music.params.abc = '';
assert.equal(music.coverReady, true, 'Original generation still permits an empty score');
console.log('PASS: review gate, stale score edits, selected cover fidelity and original-song compatibility');

let resolveStatus;
let calls = [];
let latestJob;
const { musicCover } = load('src/lib/stores/musicCover.svelte.ts', {
  '../utils/api.js': {
    getCoverCapabilities: async () => ({ configured: true, device: 'cpu', latest_job: latestJob }),
    getMusicCapabilities: async () => ({ missing_nodes: [], checkpoints: [], cover_missing_nodes: ['SheetSage2AudioToABC'], audio_encoders: [] }),
    transcribeMusicCover: async (audio, filename) => { calls.push({ audio, filename }); return 'job'; },
    getCoverTranscription: async (id, cancel) => cancel ? (calls.push({ cancel }), { status: 'running' }) : new Promise(resolve => { resolveStatus = resolve; }),
  },
  '../utils/musicAudio.js': { blobBase64: async () => 'encoded' },
  '../utils/ipc.js': { getAuthUser: () => user },
  './locale.svelte.js': { locale: { t: key => key } },
}, { setTimeout: fn => { timer = fn; return 1; }, clearTimeout: () => {} });
const flush = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };
await musicCover.refresh();
await musicCover.transcribe({ size: 0, name: 'empty.wav' });
assert.equal(calls.length, 0);
const task = musicCover.transcribe({ size: 30, name: 'source.wav' });
await flush();
assert.equal(musicCover.busy, true);
resolveStatus({ status: 'completed', abc: COVER_EXAMPLE, warnings: ['Review timing'] });
await task;
assert.equal(musicCover.candidate, COVER_EXAMPLE);
assert.equal(musicCover.busy, false);
const again = musicCover.transcribe({ size: 30, name: 'second.wav' });
await flush();
await musicCover.cancel();
assert.equal(calls.at(-1).cancel, true);
resolveStatus({ status: 'cancelled' }); await again;
assert.equal(musicCover.candidate, COVER_EXAMPLE, 'Cancellation preserves the previous draft');
const stale = musicCover.transcribe({ size: 30, name: 'private.wav' }); await flush();
user = 'other'; await musicCover.refresh();
resolveStatus({ status: 'completed', abc: 'private score' }); await stale;
assert.equal(musicCover.candidate, '', 'A previous account cannot populate a new account with a late result');
latestJob = 'recovered-job';
await musicCover.refresh(); await flush();
assert.equal(musicCover.jobId, latestJob);
assert.equal(musicCover.busy, true, 'Refresh resumes the caller\'s latest job after reload');
resolveStatus({ status: 'completed', abc: COVER_EXAMPLE, warnings: [] }); await flush();
assert.equal(musicCover.candidate, COVER_EXAMPLE);
console.log('PASS: transcription upload, draft retrieval, cancellation, size limits and stale account isolation');

// Run the real source-import/metadata handlers; the browser decoder is the boundary.
for (const seconds of [NaN, Infinity, -1, 0, 360.001]) assert.equal(cover.coverSourceMaxDuration(seconds), null);
for (const [seconds, expected] of [[0.01, 1], [89.6, 90], [120, 120], [359.99, 360], [360, 360]]) {
  assert.equal(cover.coverSourceMaxDuration(seconds), expected);
}
const componentPath = 'src/lib/components/music/MusicCover.svelte';
const component = fs.readFileSync(componentPath, 'utf8');
const componentScript = component.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1];
let sourceUser = 'source-owner', destroyed, urlSequence = 0, saved = 0;
const revoked = [];
const pageMusic = { params: { abc: '', planning: 'melody', max_duration: 240 }, busy: false, saveSettings() { saved++; } };
const { sourceEvents } = load(componentPath, {
  svelte: { onMount() {}, onDestroy(fn) { destroyed = fn; } },
  '../../stores/music.svelte.js': { music: pageMusic },
  '../../stores/musicCover.svelte.js': { musicCover: { busy: false } },
  '../../stores/locale.svelte.js': { locale: { t: key => key } },
  '../../utils/musicCover.js': cover,
  '../../utils/musicScore.js': {},
  '../../utils/ipc.js': { getAuthUser: () => sourceUser },
}, {
  $props: () => ({}), $derived: value => value,
  URL: { createObjectURL: () => `blob:source-${++urlSequence}`, revokeObjectURL: url => revoked.push(url) },
}, `${componentScript}\nexport const sourceEvents = { chooseSource, readSourceDuration, useSourceLength, sourceDurationFailed, get url() { return sourceUrl; }, get length() { return sourceLength; }, get tooLong() { return sourceTooLong; }, get error() { return durationError; } };`);
const file = { size: 100, name: 'source.wav' };
const media = (duration, url = sourceEvents.url) => ({ duration, getAttribute: () => url });
sourceEvents.chooseSource(file);
const firstUrl = sourceEvents.url;
sourceEvents.readSourceDuration(media(120.35));
assert.equal(pageMusic.params.max_duration, 121);
assert.equal(sourceEvents.length, 121);
assert.equal(saved, 1);
pageMusic.params.max_duration = 180;
sourceEvents.readSourceDuration(media(120.35));
assert.equal(pageMusic.params.max_duration, 180, 'Repeated metadata must preserve a manual extension');
sourceEvents.useSourceLength();
assert.equal(pageMusic.params.max_duration, 121);
sourceEvents.chooseSource(file);
sourceEvents.readSourceDuration(media(20, firstUrl));
assert.equal(sourceEvents.length, null, 'Previous source metadata is ignored');
pageMusic.params.max_duration = 150;
sourceEvents.readSourceDuration(media(60.2));
assert.equal(pageMusic.params.max_duration, 150, 'An edit while metadata loads wins');
assert.equal(sourceEvents.length, 61);
assert.ok(revoked.includes(firstUrl));
sourceEvents.chooseSource(file);
sourceEvents.readSourceDuration(media(361));
assert.equal(sourceEvents.tooLong, true);
assert.equal(pageMusic.params.max_duration, 150, 'Oversized sources are not silently cropped');
sourceEvents.chooseSource(file);
sourceEvents.readSourceDuration(media(Infinity));
assert.equal(sourceEvents.length, null);
assert.equal(sourceEvents.error, 'music.cover_duration_unavailable');
sourceEvents.readSourceDuration(media(44.4));
assert.equal(pageMusic.params.max_duration, 45, 'A later finite duration can recover an unknown duration');
assert.equal(sourceEvents.error, '');
sourceEvents.chooseSource(file);
sourceEvents.sourceDurationFailed(media(NaN));
assert.equal(pageMusic.params.max_duration, 45, 'Decoder failure leaves the editable maximum intact');
assert.equal(sourceEvents.error, 'music.cover_duration_unavailable');
sourceEvents.chooseSource(file);
sourceUser = 'next-owner';
sourceEvents.readSourceDuration(media(90));
assert.equal(sourceEvents.length, null, 'A late previous-account result cannot change the new draft');
sourceEvents.chooseSource(file);
pageMusic.busy = true;
sourceEvents.readSourceDuration(media(90));
assert.equal(pageMusic.params.max_duration, 45, 'Metadata never edits a running generation');
pageMusic.busy = false;
sourceEvents.chooseSource(file);
destroyed();
sourceEvents.readSourceDuration(media(90));
assert.equal(pageMusic.params.max_duration, 45, 'Unmounted imports cannot change settings');
assert.match(component, /onloadedmetadata=\{event => readSourceDuration\(event.currentTarget\)\}/);
assert.match(component, /ondurationchange=\{event => readSourceDuration\(event.currentTarget\)\}/);
console.log('PASS: source-length defaults, fractional rounding, manual extension/reset, stale metadata/account, decoder failure, 360-second limit and generation guards');
