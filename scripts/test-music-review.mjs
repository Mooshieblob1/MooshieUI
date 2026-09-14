// Real bounded analysis and LLM contract, with synthetic recognition results.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
const load = (path, imports = {}) => {
  const module = { exports: {} };
  const compiled = ts.transpileModule(fs.readFileSync(path, 'utf8'), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
  vm.runInNewContext(compiled, { module, exports: module.exports, require: key => { assert.ok(key in imports, key); return imports[key]; }, TextEncoder });
  return module.exports;
};
const review = load('src/lib/utils/musicReview.ts', { './musicScore.js': load('src/lib/utils/musicScore.ts') });
const lyrics = '[Verse]\nMorning light across the sea\nBring the road back home to me\nTake my hand and lead me on\nWe will find a brighter dawn';
const transcript = (text = lyrics, times = [1, 8, 16, 24]) => {
  const words = text.split('\n').filter(line => line && !line.startsWith('[')).flatMap((line, i) => line.split(' ').map((text, j) => ({ text, start: times[i] + j * 0.25, end: times[i] + j * 0.25 + 0.2 })));
  return { text: words.map(w => w.text).join(' '), language: 'en', duration: 40, words, audio_sha256: 'a'.repeat(64) };
};
const source = transcript();
const same = review.analyzeMusicReview(lyrics, source, source, true);
assert.equal(same.coverCoverage, 1); assert.equal(same.offset, 0); assert.equal(same.drift, 0);
const shifted = review.analyzeMusicReview(lyrics, transcript(lyrics, [3, 10, 18, 26]), source, true);
assert.equal(shifted.offset, 2); assert.equal(shifted.drift, 0);
const drift = review.analyzeMusicReview(lyrics, transcript(lyrics, [1, 9, 18, 28]), source, true);
assert.equal(drift.drift, 4); assert.equal(drift.timingLines, 4);
const changed = review.analyzeMusicReview(lyrics, source, source, false);
assert.equal(changed.offset, null); assert.equal(changed.drift, null); assert.equal(changed.sourceCoverage, null);
assert.ok(changed.lines.every(l => l.source === null && l.delta === null));
const missing = transcript(); missing.words = missing.words.filter(w => w.start < 8 || w.start >= 16);
const omissions = review.analyzeMusicReview(lyrics, missing, source, true);
assert.equal(omissions.lines[1].cover.coverage, 0); assert.equal(omissions.lines[1].delta, null);
const extra = transcript(); extra.words.splice(5, 0, { text: 'unexpected', start: 4, end: 4.5 });
assert.equal(review.analyzeMusicReview(lyrics, extra, source, true).extraCoverWords[0].text, 'unexpected');
const repeats = '[Verse]\nCome back home\n[Chorus]\nCome back home\nAnother day begins';
const repeated = review.analyzeMusicReview(repeats, transcript(repeats), transcript(repeats), true);
assert.ok(repeated.lines.slice(0, 2).every(l => l.repeated && l.delta === null));
assert.equal(repeated.offset, null);
const absent = { ...source, words: [], text: '' };
assert.equal(review.analyzeMusicReview(lyrics, absent, source, true).offset, null);
assert.equal(review.analyzeMusicReview(lyrics, absent, source, true).coverCoverage, 0);
const cjk = transcript('月光照我回家');
assert.equal(review.analyzeMusicReview('[Verse]\n月光照我回家', cjk, null, false).coverCoverage, 1);
const punct = transcript("Don't go home");
assert.equal(review.analyzeMusicReview('DON’T go home!', punct, null, false).coverCoverage, 1);
for (const bad of [-1, NaN, Infinity, 41]) {
  const invalid = transcript(); invalid.words[0].start = bad;
  assert.throws(() => review.analyzeMusicReview(lyrics, invalid, source, true), /invalid_music_transcript/);
}
assert.throws(() => review.analyzeMusicReview('[Verse]', source, source, true), /music_review_no_lyrics/);
assert.throws(() => review.analyzeMusicReview('word '.repeat(2001), source, source, true), /music_review_token_limit/);
const report = { version: 1, lyrics, source, cover: source, report: same, sourceName: 'synthetic.wav', createdAt: 1 };
const request = review.musicReviewRequest(report, { abc: '', params: { style: 'piano pop', max_duration: 40 } }, 'en');
assert.ok(request.system.includes('NOT heard audio')); assert.ok(request.system.includes('Null means unassessed'));
assert.equal(JSON.parse(request.prompt).task, 'review_music_transcripts');
const explanation = { summary: 'Matching recognized lyrics; listen to confirm.', issues: [{ evidence_id: 'line-2', explanation: 'This line matched.', suggestion: 'Listen to verify.' }] };
assert.equal(review.validateMusicReviewExplanation(JSON.stringify(explanation), same).issues[0].evidence_id, 'line-2');
assert.throws(() => review.validateMusicReviewExplanation(JSON.stringify({ ...explanation, issues: [{ ...explanation.issues[0], evidence_id: 'invented' }] }), same), /invalid_music_review/);
assert.throws(() => review.validateMusicReviewExplanation(JSON.stringify({ ...explanation, issues: [null] }), same), /invalid_music_review/);
console.log('PASS: matched lyrics, fixed offset, progressive drift, omissions, extra words, repeats, changed lyrics/tempo, CJK, empty ASR, invalid timestamps, bounds and grounded LLM references');
