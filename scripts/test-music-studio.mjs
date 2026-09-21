// Real score, edit, MIDI and ZIP logic; synthetic IPC/storage for job orchestration.
// Run: node scripts/test-music-studio.mjs
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import { webcrypto } from 'node:crypto';

function load(path, imports = {}, globals = {}) {
  const module = { exports: {} }, state = value => value; state.raw = state;
  const source = fs.readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
  const code = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
  vm.runInNewContext(code, { module, exports: module.exports, require: name => { assert.ok(name in imports, `Unexpected import ${name}`); return imports[name]; }, $state: state,
    console, TextEncoder, TextDecoder, Blob, URL, atob, Uint8Array, Uint32Array, DataView, crypto: webcrypto, ...globals }, { filename: path });
  return module.exports;
}
const score = load('src/lib/utils/musicScore.ts');
const cover = load('src/lib/utils/musicCover.ts');
const edit = load('src/lib/utils/musicEdit.ts', { './musicScore.js': score });
const settings = load('src/lib/utils/musicSettings.ts');
const project = load('src/lib/utils/musicProject.ts', { './api.js': {}, './musicAudio.js': {}, './ipc.js': { isTauri: false } });
const preview = load('src/lib/utils/musicPreview.ts', { './musicScore.js': score });
const abc = cover.COVER_EXAMPLE;
const parsed = score.parseMusicScore(abc);
assert.equal(parsed.voices.Vocal.bars.length, 8);
assert.equal(parsed.quarters, 32);
assert.equal(parsed.seconds, 32 * 60 / parsed.bpm);
const harmony = abc.replace('C2 E2 G2 E2', '"Cmaj7"C2 E2 G2 E2');
assert.notEqual(harmony, abc, 'Fixture must add a chord');
assert.equal(score.compareMusicScores(parsed, score.parseMusicScore(harmony)).match, true);
assert.equal(score.prepareCoverScore(harmony), abc);
assert.equal(score.parseMusicScore(score.prepareCoverScore(harmony, 'Ins', true)).voices.Vocal.notes.length, 0);
assert.ok(score.scoreChordNotes(score.parseMusicScore(harmony)).length >= 4);
const tempo = score.setScoreTempo(abc, 60);
assert.equal(score.parseMusicScore(tempo).seconds, 32);
assert.equal(score.compareMusicScores(parsed, score.parseMusicScore(tempo)).match, false);
assert.equal(score.compareMusicScores(parsed, score.parseMusicScore(tempo), 'both', true).match, true);
const minimal = body => `X:1\nT:\nM:4/4\nL:1/8\nQ:1/4=120\nV:Vocal clef=treble\nV:Ins clef=treble\nK:C\n% verse\nV:Vocal\n${body}\nV:Ins\nZ2|\n`;
const tied = score.parseMusicScore(minimal('^C8-|C2 C2 c2 C2|'));
assert.deepEqual(JSON.parse(JSON.stringify(tied.voices.Vocal.notes)), [{ onset:0,pitch:61,duration:5 },{ onset:5,pitch:60,duration:1 },{ onset:6,pitch:72,duration:1 },{ onset:7,pitch:60,duration:1 }]);
const accidentals = score.parseMusicScore(minimal('^C2 c2 =C2 c2|C8|'));
assert.deepEqual(Array.from(accidentals.voices.Vocal.notes,n => n.pitch), [61,73,60,72,60]);
for (const bad of [minimal('C2|C8|'), minimal('C8-|D8|'), minimal('[CEG]8|C8|'), minimal('(3CDE C4|C8|'), minimal('"Cmaj9"C8|C8|'), minimal('z8-|C8|')]) assert.throws(() => score.parseMusicScore(bad));
assert.throws(() => score.parseMusicScore('x'.repeat(131073)), /128 KiB/);
const midi = score.scoreMidi(score.parseMusicScore(harmony), 'both', true);
assert.equal(Buffer.from(midi.slice(0,4)).toString(), 'MThd');
assert.equal(new DataView(midi.buffer).getUint16(10), 4, 'Tempo + two melodies + chord reference');
assert.ok(Buffer.from(midi).includes(Buffer.from([0xff,0x51,3])));
const svg = preview.scoreSvg(parsed, '<script>alert("x")</script>');
assert.ok(!svg.includes('<script>')); assert.ok(svg.includes('&lt;script&gt;'));
const params = { title:'Test', checkpoint:'yue2', style:'English, piano pop', lyrics:'[Verse]\nHome again', abc, planning:'full', max_duration:120,steps:32,seed:'42' };
const context = { params, brief:'Add jazz harmony', constraints:{ melody:'both',rhythm:true,tempo:true,lyrics:true,structure:true } };
const response = { abc:harmony, style:'English, jazz harmony',lyrics:params.lyrics,summary:'Added a major seventh chord.' };
assert.equal(edit.validateMusicEdit(JSON.stringify(response), context).check.match, true);
assert.equal(JSON.parse(edit.musicEditRequest(context).prompt).abc, abc);
const instrumentalContext = { ...context, params: { ...params, lyrics: '' } };
assert.equal(edit.validateMusicEdit(JSON.stringify({ ...response, lyrics: '' }), instrumentalContext).check.match, true);
assert.ok(edit.musicEditRequest(instrumentalContext).system.includes('Blank or whitespace-only lyrics mean purely instrumental'));
assert.throws(() => edit.validateMusicEdit(JSON.stringify(response), instrumentalContext), /Lyrics changed/);
assert.equal(edit.validateMusicEdit(JSON.stringify({ ...response, lyrics: '' }), { ...context, constraints: { ...context.constraints, lyrics: false, structure: false } }).lyrics, '');
for (const changed of [{ ...response, lyrics:'Changed' }, { ...response, abc:tempo }, { ...response, abc:harmony.replace('C2 E2 G2 E2','D2 E2 G2 E2') }, { ...response, extra:'no' }]) assert.throws(() => edit.validateMusicEdit(JSON.stringify(changed), context));
assert.equal(edit.validateMusicEdit(JSON.stringify({...response,abc:tempo}), {...context,constraints:{...context.constraints,tempo:false}}).check.match,true);
const archive = await project.musicZip([{name:'score.abc',blob:new Blob([abc])},{name:'test.bin',blob:new Blob([new Uint8Array([0,255,10,13])])}]);
const zip = Buffer.from(await archive.arrayBuffer());
let offset = 0;
for (const content of [Buffer.from(abc),Buffer.from([0,255,10,13])]) {
  assert.equal(zip.readUInt32LE(offset),0x04034b50);
  const length=zip.readUInt32LE(offset+18),nameLength=zip.readUInt16LE(offset+26);
  const start=offset+30+nameLength;
  assert.deepEqual(zip.subarray(start,start+length),content);
  offset=start+length;
}
assert.equal(zip.readUInt32LE(offset),0x02014b50);
assert.equal(zip.readUInt32LE(zip.length-22),0x06054b50);
const crcFixture=Buffer.from(await (await project.musicZip([{name:'crc.txt',blob:new Blob(['123456789'])}])).arrayBuffer());
assert.equal(crcFixture.readUInt32LE(14),0xcbf43926,'Standard CRC32 check vector');
await assert.rejects(() => project.musicZip([{name:'../secret',blob:new Blob(['bad'])}]),/filename/);
console.log('PASS score dialect, sounding ties, accidentals, harmony, tempo, preservation constraints, MIDI, escaped SVG and ZIP bytes');

let user=null, nextStatus={status:'queued'}, timer, submits=[], fail=false;
const storage=new Map(), saved={songs:[],playlists:[],plans:[],batches:[]};
const api={ generateMusic:async p=>{ submits.push(structuredClone(p)); if(fail)throw new Error('synthetic failure'); return {prompt_id:`p${submits.length}`,worker_id:0,seed:p.seed==='-1'?'123':p.seed,kind:p.task}; }, getMusicStatus:async()=>nextStatus, interruptGeneration:async()=>{}, loadMusicAudio:async()=>btoa('fLaCtest') };
const imports={ '../utils/api.js':api,'../utils/ipc.js':{getAuthUser:()=>user},'./locale.svelte.js':{locale:{t:key=>key}},'../utils/musicAudio.js':{songTitle:()=>'',blobBase64:async()=>''},'../utils/musicCover.js':cover,'../utils/musicSettings.js':settings,
  '../utils/musicScore.js':score, '../utils/musicLibrary.js':{musicLibrary:{load:async()=>structuredClone(saved),savePlan:async(_,p)=>saved.plans.push(p),saveBatch:async(_,b)=>{saved.batches=saved.batches.filter(x=>x.id!==b.id).concat(structuredClone(b));},saveSong:async()=>{},audio:async()=>new Blob(['fLaC'])}} };
const globals={setTimeout:fn=>{timer=fn;return 1;},clearTimeout:()=>{},localStorage:{getItem:key=>storage.get(key)??null,setItem:(key,value)=>storage.set(key,value),removeItem:key=>storage.delete(key)}};
imports['../utils/musicStyleProfiles.js'] = load('src/lib/utils/musicStyleProfiles.ts', { './musicAudioStyle.js': load('src/lib/utils/musicAudioStyle.ts', { './api.js': {}, './musicAudio.js': {} }) });
imports['../utils/musicComparison.js'] = load('src/lib/utils/musicComparison.ts', { './api.js': {}, './musicAudio.js': {} });
const {music}=load('src/lib/stores/music.svelte.ts',imports,globals);
music.capabilities={missing_nodes:[],checkpoints:['yue2']}; music.params=structuredClone(params);
await Promise.all([music.generatePlan(),music.generatePlan()]); assert.equal(submits.length,1,'Double clicks submit only once');
assert.equal(submits[0].task,'plan');
nextStatus={status:'completed',kind:'plan',abc:harmony,metadata:{abc_truncated:false,semantic_truncated:null,score_source:'generated'}};
await music.poll();
assert.equal(music.params.abc,abc,'Plan is reviewed before replacing the draft');
assert.equal(saved.plans.length,1);
music.applyPlan(music.planCandidate); assert.equal(music.params.abc,harmony); music.undoScore(); assert.equal(music.params.abc,abc);
music.params.abc='changed';music.undoScore();assert.equal(music.params.abc,'changed');music.params=structuredClone(params);
music.candidateCount=4; nextStatus={status:'queued'}; await music.generate();
assert.equal(submits.length,2); const batch=music.activeBatch;
assert.equal(new Set(batch.attempts.map(a=>a.seed)).size,4); assert.equal(batch.attempts[0].seed,'42');
nextStatus={status:'error',error:'candidate failed'}; await music.poll();
await new Promise(resolve=>setImmediate(resolve));
assert.equal(submits.length,3,'Failed candidate advances sequentially');
assert.equal(batch.attempts[0].status,'failed');
const resumed=load('src/lib/stores/music.svelte.ts',imports,globals).music;
await resumed.loadLibrary();assert.equal(resumed.job.prompt_id,'p3');assert.equal(resumed.batchRunning,false,'Reload observes a job without starting pending candidates');assert.equal(submits.length,3);
await music.cancel(); assert.equal(music.batchRunning,false); assert.ok(batch.attempts.slice(2).every(a=>a.status==='cancelled'));
user='other';await resumed.loadLibrary();assert.equal(resumed.job,null);assert.equal(resumed.plans.length,saved.plans.length); // mock returns same data; active handle still isolated
console.log('PASS review-first plans, immutable undo, duplicate prevention, sequential distinct seeds, failure continuation, cancellation and job recovery');
for (const lyrics of ['', ' \n\t ']) {
  for (const action of ['generatePlan', 'generate']) {
    const instrumental = load('src/lib/stores/music.svelte.ts', imports, globals).music;
    instrumental.capabilities = { missing_nodes: [], checkpoints: ['yue2'] };
    instrumental.params = { ...structuredClone(params), lyrics };
    instrumental.libraryLoaded = true;
    const before = submits.length;
    await instrumental[action]();
    assert.equal(submits.length, before + 1, 'Blank lyrics may reach the backend for both tasks');
    assert.equal(submits.at(-1).lyrics, lyrics);
    assert.equal(submits.at(-1).task, action === 'generatePlan' ? 'plan' : 'audio');
    assert.equal(instrumental.params.lyrics, lyrics, 'Do not fill the lyrics field with directions');
  }
}
const noStyle = load('src/lib/stores/music.svelte.ts', imports, globals).music;
noStyle.capabilities = { missing_nodes: [], checkpoints: ['yue2'] };
noStyle.params = { ...structuredClone(params), lyrics: '', style: ' ' };
const beforeNoStyle = submits.length;
await noStyle.generatePlan();
assert.equal(submits.length, beforeNoStyle);
assert.equal(noStyle.error, 'music.plan_needs_text');
console.log('PASS instrumental planning, generation, composition editing and explicit lyric preservation');
