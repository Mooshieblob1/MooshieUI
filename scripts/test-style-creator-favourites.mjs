// Exercise production save methods and persistence with synthetic rounds.
// No generation requests, browser image decoding, or live user preferences.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import ts from "typescript";

const storage = new Map();
const localStorage = {
  getItem: key => storage.get(key) ?? null,
  setItem: (key, value) => storage.set(key, value),
};
const resizeCalls = [], editorCalls = [], toasts = [];
let syncCalls = 0;
const gallery = {
  artistTagIndex: new Map(),
  showToast: (...args) => toasts.push(args),
  getPersistPromise: async image => image.gallery_filename,
};
const stubs = new Map(Object.entries({
  "src/lib/stores/generation.svelte.ts": { generation: {} },
  "src/lib/stores/gallery.svelte.ts": { gallery },
  "src/lib/stores/locale.svelte.ts": { locale: { t: (key, args) => ({ key, args }) } },
  "src/lib/stores/styleEditors.svelte.ts": { styleEditors: { openStyle: id => editorCalls.push(id) } },
  "src/lib/stores/artistInsert.svelte.ts": { artistInsert: {} },
  "src/lib/utils/syncTrigger.ts": { triggerSync: () => syncCalls++ },
  "src/lib/utils/generationSubmit.ts": { submitGeneration: () => assert.fail("Unexpected generation") },
  "src/lib/utils/generationErrors.ts": {},
}));
const cache = new Map();
function load(file, fresh = false) {
  if (stubs.has(file)) return stubs.get(file);
  if (!fresh && cache.has(file)) return cache.get(file);
  const module = { exports: {} };
  const source = ts.transpileModule(fs.readFileSync(file, "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText;
  cache.set(file, module.exports);
  vm.runInNewContext(source, {
    module, exports: module.exports, console, localStorage,
    // These tests call imperative store methods; rendering is checked by build.
    $state: value => value,
    require: name => load(path.posix.normalize(path.posix.join(path.posix.dirname(file), name)).replace(/\.js$/, ".ts")),
  }, { filename: file });
  return module.exports;
}

const tagUtils = load("src/lib/utils/artistTag.ts");
const stylesModule = load("src/lib/stores/styles.svelte.ts");
stylesModule.resizeImageToDataUrl = async source => {
  resizeCalls.push(source);
  return "data:image/jpeg;base64,synthetic";
};
const { styles } = stylesModule;
const { artistFavourites } = load("src/lib/artist-gallery/favourites.svelte.ts");
const { styleCreator } = load("src/lib/stores/styleCreator.svelte.ts");
const artist = (slug, weight = 1) => ({ tag: `@${slug}`, slug, weight });
const card = (artists, key = artists.map(a => a.slug ?? a.tag).join("+")) => ({
  key, artists, name: "Chosen style", saveable: true,
  image: { url: `blob:${key}`, gallery_filename: `${key}.jxl` },
});
function round(cards) {
  styleCreator.phase = "choosing";
  styleCreator.round = { seed: "42", cards };
  styleCreator.viewerOpen = true;
}

// Both candidates are judged, but only the selected artist is favourited.
round([card([artist("first", 0.7)]), card([artist("second")])]);
await styleCreator.pick(0, true);
assert.equal(artistFavourites.isFavourite("first"), true);
assert.equal(artistFavourites.isFavourite("second"), false);
assert.equal(styles.styles.length, 0);
assert.equal(resizeCalls.length, 0, "Single picks must never copy the generated image");
assert.equal(editorCalls.length, 0, "Single picks must never open a style editor");
assert.equal(styleCreator.history.has("first"), true);
assert.equal(styleCreator.history.has("second"), true);
assert.equal(styleCreator.round, null);
assert.equal(styleCreator.viewerOpen, false);
assert.equal(toasts.at(-1)[0].key, "style_creator.saved_artists");
assert.equal(syncCalls > 0, true, "Favourites use existing preference sync");

// Re-favouriting is additive and preserves the existing category and timestamp.
const category = artistFavourites.createCategory("Keep", "#6366f1");
artistFavourites.setCategory("first", category.id);
const existing = JSON.stringify(artistFavourites.favourites.first);
round([card([artist("first")]), card([artist("second")])]);
await styleCreator.pickAll();
assert.equal(JSON.stringify(artistFavourites.favourites.first), existing);
assert.equal(artistFavourites.isFavourite("second"), true);
assert.equal(styles.styles.length, 0);
assert.equal(resizeCalls.length, 0);
await styleCreator.pickAll();
assert.equal(artistFavourites.count, 2, "Repeated save after closing the round is harmless");
const persisted = JSON.parse(storage.get("mooshieui.artist-gallery.favourites.v1"));
assert.deepEqual(Object.keys(persisted.favourites[0]).sort(), ["addedAt", "categoryId", "slug"]);
const reloaded = load("src/lib/artist-gallery/favourites.svelte.ts", true).artistFavourites;
assert.equal(reloaded.isFavourite("first"), true);
assert.equal(reloaded.categoryOf("first").id, category.id);

// One extra artist with an anchor is still a multi-artist style, with its image.
styleCreator.count = 1;
styleCreator.anchor = [artist("anchor")];
round([card([artist("anchor"), artist("extra", 1.3)])]);
await styleCreator.pick(0, true);
assert.equal(styles.styles.length, 1);
assert.equal(styles.styles[0].artists[1].weight, 1.3);
assert.equal(styles.styles[0].thumbnail, "data:image/jpeg;base64,synthetic");
assert.equal(styles.styles[0].thumbnailImage, "anchor+extra.jxl");
assert.equal(editorCalls[0], styles.styles[0].id);
assert.equal(artistFavourites.isFavourite("extra"), false);

// Save both keeps multi-artist behaviour too; incomplete cards are not saved.
round([card([artist("a"), artist("b")]), card([artist("c"), artist("d")])]);
await styleCreator.pickAll();
assert.equal(styles.styles.length, 3);
assert.equal(resizeCalls.length, 3);
assert.equal(toasts.at(-1)[0].key, "style_creator.saved_count");
const reference = { ...card([artist("reference")]), key: "", saveable: false };
round([reference, { ...card([artist("pending")]), image: null }]);
await styleCreator.pickAll();
assert.equal(styleCreator.phase, "choosing");
await styleCreator.pick(0);
assert.equal(artistFavourites.isFavourite("reference"), false);
assert.equal(artistFavourites.isFavourite("pending"), false);
assert.equal(styles.styles.length, 3);

// Resolve tag-only imports to the actual CDN slug, including escaped punctuation.
const hit = { tag: "@artist_(name)", slug: "artist_name", hasImage: true };
gallery.artistTagIndex.set("artist_(name)", hit);
const imported = [{ tag: "@Artist \\(Name\\)", weight: 1 }];
assert.equal(tagUtils.singleArtistSlug(imported, gallery.artistTagIndex), hit.slug);
assert.equal(tagUtils.singleArtistSlug([{ tag: "unknown" }], gallery.artistTagIndex), null);
assert.equal(tagUtils.singleArtistSlug([], gallery.artistTagIndex), null);
assert.equal(tagUtils.singleArtistSlug([artist("a"), artist("b")], gallery.artistTagIndex), null);
assert.equal(tagUtils.singleArtistSlug([artist("known")], new Map()), "known");
round([card(imported)]);
// A round save finishes synchronously before the next round is scheduled.
styleCreator.running = true;
let nextRounds = 0;
styleCreator.nextRound = () => {
  nextRounds++;
  assert.equal(artistFavourites.isFavourite(hit.slug), true);
  assert.equal(styleCreator.round, null);
};
await styleCreator.pick(0);
assert.equal(nextRounds, 1);
assert.equal(styles.styles.length, 3);
assert.equal(resizeCalls.length, 3);

console.log("PASS: single/both favourites, category and persistence, no generated thumbnail, anchor combinations, multi-style thumbnails, reference cards, imported tag resolution and round continuation");
