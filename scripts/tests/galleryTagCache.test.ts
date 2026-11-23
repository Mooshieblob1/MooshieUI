import { test } from "node:test";
import assert from "node:assert/strict";
import { tagsFromInterrogation } from "../../src/lib/utils/galleryTagCache.js";
import { detectCharacters, prepareCards } from "../../src/lib/utils/galleryCharacters.js";

const miku = { id: "a", name: "Hatsune Miku", prompt: "1girl, hatsune miku, aqua hair, twintails, aqua eyes", parentId: null };

test("tagger output reads as a prompt: character tags first, no repeats", () => {
  const tags = tagsFromInterrogation({
    character_tags: [{ name: "hatsune_miku", confidence: 0.9 }],
    general_tags: [
      { name: "1girl", confidence: 0.99 },
      { name: "twintails", confidence: 0.8 },
      { name: "1girl", confidence: 0.5 },
      { name: " ", confidence: 0.5 },
    ],
  });
  assert.equal(tags, "hatsune_miku, 1girl, twintails");
});

test("a saved character is found from the tagger's character tag", () => {
  const tags = tagsFromInterrogation({
    character_tags: [{ name: "hatsune_miku", confidence: 0.9 }],
    general_tags: [{ name: "1girl", confidence: 0.99 }],
  });
  const hits = detectCharacters({ positive_prompt: tags }, prepareCards([miku]));
  assert.equal(hits[0]?.characterId, "a");
  assert.equal(hits[0]?.score, 1);
});

test("a saved character is found from looks alone, and the subject gate still applies", () => {
  const cards = prepareCards([miku]);
  const looks = tagsFromInterrogation({
    character_tags: [],
    general_tags: ["1girl", "aqua_hair", "twintails", "aqua_eyes", "skirt"].map((name) => ({ name, confidence: 0.7 })),
  });
  assert.equal(detectCharacters({ positive_prompt: looks }, cards)[0]?.characterId, "a");
  const boy = tagsFromInterrogation({
    character_tags: [],
    general_tags: ["1boy", "aqua_hair", "twintails", "aqua_eyes"].map((name) => ({ name, confidence: 0.7 })),
  });
  assert.deepEqual(detectCharacters({ positive_prompt: boy }, cards), []);
});

test("an empty result gives an empty string, which still counts as scanned", () => {
  assert.equal(tagsFromInterrogation({ character_tags: [], general_tags: [] }), "");
});
