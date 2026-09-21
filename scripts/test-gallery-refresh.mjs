// Run: node scripts/test-gallery-refresh.mjs
// Exercise the real store with synthetic directory/metadata responses.
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import ts from "typescript";

const compiled = ts.transpileModule(
  fs.readFileSync(new URL("../src/lib/stores/gallery.svelte.ts", import.meta.url), "utf8"),
  { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } },
).outputText;
const tick = () => new Promise(resolve => setImmediate(resolve));
function deferred() {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
}
const entry = (filename, modified_ms = 1, size_bytes = 100) => ({ filename, modified_ms, size_bytes });
const output = (filename, saved = false) => ({
  filename, prompt_id: filename, subfolder: "", type: "output",
  gallery_filename: saved ? filename : undefined,
  url: `blob:${filename}`, metadata: { positive_prompt: "session prompt" },
});
function harness(desktop = false) {
  const disk = { entries: [], metadata: new Map(), listGate: null, metadataGate: null, fail: false };
  const calls = { lists: 0, metadata: [], revoked: [] };
  const api = {
    listGalleryImageEntries: async () => {
      calls.lists++;
      if (disk.fail) throw Error("Synthetic directory read failure");
      const result = disk.entries.map(item => ({ ...item }));
      if (disk.listGate) await disk.listGate.promise;
      return result;
    },
    readImageMetadata: async filename => {
      calls.metadata.push(filename);
      const result = disk.metadata.get(filename) ?? null;
      if (disk.metadataGate) await disk.metadataGate.promise;
      return result;
    },
    getStorageInfo: async () => ({ usage_bytes: 0, limit_bytes: 0, expiry_secs: 0, images: [] }),
    deleteGalleryImage: async filename => {
      disk.entries = disk.entries.filter(item => item.filename !== filename);
    },
    saveToGalleryBytes: async () => { await disk.saveGate.promise; return "saved.png"; },
    renameGalleryImage: () => { throw Error("Refresh must not rename files"); },
  };
  const imports = {
    "../utils/api.js": api,
    "../utils/ipc.js": { isTauri: desktop, isBrowserMode: !desktop, getAuthToken: () => "synthetic" },
    "@tauri-apps/api/core": { convertFileSrc: (name, protocol) => `https://${protocol}.localhost/${encodeURIComponent(name)}` },
    "./locale.svelte.js": { locale: { t: key => key } },
    "./generation.svelte.js": { generation: { manualSaveMode: false } },
    "./progress.svelte.js": { progress: {} },
    "../artist-gallery/client.js": {},
    "../utils/cdnFetch.js": {},
    "../artist-gallery/detection.js": {},
  };
  const module = { exports: {} };
  vm.runInNewContext(compiled, {
    module, exports: module.exports, $state: value => value,
    require: name => {
      assert.ok(name in imports, `Unexpected import ${name}`);
      return imports[name];
    },
    console: { log() {}, debug() {}, error() {}, warn() {} },
    localStorage: { getItem: () => null, setItem() {} },
    setTimeout: (fn, delay) => {
      const timer = setTimeout(fn, delay);
      if (delay > 100) timer.unref();
      return timer;
    },
    clearTimeout, Blob, FileReader: undefined,
    URL: { revokeObjectURL: url => calls.revoked.push(url) },
  });
  return { gallery: module.exports.gallery, disk, calls };
}
const names = gallery => Array.from(gallery.images, image => image.gallery_filename ?? image.filename);

{
  const { gallery, disk, calls } = harness();
  await gallery.refresh();
  assert.equal(gallery.images.length, 0);
  assert.equal(gallery.toast.message, "gallery.toast.refreshed");
  const original = "job__txt2img__original.png";
  disk.entries = [entry(original), entry("removed.png")];
  disk.metadata.set(original, { positive_prompt: "original prompt" });
  await gallery.loadFromDisk();
  await gallery.hydrateMetadataInBackground();
  const saved = gallery.images[0];
  saved.generationTimeMs = 1234;
  const unsaved = output("unsaved.png");
  gallery.images = [unsaved, ...gallery.images];
  gallery.sessionImages = [unsaved, saved];
  gallery.boardAssignments = { [original]: "Favorites" };
  gallery.selectedImage = gallery.images[2];
  gallery.lastSelectedImage = gallery.images[2];
  gallery.lightboxOpen = true;
  gallery.comparePin = gallery.images[2];
  gallery.compareA = gallery.images[2];
  gallery.compareB = saved;
  gallery.compareOpen = true;
  disk.entries = [entry("restored #1.png"), entry(original, 2)];
  disk.metadata.set(original, { positive_prompt: "updated prompt" });
  disk.metadata.set("restored #1.png", { positive_prompt: "restored prompt" });
  const oldThumbnail = saved.thumbnailUrl;
  await gallery.refresh();
  assert.deepEqual(names(gallery), ["unsaved.png", "restored #1.png", original]);
  assert.equal(gallery.images[2], saved, "Retain live session object identity");
  assert.equal(saved.generationTimeMs, 1234);
  assert.equal(saved.metadata.positive_prompt, "updated prompt");
  assert.notEqual(saved.thumbnailUrl, oldThumbnail, "Replaced files bypass the thumbnail cache");
  assert.match(gallery.images[1].thumbnailUrl, /restored%20%231.png\?token=synthetic&v=/);
  assert.equal(gallery.images[1].metadata.positive_prompt, "restored prompt");
  assert.deepEqual(Array.from(gallery.sessionImages), [unsaved, saved]);
  assert.equal(gallery.boardAssignments[original], "Favorites");
  assert.equal(gallery.lightboxOpen, false);
  assert.equal(gallery.lastSelectedImage, null);
  assert.equal(gallery.compareOpen, false);
  assert.equal(gallery.comparePin, null);
  assert.ok(!calls.revoked.includes(unsaved.url));
  await gallery.loadFromDisk();
  await gallery.loadFromDisk();
  await gallery.refresh();
  assert.equal(gallery.images.length, 3, "Repeated loads and refreshes never duplicate files");
  disk.entries = [];
  await gallery.refresh();
  assert.deepEqual(names(gallery), ["unsaved.png"], "Empty folders remove stale saved entries");
  assert.deepEqual(Array.from(gallery.sessionImages), [unsaved]);
}
console.log("PASS: empty gallery, restored files, removals, metadata, cache URLs, session objects, boards and repeated loads");

{
  const { gallery, disk, calls } = harness();
  disk.entries = [entry("no-metadata.png")];
  await gallery.loadFromDisk();
  await gallery.hydrateMetadataInBackground();
  assert.equal(calls.metadata.length, 1);
  disk.metadata.set("no-metadata.png", { positive_prompt: "recovered prompt" });
  await gallery.refresh();
  assert.equal(gallery.images[0].metadata.positive_prompt, "recovered prompt", "Retry earlier metadata misses");
  const previous = gallery.images;
  disk.fail = true;
  await gallery.refresh();
  assert.equal(gallery.images, previous, "A failed listing cannot empty the gallery");
  assert.equal(gallery.toast.message, "gallery.toast.refresh_failed");
  assert.equal(gallery.loading, false);
  assert.equal(gallery.refreshing, false);
  disk.fail = false;
  await gallery.refresh();
  assert.equal(gallery.toast.message, "gallery.toast.refreshed");
}
console.log("PASS: metadata retry, failed listing preserves state, and later refresh recovers");

{
  const { gallery, disk, calls } = harness();
  disk.entries = [entry("delete.png")];
  await gallery.loadFromDisk();
  await gallery.hydrateMetadataInBackground();
  disk.listGate = deferred();
  const refresh = gallery.refresh();
  await tick();
  const count = calls.lists;
  await gallery.refresh();
  assert.equal(calls.lists, count, "Ignore repeated clicks while refreshing");
  await gallery.deleteImage(gallery.images[0]);
  const generated = output("generated.png", true);
  const unsaved = output("pending.png");
  gallery.images = [generated, unsaved, ...gallery.images];
  gallery.sessionImages = [generated, unsaved];
  disk.listGate.resolve();
  await refresh;
  assert.deepEqual(names(gallery), ["generated.png", "pending.png"], "Do not resurrect deleted files or drop new results");
  assert.equal(gallery.images[0], generated);
}
console.log("PASS: concurrent generation, deletion and repeated refresh clicks");

{
  const { gallery, disk, calls } = harness();
  disk.entries = [entry("first.png")];
  disk.listGate = deferred();
  const first = gallery.loadFromDisk();
  await tick();
  const second = gallery.loadFromDisk();
  await tick();
  assert.equal(calls.lists, 1, "Directory scans are serialized");
  disk.entries = [entry("second.png")];
  const gate = disk.listGate;
  disk.listGate = null;
  gate.resolve();
  await Promise.all([first, second]);
  await gallery.hydrateMetadataInBackground();
  assert.deepEqual(names(gallery), ["second.png"], "A queued load reads the latest directory state");
}
console.log("PASS: overlapping directory loads cannot commit stale snapshots");

{
  const { gallery, disk } = harness();
  disk.entries = [entry("metadata.png")];
  disk.metadata.set("metadata.png", { positive_prompt: "old prompt" });
  disk.metadataGate = deferred();
  await gallery.loadFromDisk();
  await tick();
  const refresh = gallery.refresh();
  await tick();
  disk.metadata.set("metadata.png", { positive_prompt: "new prompt" });
  const gate = disk.metadataGate;
  disk.metadataGate = null;
  gate.resolve();
  await refresh;
  assert.equal(gallery.images[0].metadata.positive_prompt, "new prompt", "Old hydration cannot overwrite refreshed metadata");
}
console.log("PASS: refresh waits for earlier metadata reads before reloading");

{
  const { gallery, disk } = harness();
  const saving = output("pending.png");
  gallery.images = [saving];
  gallery.sessionImages = [saving];
  disk.saveGate = deferred();
  const save = gallery.persistImages([saving], undefined, [new Blob(["synthetic image"])]);
  disk.entries = [entry("saved.png")];
  const refresh = gallery.refresh();
  await tick();
  disk.saveGate.resolve();
  await Promise.all([save, refresh]);
  assert.deepEqual(names(gallery), ["saved.png"]);
  assert.equal(gallery.images[0], saving, "An in-flight save and its listed file become one live object");
  assert.equal(gallery.sessionImages[0], saving);
}
console.log("PASS: a file discovered before its save response does not duplicate the session image");

{
  const { gallery, disk } = harness(true);
  disk.entries = [entry("desktop copy.jxl")];
  disk.metadata.set("desktop copy.jxl", { positive_prompt: "desktop metadata" });
  await gallery.refresh();
  assert.match(gallery.images[0].thumbnailUrl, /^https:\/\/thumbnail.localhost\/desktop%20copy.jxl\?v=/);
  assert.match(gallery.images[0].fullImageUrl, /^https:\/\/gallery.localhost\/desktop%20copy.jxl\?v=/);
  assert.equal(gallery.images[0].metadata.positive_prompt, "desktop metadata");
  await gallery.refresh();
  assert.equal(gallery.images.length, 1);
  disk.entries = [];
  await gallery.refresh();
  assert.equal(gallery.images.length, 0);
}
console.log("PASS: desktop protocol URLs, restored metadata, repeated refresh and removal");
