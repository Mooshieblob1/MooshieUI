import assert from "node:assert/strict";
import test from "node:test";
import { detectCharacters, normalizeTag, prepareCards, promptTags, type DetectCard } from "../../src/lib/utils/galleryCharacters.ts";

const card = (id: string, name: string, prompt: string, parentId: string | null = null): DetectCard => ({ id, name, prompt, parentId });
const meta = (positive_prompt: string, boxes?: string[]) => ({
  positive_prompt,
  ...(boxes ? { mooshie_novelai_characters: JSON.stringify(boxes.map((prompt) => ({ prompt, enabled: true }))) } : {}),
});

const yuu = card("yuu", "Yuu", "1boy, short black hair, messy hair, green eyes, glasses, grey hoodie");
const ani = card("ani", "Ani", "1girl, long blonde hair, twintails, blue eyes, fang, school uniform");
const aniSwim = card("ani-swim", "Ani (swimsuit)", "1girl, long blonde hair, twintails, blue eyes, fang, bikini, beach", "ani");
const miku = card("miku", "Hatsune Miku", "1girl, hatsune miku, aqua hair, twintails, aqua eyes");
const cards = prepareCards([yuu, ani, aniSwim, miku]);
const ids = (m: Record<string, string>) => detectCharacters(m, cards).map((hit) => hit.characterId);

test("tags are normalized: weights, brackets, underscores, NovelAI emphasis, LoRA syntax", () => {
  assert.equal(normalizeTag("(Blue_Hair:1.2)"), "blue hair");
  assert.equal(normalizeTag("{{twintails}}"), "twintails");
  assert.equal(normalizeTag("1.3::green eyes::"), "green eyes");
  assert.equal(normalizeTag("hatsune miku \\(vocaloid\\)"), "hatsune miku (vocaloid)");
  assert.deepEqual(promptTags("1girl, <lora:x:0.8>, solo\nred_eyes | smile"), ["1girl", "solo", "red eyes", "smile"]);
});

test("an image with the character's identity tags matches, whatever the outfit", () => {
  assert.deepEqual(ids(meta("masterpiece, 1girl, solo, long blonde hair, twintails, blue eyes, fang, maid, kitchen")), ["ani"]);
});

test("the gate: no girl tag at all means a 1girl character is skipped outright", () => {
  assert.deepEqual(ids(meta("long blonde hair, twintails, blue eyes, fang, school uniform")), []);
  assert.deepEqual(ids(meta("1boy, long blonde hair, twintails, blue eyes, fang, school uniform")), []);
});

test("a group shot still passes the gate", () => {
  assert.deepEqual(ids(meta("2girls, long blonde hair, twintails, blue eyes, fang, school uniform")), ["ani"]);
});

test("sharing one or two traits is not a match", () => {
  assert.deepEqual(ids(meta("1girl, long blonde hair, red eyes, armor")), []);
  assert.deepEqual(ids(meta("1girl, twintails, blue eyes, pink hair")), []);
});

test("naming the character matches even when the look is not described", () => {
  assert.deepEqual(ids(meta("1girl, hatsune miku, maid, kitchen")), ["miku"]);
  assert.deepEqual(ids(meta("1girl, hatsune_miku \\(vocaloid\\), swimsuit")), ["miku"]);
  // The name alone does not pass the gate.
  assert.deepEqual(ids(meta("1boy, hatsune miku cosplay")), []);
});

test("a short one-word name is not trusted as a name tag", () => {
  assert.deepEqual(ids(meta("1boy, yuu, red hair, blue eyes")), []);
});

test("variants report their parent character, and the closest outfit picks the card", () => {
  const hits = detectCharacters(meta("1girl, long blonde hair, twintails, blue eyes, fang, bikini, beach"), cards);
  assert.equal(hits.length, 1);
  assert.equal(hits[0]!.characterId, "ani");
  assert.equal(hits[0]!.cardId, "ani-swim");
  const plain = detectCharacters(meta("1girl, long blonde hair, twintails, blue eyes, fang, school uniform"), cards);
  assert.equal(plain[0]!.cardId, "ani");
});

test("NovelAI character boxes are matched one by one", () => {
  const image = meta("1girl, 1boy, park, sunset", [
    "girl, long blonde hair, twintails, blue eyes, fang, sundress",
    "boy, short black hair, messy hair, green eyes, glasses, t-shirt",
  ]);
  assert.deepEqual(ids(image).sort(), ["ani", "yuu"]);
});

test("traits split across two boxes do not add up to one character", () => {
  const image = meta("2girls, park", ["girl, long blonde hair, twintails, red eyes", "girl, black hair, blue eyes, fang"]);
  assert.deepEqual(ids(image), []);
});

test("prose prompts match on whole words", () => {
  assert.deepEqual(
    ids(meta("A photo of 1girl with long blonde hair in twintails, blue eyes and a small fang, wearing a school uniform")),
    ["ani"],
  );
});

test("images without a prompt, and cards too thin to tell apart, match nothing", () => {
  assert.deepEqual(ids({}), []);
  assert.deepEqual(detectCharacters(meta("1girl, blue hair"), prepareCards([card("x", "X", "1girl, blue hair")])), []);
});
