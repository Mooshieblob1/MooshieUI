import assert from "node:assert/strict";
import test from "node:test";
import { getUnknownTagRanges } from "../../src/lib/utils/promptSpellcheck.ts";

// A tiny stand-in for the Danbooru taglist, normalized the way the store does it.
const KNOWN = new Set(["1girl", "2girls", "solo", "smile", "rain", "night", "hat", "silver_hair", "hug", "headpat", "long_hair"]);
const isKnown = (name: string) => KNOWN.has(name.toLowerCase().trim().replace(/\s+/g, "_"));

function flagged(raw: string, dialect: "danbooru" | "novelai" = "danbooru"): string[] {
  return getUnknownTagRanges(raw, isKnown, -1, dialect).map((r) => raw.slice(r.start, r.end));
}

test("plain misspellings are still flagged", () => {
  assert.deepEqual(flagged("1girl, smiel, long hair"), ["smiel"]);
});

test("quality, year and score tokens are known in every dialect", () => {
  assert.deepEqual(flagged("masterpiece, best quality, very aesthetic, year 2024, score_9_up, 1girl"), []);
});

test("prose lines inside a tag prompt are not checked", () => {
  const raw = "2girls, smile\n\nTwo girls wait out the rain under a bus stop, with rain streaking past the open front. Soft grey daylight.";
  assert.deepEqual(flagged(raw), []);
});

test("a long sentence fragment between commas is not checked", () => {
  assert.deepEqual(flagged("1girl, a girl standing alone in the rain, smiel"), ["smiel"]);
});

test("a tag segment that runs across a line break is checked line by line", () => {
  assert.deepEqual(flagged("1girl, solo\nsmiel, rain"), ["smiel"]);
});

test("NovelAI tokens, prefixes and weights are known in the novelai dialect", () => {
  const raw =
    "2girls, high complexity, artist:someone obscure, 1.2::rain, night::, -1::hat::, {{silver hair}}, location, no text, fur dataset\n\ngirl, source#hug, target#headpat, mutual#hug";
  assert.deepEqual(flagged(raw, "novelai"), []);
});

test("the novelai dialect still flags misspellings, including behind a prefix", () => {
  assert.deepEqual(flagged("1girl, smiel, source#hgu, -1::hta::", "novelai"), ["smiel", "source#hgu", "-1::hta::"]);
});

test("NovelAI Text: lettering is not checked", () => {
  assert.deepEqual(flagged("1girl, smile\nText: Welcom to the shopp", "novelai"), []);
});

test("NovelAI-only tokens stay unknown to the danbooru dialect", () => {
  assert.deepEqual(flagged("1girl, high complexity", "danbooru"), ["high complexity"]);
});

test("the caret's piece is never flagged", () => {
  const raw = "1girl, smie";
  assert.deepEqual(getUnknownTagRanges(raw, isKnown, raw.length, "danbooru"), []);
});
