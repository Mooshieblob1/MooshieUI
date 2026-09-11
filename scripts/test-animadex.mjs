// Focused regression checks for issue #686. Run: node scripts/test-animadex.mjs
// Compile the real modules and stub their network/persistence boundaries; no app
// runtime or frontend test framework is needed for these synchronous store flows.
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import ts from "typescript";

const root = new URL("../", import.meta.url);

function loadSource(path, imports, globals = {}) {
  const source = fs.readFileSync(new URL(path, root), "utf8");
  const { outputText } = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  });
  const module = { exports: {} };
  vm.runInNewContext(outputText, {
    module,
    exports: module.exports,
    require(name) {
      assert.ok(Object.hasOwn(imports, name), `Unexpected import: ${name}`);
      return imports[name];
    },
    URLSearchParams, Response, DOMException, setTimeout, clearTimeout,
    ...globals,
  }, { filename: path });
  return module.exports;
}

// Names/counts from the reported Animadex "aimi" search; appearance tags shortened.
const aimi = {
  name: "Aimi (Sky-freedom)", slug: "aimi_(sky-freedom)",
  trigger: "aimi (sky-freedom), original", copyright: "original",
  copyright_name: "Original", count: 48, tags: ["1girl", "purple eyes", "black hair"],
  has_image: true, loras: [], url: "", thumb_url: "", img_url: "",
};
const yamasaka = {
  ...aimi, name: "Yamasaka Aimi", slug: "yamasaka_aimi",
  trigger: "yamasaka aimi, cookie (touhou)", copyright: "cookie_(touhou)", count: 43,
};
let upstream = { total: 2, page: 1, page_size: 60, pages: 1, results: [aimi, yamasaka] };
const client = loadSource("src/lib/animadex/client.ts", {
  "./fetch.js": { ANIMADEX_ORIGIN: "https://animadex.net/", animadexFetch: undefined },
  "./types.js": loadSource("src/lib/animadex/types.ts", {}),
}, { fetch: async () => new Response(JSON.stringify(upstream)) });

const search = await client.searchCharacters({ q: "aimi" });
assert.equal(search.total, 2);
assert.equal(search.results.length, 2);
assert.equal(search.results[0].slug, aimi.slug);
assert.equal(search.results[1].slug, yamasaka.slug);

upstream = {
  total: 121, page: 2, page_size: 60, pages: 3,
  results: [
    { ...aimi, name: "Muto", slug: "muto", trigger: "muto, original", count: 40 },
    { ...aimi, name: "Mutou", slug: "mutou", trigger: "mutou, original", count: 48 },
    { ...yamasaka, count: 0 },
  ],
};
const aliases = await client.searchCharacters({ q: "aliases", page: 2 });
assert.equal(aliases.results.length, 2, "Keep alias deduplication and zero-count characters");
assert.equal(aliases.results[0].slug, "mutou", "Keep the stronger alias");
assert.equal(aliases.results[1].count, 0);
assert.equal(aliases.total, 121, "Do not substitute a page length for the catalog total");
assert.equal(aliases.page, 2);
assert.equal(aliases.pages, 3);
console.log("PASS search: reported low counts, zero counts, aliases, pagination");

const insertion = loadSource("src/lib/animadex/characterInsert.ts", {
  "../stores/generation.svelte.js": { DEFAULT_ANIMA_POSITIVE_QUALITY: "best quality, highres" },
});

function createStore(prompt, character = aimi) {
  const generation = { positivePrompt: prompt, saves: 0, saveSettings() { this.saves++; } };
  const { characterInsert } = loadSource("src/lib/stores/characterInsert.svelte.ts", {
    "../animadex/characterInsert.js": insertion,
    "./generation.svelte.js": { generation },
  }, { $state: (value) => value });
  characterInsert.request(character);
  return { generation, store: characterInsert };
}

function parts(prompt) {
  return prompt.split(",").map((part) => part.trim()).filter(Boolean);
}

for (const level of ["name", "name_copyright", "all"]) {
  const { generation, store } = createStore("");
  const preview = store.previewInsert(level);
  store.chooseTagLevel(level);
  assert.equal(generation.positivePrompt, preview, `${level}: insert only the selected tags`);
  assert.equal(store.pending, null);
  assert.equal(generation.saves, 1);

  for (const prompt of ["best quality, highres", "1girl, purple eyes, black hair", "1girl, solo, best quality, highres"]) {
    const { generation, store } = createStore(prompt);
    store.chooseTagLevel(level);
    assert.equal(store.pending, null, `${level}: automatic insertion completes`);
    const result = parts(generation.positivePrompt);
    assert.ok(!result.includes("2girls") && !result.includes("multiple girls"));
    for (const part of parts(prompt)) assert.ok(result.includes(part), `Preserve ${part}`);
    assert.ok(result.filter((part) => part === "1girl").length <= 1);
  }
}

const male = { ...aimi, name: "Example Boy", slug: "example_boy", trigger: "example boy, original", tags: ["1boy", "black hair"] };
const maleInsert = createStore("", male);
maleInsert.store.chooseTagLevel("all");
assert.ok(parts(maleInsert.generation.positivePrompt).includes("1boy"));
assert.ok(!/\dgirls?/.test(maleInsert.generation.positivePrompt), "Do not invent female subjects");
console.log("PASS insertion: all tag levels, empty/quality/appearance prompts, existing counts");

for (const prompt of ["1girl, solo", "1girl, solo, old character, old series", "2girls, multiple girls, old character"]) {
  const { generation, store } = createStore(prompt, { ...aimi, tags: [...aimi.tags, "solo"] });
  store.chooseTagLevel("all");
  assert.equal(store.pending.step, "pick_action", "Existing character retains explicit add/replace choice");
  const expectedCount = prompt.startsWith("2girls") ? "3girls" : "2girls";
  assert.equal(store.previewGirlCountAfterAdd(), expectedCount);
  store.apply("all", "add");
  const result = parts(generation.positivePrompt);
  assert.ok(result.includes(expectedCount));
  assert.equal(result.filter((part) => /^\d+\+?girls?$/.test(part)).length, 1);
  assert.equal(result.filter((part) => part === "multiple girls").length, 1);
  assert.ok(!result.includes("solo"));
}

const replacement = createStore("1girl, solo, old character, old series");
replacement.store.chooseTagLevel("all");
replacement.store.apply("all", "replace");
const replaced = parts(replacement.generation.positivePrompt);
assert.equal(replaced.filter((part) => part === "1girl").length, 1);
assert.ok(replaced.includes("solo"));
assert.ok(!replaced.includes("old character") && !replaced.includes("old series"));

const duplicate = createStore("aimi (sky-freedom), original, 1girl");
assert.equal(duplicate.store.pending.step, "duplicate");
assert.equal(duplicate.generation.saves, 0);
console.log("PASS explicit add, replacement, composition conflicts, duplicate detection");
