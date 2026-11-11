import assert from "node:assert/strict";
import test from "node:test";
import { SAMPLE_ARCHITECTURES, sampleCardId, samplePrompt, withSamples } from "../../src/lib/utils/sampleCharacters.ts";

const pictures = { yue: { thumbnail: "data:image/webp;base64,AA", reference: "data:image/webp;base64,BB" } };

test("a new user gets Yue once in every tag architecture, with her notes", () => {
  const out = withSamples([], [], pictures, 1);
  assert.equal(out.changed, true);
  assert.deepEqual(out.seen, ["yue"]);
  assert.equal(out.characters.length, SAMPLE_ARCHITECTURES.length);
  const nai = out.characters.find((c) => c.architecture === "novelai")!;
  assert.equal(nai.id, sampleCardId("yue", "novelai"));
  assert.ok(nai.prompt.startsWith("girl, yue,"));
  assert.equal(nai.thumbnail, pictures.yue.thumbnail);
  const sdxl = out.characters.find((c) => c.architecture === "sdxl")!;
  assert.ok(sdxl.prompt.startsWith("1girl, yue,"));
  assert.match((nai as { memory: { content: string }[] }).memory[0].content, /Usual expression: smiling/);
});

test("once seen, a deleted Yue is not added again", () => {
  const out = withSamples([], ["yue"], pictures, 1);
  assert.equal(out.changed, false);
  assert.equal(out.characters.length, 0);
});

test("an architecture that already has a Yue gets no second card", () => {
  const mine = { id: "x", name: " yue ", architecture: "novelai" };
  const synced = { id: sampleCardId("yue", "sdxl"), name: "Yue", architecture: "sdxl" };
  const out = withSamples([mine, synced], [], pictures, 1);
  assert.equal(out.characters.filter((c) => c.architecture === "novelai").length, 1);
  assert.equal(out.characters.filter((c) => c.architecture === "sdxl").length, 1);
  assert.equal(out.characters.length, SAMPLE_ARCHITECTURES.length);
});

test("only a leading girl becomes 1girl", () => {
  assert.equal(samplePrompt("girl, yue, girl", "pony"), "1girl, yue, girl");
  assert.equal(samplePrompt("girl, yue", "novelai"), "girl, yue");
});
