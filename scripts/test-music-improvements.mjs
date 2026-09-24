// Run: node scripts/test-music-improvements.mjs
// Production helpers with synthetic provider/storage boundaries; no paid requests.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import { webcrypto } from 'node:crypto';
function load(path, imports = {}, globals = {}) {
  const module = { exports: {} }, state = value => value; state.raw = state;
  const source = fs.readFileSync(new URL('../' + path, import.meta.url), 'utf8');
  const code = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
  vm.runInNewContext(code, { module, exports: module.exports, require: name => {
    assert.ok(name in imports, 'Unexpected import ' + name); return imports[name];
  }, $state: state, TextEncoder, Blob, crypto: webcrypto, console, ...globals }, { filename: path });
  return module.exports;
}
const audio = load('src/lib/utils/musicAudioStyle.ts', { './api.js': {}, './musicAudio.js': {} });
const profiles = load('src/lib/utils/musicStyleProfiles.ts', { './musicAudioStyle.js': audio });
const target = { instrumental: true, max_duration: 60, language: 'en-US' };
const profile = { description: 'Warm piano with light percussion.', style: 'Instrumental piano soul with a resolved ending.', estimates: ['Tempo is approximate.'], audio_sha256: 'a'.repeat(64), duration_seconds: 20, backend_id: 'audio-test', source_start_seconds: 30, source_end_seconds: 50, excerpt: true };
const saved = { version: 1, id: 'saved-1', name: 'Piano', createdAt: 1, profile, style: profile.style, target, model: 'audio-test' };
for (const [start, end, duration, valid] of [[30,50,60,true],[600,660,1000,true],[-1,4,60,false],[5,5,60,false],[0,361,600,false],[0,70,60,false],[NaN,50,60,false],[0,Infinity,60,false]]) {
  assert.equal(audio.audioRangeValid(start,end,duration),valid);
}
assert.equal(audio.audioStyleTargetKey(target), audio.audioStyleTargetKey({...target, source_start:30,source_end:50}));
assert.throws(() => audio.validateAudioStyle({...profile,source_end_seconds:400},'audio-test'));
const clean = profiles.readSavedMusicStyle({...saved, audio: new Blob(['private']), key:'secret', url:'https://private', profile:{...profile,filename:'private.wav'}, target:{...target,lyrics:'private lyrics'}});
assert.equal(JSON.stringify(clean).includes('private'),false);
assert.equal(JSON.stringify(clean).includes('secret'),false);
assert.equal(clean.profile.excerpt,true);
for (const invalid of [{...saved,name:''},{...saved,target:{...target,max_duration:361}},{...saved,profile:{...profile,description:''}}]) assert.throws(()=>profiles.readSavedMusicStyle(invalid));
const adaptation = profiles.savedStyleRequest(saved,{...target,instrumental:false,max_duration:120});
const payload = JSON.parse(adaptation.prompt);
assert.equal(payload.target.max_duration,120);
assert.equal(payload.target.instrumental,false);
assert.equal(payload.saved_target.instrumental,true);
assert.equal('audio' in payload,false);
assert.ok(adaptation.system.includes('No audio is being supplied'));
console.log('PASS excerpt boundaries, target identity, profile validation/privacy and text-only adaptation');

let measurementCurrent = true, measurementCalls = [], measurementReply = {status:'completed',loudness_lufs:-20};
const comparison = load('src/lib/utils/musicComparison.ts', {
  './musicAudio.js': {blobBase64:async()=> 'encoded'},
  './api.js': {measureMusicLoudness:async()=>{measurementCalls.push('start');return 'measure-1';},getMusicAudioStyle:async(id,cancel)=>{measurementCalls.push({id,cancel});return measurementReply;}},
}, {setTimeout:fn=>{measurementCurrent=false;fn();}});
const gains = comparison.comparisonGains([-10,-20]);
assert.ok(Math.abs(gains[0]-0.316227766)<1e-8);
assert.equal(gains[1],1);
assert.equal(comparison.comparisonGains([-70,-20]),null);
assert.equal(comparison.comparisonGains([null,-20]),null);
assert.equal(comparison.comparisonSeek(45,60,30),29.95);
assert.equal(comparison.comparisonSeek(-4,60,30),0);
const note = { id: comparison.comparisonId('b','a'), song_ids:['b','a'], votes:{style:'a',ending:'tie'},notes:'A has a cleaner ending.',updatedAt:1 };
assert.equal(comparison.readComparisonNotes(note).votes.style,'a');
assert.equal(comparison.comparisonId('a','b'),note.id);
assert.throws(()=>comparison.readComparisonNotes({...note,votes:{style:'outside'}}));
assert.throws(()=>comparison.readComparisonNotes({...note,notes:'x'.repeat(2001)}));
assert.equal(await comparison.measurePlaybackLoudness(new Blob(['audio']),()=>measurementCurrent,()=>{}),-20);
measurementReply = {status:'running'};
assert.equal(await comparison.measurePlaybackLoudness(new Blob(['audio']),()=>measurementCurrent,()=>{}),null);
assert.equal(measurementCalls.at(-1).cancel,true);
console.log('PASS attenuation-only loudness matching, overlap seeking, stable votes and cancellation');

const score = load('src/lib/utils/musicScore.ts');
const edit = load('src/lib/utils/musicEdit.ts',{'./musicScore.js':score});
const params = {style:'Piano',lyrics:'',abc:'',planning:'off',max_duration:60,checkpoint:'yue2',steps:32,seed:'42'};
const constraints = {melody:'both',rhythm:true,tempo:true,lyrics:true,structure:true};
const context = {params,brief:'Warmer piano production',scope:'style',constraints};
assert.equal('abc' in JSON.parse(edit.musicEditRequest(context).prompt),false);
const proposal = edit.validateMusicEdit(JSON.stringify({style:'Warm instrumental piano.',summary:'Warmer piano'}),context);
assert.equal(proposal.abc,''); assert.equal(proposal.lyrics,'');
assert.equal(proposal.check.match,true);
assert.throws(()=>edit.validateMusicEdit(JSON.stringify({style:'Warm piano',lyrics:'Injected words',summary:'No'}),context),/selected fields/);
const vocal = {...context,scope:'lyrics',params:{...params,lyrics:'[Verse]\nGoing home'},constraints:{...constraints,lyrics:false}};
assert.equal(edit.validateMusicEdit(JSON.stringify({lyrics:'[Verse]\nHeading home',summary:'Rephrased'}),vocal).style,params.style);
assert.throws(()=>edit.validateMusicEdit(JSON.stringify({lyrics:'[Chorus]\nHeading home',summary:'Changed section'}),vocal),/layout/);
assert.throws(()=>edit.musicEditRequest({...context,scope:'score'}));
console.log('PASS scoped edits without a score, protected field rejection and lyric-layout preservation');

const arrangement = load('src/lib/utils/musicArrangement.ts');
for (const duration of [1,5,30,60,120,360]) {
  for (const lyrics of ['', '[Verse]\nOne line\n[Chorus]\nAnother line']) {
    const sections = arrangement.suggestArrangement(duration,lyrics);
    assert.equal(sections.reduce((sum,s)=>sum+s.seconds,0),duration);
    assert.equal(arrangement.validArrangement(sections,duration),true);
    assert.ok(lyrics || sections.every(s=>!['verse','chorus'].includes(s.kind)));
    const style = arrangement.arrangementStyle('Piano',sections,duration,!lyrics);
    assert.ok(style.includes('approximate section timings'));
    assert.ok(lyrics || style.includes('Entirely instrumental'));
  }
}
const sections = [{kind:'verse',seconds:30,direction:'Gentle piano'},{kind:'chorus',seconds:30,direction:'Full drums'}];
const fit = arrangement.fitArrangement(sections,31);
assert.equal(fit[0].direction,'Gentle piano');
assert.equal(sections[0].seconds,30,'Fit does not mutate a prior draft');
assert.equal(arrangement.validArrangement(sections,59),false);
assert.throws(()=>arrangement.fitArrangement(sections,1));
console.log('PASS duration-fitting arrangement prototype, lyric section order and instrumental guidance');

let owner = null, fail = false, pendingWrite = null;
const databases = new Map();
function database(account) {
  if (!databases.has(account)) databases.set(account,{songs:[],playlists:[],styles:[],comparisons:[]});
  return databases.get(account);
}
const library = {
  load: async account => structuredClone(database(account)),
  saveStyle: async(account,record)=>{if(pendingWrite)await pendingWrite;if(fail)throw Error('quota');const db=database(account);db.styles=db.styles.filter(p=>p.id!==record.id).concat(structuredClone(record));},
  deleteStyle:async(account,id)=>{database(account).styles=database(account).styles.filter(p=>p.id!==id);},
  saveComparison:async(account,record)=>{const db=database(account);db.comparisons=db.comparisons.filter(p=>p.id!==record.id).concat(structuredClone(record));},
};
const {music} = load('src/lib/stores/music.svelte.ts',{
  '../utils/api.js':{},'../utils/ipc.js':{getAuthUser:()=>owner},'./locale.svelte.js':{locale:{t:key=>key}},
  '../utils/musicAudio.js':{},'../utils/musicLibrary.js':{musicLibrary:library},'../utils/musicCover.js':{},
  '../utils/musicScore.js':score,'../utils/musicSettings.js':load('src/lib/utils/musicSettings.ts'),
  '../utils/musicStyleProfiles.js':profiles,'../utils/musicComparison.js':comparison,
},{setTimeout,clearTimeout,localStorage:{getItem:()=>null,setItem(){},removeItem(){}}});
await music.loadLibrary();
assert.equal(await music.saveStyleProfile(saved),true);
assert.equal(music.styleProfiles.length,1);
assert.equal(await music.saveStyleProfile({...saved,name:'Renamed'}),true);
assert.equal(music.styleProfiles.length,1);
assert.equal(music.styleProfiles[0].name,'Renamed');
assert.equal(await music.saveComparisonNotes(note),true);
fail=true; assert.equal(await music.saveStyleProfile({...saved,name:'Failed'}),false);
assert.equal(music.styleProfiles[0].name,'Renamed'); fail=false;
owner='alice'; await music.loadLibrary(); assert.equal(music.styleProfiles.length,0); assert.equal(music.comparisonNotes.length,0);
await music.saveStyleProfile({...saved,id:'alice-profile'});
owner=null; await music.loadLibrary(); assert.equal(music.styleProfiles[0].name,'Renamed'); assert.equal(music.comparisonNotes.length,1);
let finish; pendingWrite = new Promise(resolve=>finish=resolve);
const saving = music.saveStyleProfile({...saved,name:'Delayed'});
await new Promise(resolve=>setImmediate(resolve));
owner='bob'; await music.loadLibrary(); finish(); await saving; pendingWrite=null;
assert.equal(music.styleProfiles.length,0,'An old account write never enters the new account state');
owner=null; await music.loadLibrary(); assert.equal(music.styleProfiles[0].name,'Delayed');
await music.deleteStyleProfile(saved.id); assert.equal(music.styleProfiles.length,0);
database('user:corrupt').styles=[{id:'corrupt'}]; owner='corrupt'; await music.loadLibrary(); assert.equal(music.styleProfiles.length,0);
console.log('PASS profile CRUD, storage failure, durable comparison notes, account isolation and corrupt-record recovery');

const english = load('src/lib/locales/en.ts').default;
for (const language of ['de','es','fr','it','ja','ko','pl','pt','ru','zh','zh-tw']) {
  const translated = load('src/lib/locales/'+language+'.ts').default;
  for (const key of Object.keys(english).filter(k=>/^music\.(audio_|profile|edit_|arrange_|compare_)/.test(k))) {
    assert.ok(translated[key],language+': '+key);
    assert.deepEqual((translated[key].match(/\{\w+\}/g)||[]).sort(),(english[key].match(/\{\w+\}/g)||[]).sort(),language+': '+key);
  }
}
console.log('PASS all 12 locales contain music improvement strings with matching placeholders');
