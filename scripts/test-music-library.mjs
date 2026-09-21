// Run: node scripts/test-music-library.mjs
// Real store logic with synthetic audio/storage boundaries; no GPU or provider calls.
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import ts from "typescript";

const source = path => fs.readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
function load(path, imports = {}, globals = {}) {
  const module = { exports: {} };
  const state = value => value;
  state.raw = state;
  const compiled = ts.transpileModule(source(path), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
  vm.runInNewContext(compiled, { module, exports: module.exports, require: name => {
    assert.ok(name in imports, `Unexpected import ${name}`);
    return imports[name];
  }, $state: state, console, setTimeout, clearTimeout, Blob, URL, atob, crypto, localStorage: { getItem() { return null; }, setItem() {}, removeItem() {} }, ...globals });
  return module.exports;
}
const timing = load("src/lib/utils/lyricTiming.ts");
const lines = timing.lyricLines("[Verse]\nOne small light\nLeads me home\n\n[Chorus]\nStay with me");
lines[1].start = 2;
lines[2].start = 8;
lines[4].start = 15;
const alignment = timing.manualLyricAlignment(lines, 30);
assert.equal(alignment.matched, 3);
assert.equal(alignment.lines[1].end, 8);
assert.equal(alignment.lines[4].end, 30);
assert.equal(timing.activeLyricLine(alignment.lines, 7.99), 1);
assert.equal(timing.activeLyricLine(alignment.lines, 8), 2);
assert.equal(timing.activeLyricLine(alignment.lines, 30), -1);
assert.equal(lines[1].end, null, "Saving timing does not mutate the editable draft");
for (const invalid of [-1, NaN, Infinity, 8, 31]) {
  assert.throws(() => timing.manualLyricAlignment(lines.map((line, i) => i === 1 ? { ...line, start: invalid } : line), 30), /invalid_timing/);
}
assert.equal(timing.manualLyricAlignment(timing.lyricLines("[Verse]\nUnmarked"), 30).matched, 0);
assert.throws(() => timing.manualLyricAlignment(lines, 0), /invalid_timing/);
console.log("PASS: manual line timing, boundaries, untimed lines, invalid/overlapping marks and immutable drafts");

const clone = value => structuredClone(value);
let user = null;
let failStorage = false;
let remoteCalls = 0;
let pendingRemote;
const recordings = new Map();
const library = owner => {
  if (!recordings.has(owner)) recordings.set(owner, { songs: new Map(), blobs: new Map(), playlists: new Map() });
  return recordings.get(owner);
};
const params = { style: "Acoustic folk", lyrics: "[Verse]\nOne small light", checkpoint: "yue2", planning: "off", abc: "", max_duration: 30, steps: 32, seed: "1" };
const song = id => ({ prompt_id: id, worker_id: 1, seed: "1", filename: `${id}.flac`, abc: "", params: { ...params }, title: `Song ${id}`, saved: true, createdAt: 1 });
for (const id of ["a", "b"]) {
  library("host").songs.set(id, song(id));
  library("host").blobs.set(id, new Blob(["synthetic audio"]));
}
const audioInstances = [];
class AudioBoundary {
  constructor(src) { this.src = src; this.paused = true; this.currentTime = 0; this.duration = 30; audioInstances.push(this); }
  async play() { this.paused = false; this.onplay?.(); }
  pause() { this.paused = true; this.onpause?.(); }
  removeAttribute() { this.src = ""; }
  load() {}
}
const { music } = load("src/lib/stores/music.svelte.ts", {
  "../utils/api.js": {
    loadMusicAudio: async () => { remoteCalls++; return pendingRemote ? await pendingRemote : btoa("fLaC synthetic"); },
  },
  "../utils/ipc.js": { getAuthUser: () => user },
  "../utils/musicCover.js": load("src/lib/utils/musicCover.ts", {}, { TextEncoder }),
  "../utils/musicSettings.js": load("src/lib/utils/musicSettings.ts"),
  "../utils/musicStyleProfiles.js": load("src/lib/utils/musicStyleProfiles.ts", { "./musicAudioStyle.js": load("src/lib/utils/musicAudioStyle.ts", { "./api.js": {}, "./musicAudio.js": {} }) }),
  "../utils/musicComparison.js": load("src/lib/utils/musicComparison.ts", { "./api.js": {}, "./musicAudio.js": {} }),
  "../utils/musicScore.js": load("src/lib/utils/musicScore.ts", {}, { TextEncoder }),
  "./locale.svelte.js": { locale: { t: key => key } },
  "../utils/musicAudio.js": { songTitle: () => "Fallback title", exportFlac: async () => "downloaded", blobBase64: async () => "" },
  "../utils/musicLibrary.js": { musicLibrary: {
    load: async owner => ({ songs: [...library(owner).songs.values()].map(clone), playlists: [...library(owner).playlists.values()].map(clone) }),
    audio: async (owner, id) => library(owner).blobs.get(id),
    saveSong: async (owner, result, blob) => {
      if (failStorage) throw Error("quota");
      library(owner).songs.set(result.prompt_id, clone(result));
      if (blob) library(owner).blobs.set(result.prompt_id, blob);
    },
    savePlaylist: async (owner, playlist) => {
      if (failStorage) throw Error("quota");
      library(owner).playlists.set(playlist.id, clone(playlist));
    },
    deletePlaylist: async (owner, id) => library(owner).playlists.delete(id),
  } },
// Plain HTTP LAN clients do not expose randomUUID; playlist creation still works.
}, { Audio: AudioBoundary, crypto: { getRandomValues: values => crypto.getRandomValues(values) } });
await music.loadLibrary();
assert.equal(music.results.length, 2);
await music.playAll(music.results);
assert.equal(music.playing, true);
assert.equal(music.selectedResult.prompt_id, "a");
assert.equal(remoteCalls, 0, "Saved recordings play without the generation server");
const firstAudio = audioInstances.at(-1);
firstAudio.onloadedmetadata();
await music.writes;
music.seek(500);
assert.equal(firstAudio.currentTime, 30);
music.seek(-1);
assert.equal(music.currentTime, 0);
music.setVolume(0.3);
music.toggleMute();
assert.equal(firstAudio.volume, 0.3);
assert.equal(firstAudio.muted, true);
music.toggleLoop();
assert.equal(firstAudio.loop, true);
music.toggleLoop();
await music.togglePlayback();
assert.equal(music.playing, false);
await music.togglePlayback();
firstAudio.onended();
await new Promise(resolve => setImmediate(resolve));
assert.equal(music.selectedResult.prompt_id, "b", "Ended advances the shared queue");
assert.equal(firstAudio.src, "", "Old audio resources are released");
await music.previous();
assert.equal(music.selectedResult.prompt_id, "a");
const selectedAudio = audioInstances.at(-1);
await music.updateSong("a", { title: "  Home Again  " });
assert.equal(music.title, "Home Again");
assert.equal(music.audio, selectedAudio, "Renaming does not restart playback");
await Promise.all([music.updateSong("a", { title: "Home" }), music.updateSong("a", { alignment })]);
assert.equal(library("host").songs.get("a").title, "Home");
assert.equal(library("host").songs.get("a").alignment.matched, 3, "Concurrent metadata edits merge instead of overwriting");
await music.savePlaylist("Night drive");
const playlistId = music.playlists[0].id;
await music.setPlaylistSong(playlistId, "a", true);
await music.setPlaylistSong(playlistId, "a", true);
await music.setPlaylistSong(playlistId, "b", true);
assert.equal(music.playlists[0].songIds.join(), "a,b");
await music.setPlaylistSong(playlistId, "a", false);
assert.equal(music.playlists[0].songIds.join(), "b");
await music.savePlaylist("Evening", playlistId);
assert.equal(music.playlists[0].name, "Evening");
failStorage = true;
assert.equal(await music.updateSong("a", { title: "Unsaved" }), false);
assert.equal(music.title, "Home", "Failed storage leaves committed metadata intact");
assert.match(music.libraryError, /quota/);
failStorage = false;
await music.deletePlaylist(playlistId);
assert.equal(music.playlists.length, 0);
assert.equal(library("host").songs.size, 2, "Deleting a playlist preserves its songs");
console.log("PASS: offline saved playback, shared controls, ordered queue, titles, playlist CRUD, serialized metadata and storage failure recovery");

let finishRemote;
pendingRemote = new Promise(resolve => { finishRemote = resolve; });
const fresh = { ...song("c"), saved: false };
music.results = [...music.results, fresh];
const stalePlay = music.play(fresh);
await new Promise(resolve => setImmediate(resolve));
await music.play(music.results[1]);
finishRemote(btoa("fLaC synthetic"));
await stalePlay;
assert.equal(music.selectedResult.prompt_id, "b", "A late fetch cannot replace the latest song selection");
assert.ok(library("host").songs.has("c"), "Late recordings can still be archived");
pendingRemote = null;
user = "alice";
await music.loadLibrary();
assert.equal(music.playing, false);
assert.equal(music.audioUrl, "");
assert.equal(music.results.length, 0, "Account change clears playback and library state");
assert.equal(music.playlists.length, 0);
assert.equal(library("host").songs.size, 3);
user = null;
await music.loadLibrary();
assert.equal(music.results.length, 3, "Reload restores saved songs and metadata");
assert.equal(music.results.find(song => song.prompt_id === "a").alignment.matched, 3);
music.stopPlayback();
console.log("PASS: competing playback requests, archive persistence, account isolation and reload recovery");
