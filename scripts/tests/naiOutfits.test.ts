import assert from "node:assert/strict";
import test from "node:test";
import {
  normalizeNaiResponse,
  parseNaiResponse,
  validateNaiResponse,
} from "../../src/lib/utils/naiParse.ts";
import { naiRewriteSystemPrompt } from "../../src/lib/utils/naiPrompt.ts";
import {
  findOutfitOverlaps,
  hasOppositeOutfitUc,
  isolateOutfits,
  keepTagIn,
  outfitTags,
  removeTag,
  similarBodyPairs,
  tagKind,
  withOppositeOutfitUc,
} from "../../src/lib/utils/naiOutfits.ts";

const parse = (raw: string) => normalizeNaiResponse(parseNaiResponse(raw));

test("tagKind tells clothing from hair, body and actions", () => {
  assert.equal(tagKind("white hoodie"), "clothing");
  assert.equal(tagKind("ribbon (hair)"), "clothing");
  assert.equal(tagKind("sportswear"), "clothing");
  assert.equal(tagKind("skirt lift"), "other");
  assert.equal(tagKind("girl on top"), "other");
  assert.equal(tagKind("silver hair"), "hair");
  assert.equal(tagKind("twintails"), "hair");
  assert.equal(tagKind("blue eyes"), "body");
  assert.equal(tagKind("closed eyes"), "other");
  assert.equal(tagKind("source#hug"), "other");
});

test("outfitTags reads tags through emphasis and skips negative spans", () => {
  assert.deepEqual(
    outfitTags("girl, black hair,\n{white hoodie}, 1.2::blue jeans::, -1::hat::, standing"),
    ["white hoodie", "blue jeans"],
  );
});

test("CHAR n UC lines are parsed per box", () => {
  const p = parse(`BASE:
2girls, park

UC:
extra arms

CHAR 1:
girl, white hoodie

CHAR 1 UC:
school uniform

CHAR 2:
girl, school uniform

CHAR 2 UC:
none`);
  assert.deepEqual(p.characters, ["girl, white hoodie", "girl, school uniform"]);
  assert.deepEqual(p.characterUcs, ["school uniform", ""]);
  assert.equal(p.uc, "extra arms");
});

test("a box with no UC line has a null UC, and old answers still parse", () => {
  const p = parse("BASE:\n1girl, park\n\nCHAR 1:\ngirl, red coat");
  assert.deepEqual(p.characterUcs, [null]);
});

test("opposite-outfit UC adds the other boxes' distinctive garments", () => {
  const ucs = withOppositeOutfitUc(
    ["girl, white hoodie, blue jeans", "girl, navy school uniform, pleated skirt"],
    ["", "extra arms"],
  );
  assert.deepEqual(ucs, ["navy school uniform, pleated skirt", "extra arms, white hoodie, blue jeans"]);
});

test("opposite-outfit UC never fights a garment the box wears", () => {
  // `hoodie` would erase her own black hoodie; `white hoodie` keeps them apart.
  const ucs = withOppositeOutfitUc(["girl, black hoodie", "girl, white hoodie, hoodie"], ["", ""]);
  assert.deepEqual(ucs, ["white hoodie", "black hoodie"]);
});

test("opposite-outfit UC skips what the UC already covers and matching outfits", () => {
  assert.deepEqual(
    withOppositeOutfitUc(["girl, red coat", "girl, navy school uniform"], ["school uniform", ""]),
    ["school uniform", "red coat"],
  );
  assert.deepEqual(withOppositeOutfitUc(["girl, maid", "girl, maid"], ["", ""]), ["", ""]);
  assert.deepEqual(withOppositeOutfitUc(["girl, maid"], [""]), [""]);
});

test("the validator flags outfits restated in the BASE tag line", () => {
  const p = parse(`BASE:
2girls, bus stop, white hoodie

The girl in the white hoodie stands on the left.

CHAR 1:
girl, white hoodie, on the left

CHAR 2:
girl, school uniform, on the right`);
  const problems = validateNaiResponse(p);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /BASE restated .*white hoodie/);
});

test("the placing sentence may name a garment", () => {
  const p = parse(`BASE:
2girls, bus stop

The girl in the white hoodie stands on the left, the girl in the school uniform on the right.

CHAR 1:
girl, white hoodie, on the left

CHAR 2:
girl, school uniform, on the right`);
  assert.deepEqual(validateNaiResponse(p), []);
});

test("the validator wants interaction tags for a described interaction", () => {
  const untagged = parse(`BASE:
2girls, park

The girl on the left hugs the girl on the right.

CHAR 1:
girl, red coat, on the left

CHAR 2:
girl, blue coat, on the right`);
  assert.match(validateNaiResponse(untagged).join("\n"), /interaction \(hugs\)/);

  const unpaired = parse(`BASE:
2girls, park

The girl on the left hugs the girl on the right.

CHAR 1:
girl, red coat, source#hug, on the left

CHAR 2:
girl, blue coat, on the right`);
  assert.match(validateNaiResponse(unpaired).join("\n"), /source#hug had no partner/);
});

test("the spec's own multi-character example passes the validator", () => {
  const system = naiRewriteSystemPrompt({
    variant: "full",
    ucPreset: 0,
    characterCount: 2,
    existing: null,
    language: "en",
    references: [],
  });
  const example = system.match(/MULTI-CHARACTER EXAMPLE[\s\S]*?(BASE:[\s\S]*?CHAR 2 UC:\n[^\n]*)/);
  assert.ok(example, "example block present");
  const p = parse(example[1]);
  assert.equal(p.characters.length, 2);
  assert.deepEqual(validateNaiResponse(p), []);
  assert.ok(p.characterUcs.every((uc) => uc && uc.length > 0));
});

test("removeTag keeps spans and prose intact", () => {
  assert.equal(removeTag("girl, white hoodie, jeans", "white hoodie"), "girl, jeans");
  assert.equal(removeTag("girl, {white hoodie}, jeans", "white hoodie"), "girl, jeans");
  assert.equal(removeTag("girl, 1.2::white hoodie, jeans::, smile", "white hoodie"), "girl, 1.2::jeans::, smile");
  assert.equal(removeTag("girl, 1.2::jeans, white hoodie::, smile", "white hoodie"), "girl, 1.2::jeans::, smile");
  assert.equal(removeTag("girl, -1::white hoodie::, jeans", "white hoodie"), "girl, -1::white hoodie::, jeans");
  assert.equal(removeTag("girl,\nwhite hoodie, jeans,\nsmile", "white hoodie"), "girl,\njeans,\nsmile");
  assert.equal(
    removeTag("2girls, park, white hoodie\n\nThe girl in the white hoodie waves.", "white hoodie"),
    "2girls, park\n\nThe girl in the white hoodie waves.",
  );
});

test("findOutfitOverlaps finds base restatements and shared garments, not shared hair", () => {
  const overlaps = findOutfitOverlaps("2girls, park, white hoodie", [
    "girl, long hair, white hoodie, red scarf",
    "girl, long hair, red scarf, jeans",
  ]);
  assert.deepEqual(
    overlaps.map((o) => [o.tag, o.inBase, o.boxes]),
    [
      ["white hoodie", true, [0]],
      ["red scarf", false, [0, 1]],
    ],
  );
});

test("keepTagIn moves a tag into one box and the others' UC", () => {
  const out = keepTagIn(
    { base: "2girls, red scarf", boxes: ["girl, red scarf", "girl, red scarf, jeans"], ucs: ["", ""] },
    "red scarf",
    0,
  );
  assert.deepEqual(out, { base: "2girls", boxes: ["girl, red scarf", "girl, jeans"], ucs: ["", "red scarf"] });
});

test("isolateOutfits cleans the base, places boxes and adds opposite UC", () => {
  const out = isolateOutfits(
    {
      base: "2girls, park, white hoodie, silver hair\n\nTwo girls talk.",
      boxes: ["girl, silver hair, white hoodie", "girl, black hair, school uniform, on the right"],
      ucs: ["", ""],
    },
    ["on the left", "on the left"],
  );
  assert.deepEqual(out, {
    base: "2girls, park\n\nTwo girls talk.",
    boxes: ["girl, silver hair, white hoodie, on the left", "girl, black hair, school uniform, on the right"],
    ucs: ["school uniform", "white hoodie"],
  });
});

test("similarBodyPairs needs alike bodies and different clothes", () => {
  assert.deepEqual(
    similarBodyPairs(["girl, silver hair, white hoodie", "girl, silver hair, school uniform"]),
    [[0, 1]],
  );
  assert.deepEqual(similarBodyPairs(["girl, silver hair, maid", "girl, silver hair, maid"]), []);
  assert.deepEqual(similarBodyPairs(["girl, silver hair, hoodie", "girl, black hair, dress"]), []);
  assert.equal(hasOppositeOutfitUc(["girl, hoodie", "girl, dress"], ["dress", "hoodie"]), true);
  assert.equal(hasOppositeOutfitUc(["girl, hoodie", "girl, dress"], ["", "hoodie"]), false);
});
