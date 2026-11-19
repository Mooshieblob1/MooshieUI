import assert from "node:assert/strict";
import test from "node:test";
import {
  characterExtractRequest,
  groundExtracted,
  isTagPrompt,
  relevantSavedHints,
  type ExtractedCharacter,
} from "../../src/lib/utils/characterExtract.ts";
import { findDuplicate } from "../../src/lib/utils/characterDuplicates.ts";

const found = (name: string, prompt: string, negative = "", sameAs = ""): ExtractedCharacter => ({
  name,
  prompt,
  negative,
  sameAs,
  variant: "",
});

test("tag prompts and sentence prompts are told apart", () => {
  assert.ok(isTagPrompt("1girl, long pink hair, blue eyes, school uniform, classroom"));
  assert.ok(isTagPrompt("masterpiece, 2girls\nCharacter 1: girl, red hair, fang\nCharacter 2: girl, blue hair"));
  assert.ok(!isTagPrompt("A tall knight with silver hair stands in the rain, holding a lantern, looking at the sky."));
});

test("invented tags, counts, quality and artist tags are dropped from a tag prompt", () => {
  const prompt = "masterpiece, best quality, artist:wlop, 1girl, solo, long pink hair, blue eyes, school uniform, classroom";
  const [ani] = groundExtracted(
    [found("Pink-haired girl", "1girl, long pink hair, blue eyes, school uniform, red ribbon, artist:wlop, masterpiece")],
    prompt,
    "",
  );
  assert.equal(ani.prompt, "long pink hair, blue eyes, school uniform");
});

test("weights, underscores and part of a tag still count as in the prompt", () => {
  const prompt = "1girl, {{long_pink_hair}}, (blue eyes:1.2), 1.3::school uniform::, smile";
  const [ani] = groundExtracted([found("Ani", "long pink hair, blue eyes, school uniform, pink hair")], prompt, "");
  assert.equal(ani.prompt, "long pink hair, blue eyes, school uniform, pink hair");
});

test("a trait the negative prompt rules out never goes into the character prompt", () => {
  const prompt = "1girl, long hair, blue eyes, dress";
  const [ani] = groundExtracted(
    [found("Ani", "long hair, blue eyes, dress, red eyes", "red eyes, lowres, bad anatomy, watermark")],
    prompt,
    "red eyes, lowres, bad anatomy, watermark",
  );
  assert.equal(ani.prompt, "long hair, blue eyes, dress");
  assert.equal(ani.negative, "red eyes");
});

test("tags only another NovelAI character box has are taken off", () => {
  const prompt = "2girls, classroom\nCharacter 1: girl, red hair, fang, hoodie\nCharacter 2: girl, blue hair, glasses, school uniform";
  const result = groundExtracted(
    [
      found("Red-haired girl", "Character 1: girl, red hair, fang, hoodie, glasses"),
      found("Blue-haired girl", "girl, blue hair, glasses, school uniform"),
    ],
    prompt,
    "",
  );
  assert.deepEqual(
    result.map((c) => c.prompt),
    ["girl, red hair, fang, hoodie", "girl, blue hair, glasses, school uniform"],
  );
});

test("a character left with nothing from the prompt is dropped", () => {
  const prompt = "1girl, solo, long hair, red eyes, forest";
  assert.deepEqual(groundExtracted([found("Ghost", "1girl, solo, wings, halo")], prompt, ""), []);
});

test("sentence prompts are passed through untouched", () => {
  const prompt = "A tall knight with silver hair stands in the rain, holding a lantern.";
  const items = [found("Silver-haired knight", "a tall knight with silver hair and a red cape")];
  assert.deepEqual(groundExtracted(items, prompt, ""), items);
});

test("only saved characters the prompt could be about are listed, best first", () => {
  const saved = [
    { name: "Yuu", prompt: "1boy, short black hair, glasses, grey hoodie" },
    { name: "Ani", prompt: "1girl, long blonde hair, twintails, blue eyes, fang" },
    { name: "Hatsune Miku", prompt: "1girl, hatsune miku, aqua hair, twintails" },
    { name: "Unrelated", prompt: "1girl, green hair, witch hat" },
  ];
  const hints = relevantSavedHints("1girl, hatsune miku, twintails, blue eyes, smile", saved);
  assert.deepEqual(
    hints.map((h) => h.name),
    ["Hatsune Miku", "Ani"],
  );
  const request = characterExtractRequest("1girl, hatsune miku, twintails, blue eyes, smile", "", saved);
  assert.ok(!request.prompt.includes("Unrelated") && !request.prompt.includes("Yuu"));
});

test("a same_as hint needs a shared name or two shared tags to count", () => {
  const saved = [
    { id: "a", name: "Ani", prompt: "1girl, long blonde hair, twintails, blue eyes, fang", negative: "" },
    { id: "m", name: "Hatsune Miku", prompt: "1girl, hatsune miku, aqua hair, twintails, aqua eyes", negative: "" },
  ];
  // Only "1girl" and twintails in common, and no shared name: not Ani.
  assert.equal(findDuplicate(found("Pink-haired girl", "1girl, pink hair, twintails, red eyes", "", "Ani"), saved), null);
  // Unnamed but drawn the same way: Ani.
  assert.equal(
    findDuplicate(found("Blonde girl", "long blonde hair, twintails, blue eyes, swimsuit", "", "Ani"), saved)?.id,
    "a",
  );
  // A short form of the name still counts.
  assert.equal(findDuplicate(found("Miku", "aqua hair", "", "Hatsune Miku"), saved)?.id, "m");
});
