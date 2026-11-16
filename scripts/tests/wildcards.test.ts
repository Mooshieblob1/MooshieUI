import assert from "node:assert/strict";
import test from "node:test";
import {
  expandWildcards,
  findWildcardTokens,
  lookupWildcard,
  parseSimpleYaml,
  parseWildcardFiles,
  seededRandom,
} from "../../src/lib/utils/wildcards.ts";

const files = [
  { path: "colors.txt", content: "# a comment\nred\n\n blue \ngreen" },
  { path: "Hair/Styles.txt", content: "bob cut\nponytail" },
  {
    path: "pack.yaml",
    content: [
      "outfits:",
      "  casual:",
      "    - hoodie",
      "    - \"jeans, t-shirt\"  # inline comment",
      "  formal: [suit, 'evening gown']",
      "weather:",
      "- rain",
      "- snow",
    ].join("\n"),
  },
  { path: "nested.json", content: '{"pose": {"standing": ["contrapposto"], "sitting": ["seiza", "on chair"]}}' },
];

test("txt files: one option per line, comments and blanks dropped, paths lowercased", () => {
  const table = parseWildcardFiles(files);
  assert.deepEqual(table.get("colors"), ["red", "blue", "green"]);
  assert.deepEqual(table.get("hair/styles"), ["bob cut", "ponytail"]);
});

test("yaml maps register bare keys and file-prefixed keys, and a map name pools its leaves", () => {
  const table = parseWildcardFiles(files);
  assert.deepEqual(table.get("outfits/casual"), ["hoodie", "jeans, t-shirt"]);
  assert.deepEqual(table.get("pack/outfits/formal"), ["suit", "evening gown"]);
  assert.deepEqual(table.get("outfits"), ["hoodie", "jeans, t-shirt", "suit", "evening gown"]);
  assert.deepEqual(table.get("weather"), ["rain", "snow"]);
  assert.deepEqual(table.get("pose/sitting"), ["seiza", "on chair"]);
});

test("the YAML reader handles nested lists of maps without crashing", () => {
  assert.deepEqual(parseSimpleYaml("a:\n  - x\n  -\n    b: c\n"), { a: ["x", { b: "c" }] });
  assert.equal(parseSimpleYaml(""), null);
});

test("tokens are found and globbing pools matches", () => {
  assert.deepEqual(
    findWildcardTokens("1girl, __Hair/Styles__, [__colors__]").map((t) => t.name),
    ["hair/styles", "colors"],
  );
  const table = parseWildcardFiles(files);
  assert.deepEqual(lookupWildcard(table, "pose/*"), ["contrapposto", "seiza", "on chair"]);
});

test("a fixed seed repeats its picks, and brackets around a token survive", () => {
  const table = parseWildcardFiles(files);
  const a = expandWildcards("[__colors__], __hair/styles__", table, { random: seededRandom(42) });
  const b = expandWildcards("[__colors__], __hair/styles__", table, { random: seededRandom(42) });
  assert.equal(a.text, b.text);
  assert.match(a.text, /^\[(red|blue|green)\], (bob cut|ponytail)$/);
});

test("unknown names stay as written and are reported", () => {
  const r = expandWildcards("__nope__, __colors__", parseWildcardFiles(files), { random: () => 0 });
  assert.equal(r.text, "__nope__, red");
  assert.deepEqual(r.unknown, ["nope"]);
});

test("in-order names follow the counter, not the seed, and are reported for advancing", () => {
  const table = parseWildcardFiles(files);
  const ordered = new Set(["colors"]);
  const picks = [0, 1, 2, 3].map(
    (n) => expandWildcards("__colors__", table, { random: seededRandom(7), ordered, counters: { colors: n } }).text,
  );
  assert.deepEqual(picks, ["red", "blue", "green", "red"]);
  const r = expandWildcards("__colors__ and __colors__", table, { random: () => 0.9, ordered, counters: { colors: 1 } });
  assert.equal(r.text, "blue and blue");
  assert.deepEqual(r.orderedUsed, ["colors"]);
});

test("nested wildcards expand, and a self-reference stops at the depth limit", () => {
  const table = parseWildcardFiles([
    { path: "outer.txt", content: "__colors__ dress" },
    { path: "colors.txt", content: "red" },
    { path: "loop.txt", content: "__loop__" },
  ]);
  assert.equal(expandWildcards("__outer__", table, { random: () => 0 }).text, "red dress");
  assert.equal(expandWildcards("__loop__", table, { random: () => 0, maxDepth: 3 }).text, "__loop__");
});

test("NovelAI weight syntax and plain underscores are not tokens", () => {
  assert.deepEqual(findWildcardTokens("1.2::rain::, o_o, ^_^, snake_case_tag"), []);
});
