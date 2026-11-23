import assert from "node:assert/strict";
import test from "node:test";
import { isTagPrompt } from "../../src/lib/utils/characterExtract.ts";
import {
  SAMPLE_ARCHITECTURES,
  SAMPLE_NATURAL_ARCHITECTURES,
  YUE_NATURAL,
  sampleCardId,
  samplePrompt,
  withSamples,
} from "../../src/lib/utils/sampleCharacters.ts";

const pictures = {
  yue: { thumbnail: "data:image/webp;base64,AA", reference: "data:image/webp;base64,BB" },
  "yue-natural": { thumbnail: "data:image/webp;base64,AA", reference: null },
};

test("a new user gets Yue once in every tag and prose architecture, with her notes", () => {
  const out = withSamples([], [], pictures, 1);
  assert.equal(out.changed, true);
  assert.deepEqual(out.seen, ["yue", "yue-natural"]);
  assert.equal(out.characters.length, SAMPLE_ARCHITECTURES.length + SAMPLE_NATURAL_ARCHITECTURES.length);

  const nai = out.characters.find((c) => c.architecture === "novelai")!;
  assert.equal(nai.id, sampleCardId("yue", "novelai"));
  assert.ok(nai.prompt.startsWith("girl, yue,"));
  assert.equal(nai.thumbnail, pictures.yue.thumbnail);
  assert.equal(nai.reference, pictures.yue.reference);

  const sdxl = out.characters.find((c) => c.architecture === "sdxl")!;
  assert.ok(sdxl.prompt.startsWith("1girl, yue,"));
  assert.match((nai as { memory: { content: string }[] }).memory[0].content, /Usual expression: smiling/);

  for (const arch of ["flux1d", "flux2d", "krea2", "qwen", "qwenimage21", "mossimage", "mingimage"]) {
    const card = out.characters.find((c) => c.architecture === arch);
    assert.ok(card, `expected natural-language Yue card for ${arch}`);
    assert.equal(card.id, sampleCardId("yue-natural", arch));
    assert.equal(card.prompt, YUE_NATURAL.prompt);
    assert.equal(isTagPrompt(card.prompt), false);
    assert.equal(card.thumbnail, pictures["yue-natural"].thumbnail);
    assert.match((card as { memory: { content: string }[] }).memory[0].content, /Usual expression: smiling/);
  }
});

test("existing users who already saw tag Yue get natural Yue once without re-adding deleted tag Yue", () => {
  const out = withSamples([], ["yue"], pictures, 1);
  assert.equal(out.changed, true);
  assert.deepEqual(out.seen, ["yue", "yue-natural"]);
  assert.equal(out.characters.length, SAMPLE_NATURAL_ARCHITECTURES.length);
  assert.equal(out.characters.some((c) => c.architecture === "novelai"), false);
});

test("once seen, deleted Yue cards are not added again", () => {
  const out = withSamples([], ["yue", "yue-natural"], pictures, 1);
  assert.equal(out.changed, false);
  assert.equal(out.characters.length, 0);
});

test("an architecture that already has a Yue gets no second card", () => {
  const mine = { id: "x", name: " yue ", architecture: "novelai" };
  const synced = { id: sampleCardId("yue", "sdxl"), name: "Yue", architecture: "sdxl" };
  const mineFlux = { id: "y", name: "Yue", architecture: "flux1d" };
  const out = withSamples([mine, synced, mineFlux], [], pictures, 1);
  assert.equal(out.characters.filter((c) => c.architecture === "novelai").length, 1);
  assert.equal(out.characters.filter((c) => c.architecture === "sdxl").length, 1);
  assert.equal(out.characters.filter((c) => c.architecture === "flux1d").length, 1);
  assert.equal(out.characters.length, SAMPLE_ARCHITECTURES.length + SAMPLE_NATURAL_ARCHITECTURES.length);
});

test("only a leading girl becomes 1girl", () => {
  assert.equal(samplePrompt("girl, yue, girl", "pony"), "1girl, yue, girl");
  assert.equal(samplePrompt("girl, yue", "novelai"), "girl, yue");
  assert.equal(samplePrompt(YUE_NATURAL.prompt, "flux1d"), YUE_NATURAL.prompt);
});
