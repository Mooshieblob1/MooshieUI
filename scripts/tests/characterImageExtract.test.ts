import assert from "node:assert/strict";
import test from "node:test";
import type { InterrogationResult } from "../../src/lib/types/index.js";
import {
  characterExtractRequest,
  groundExtracted,
  type ExtractedCharacter,
} from "../../src/lib/utils/characterExtract.js";
import {
  metadataToExtractSource,
  partitionExtracted,
  tagsToExtractSource,
} from "../../src/lib/utils/characterImageExtract.js";

const found = (
  name: string,
  prompt: string,
  negative = "",
  sameAs = "",
): ExtractedCharacter => ({
  name,
  prompt,
  negative,
  sameAs,
  variant: "",
});

test("metadataToExtractSource reads standard and NovelAI character metadata", () => {
  assert.equal(metadataToExtractSource(null), null);
  assert.equal(metadataToExtractSource({ negative_prompt: "lowres" }), null);

  const plain = metadataToExtractSource({
    positive_prompt: "  1girl, silver hair, red eyes ",
    negative_prompt: "  blue eyes, lowres  ",
  });
  assert.deepEqual(plain, {
    prompt: "1girl, silver hair, red eyes",
    negative: "blue eyes, lowres",
    kind: "metadata",
  });

  const nai = metadataToExtractSource({
    positive_prompt: "2girls, street",
    negative_prompt: "lowres",
    mooshie_novelai_characters: JSON.stringify([
      { prompt: "girl, green hair, beret", negative_prompt: "red hair", enabled: true },
      { prompt: "skipped girl", negative_prompt: "foo", enabled: false },
      { prompt: "girl, pink twintails", negative_prompt: "", enabled: true },
    ]),
  });
  assert.equal(nai?.kind, "metadata");
  assert.equal(
    nai?.prompt,
    "2girls, street\nCharacter 1: girl, green hair, beret\nCharacter 2: girl, pink twintails",
  );
  assert.equal(nai?.negative, "lowres\nCharacter 1: red hair");
  assert.deepEqual(nai?.boxes, [
    { prompt: "girl, green hair, beret", negative: "red hair" },
    { prompt: "girl, pink twintails", negative: "" },
  ]);

  const badJson = metadataToExtractSource({
    positive_prompt: "1boy, jacket",
    mooshie_novelai_characters: "{broken",
  });
  assert.deepEqual(badJson, {
    prompt: "1boy, jacket",
    negative: "",
    kind: "metadata",
  });
});

test("tagsToExtractSource formats checked tags and always omits artist tags", () => {
  const result: InterrogationResult = {
    character_tags: [
      { name: "hatsune_miku", confidence: 0.95 },
      { name: "kagamine_rin", confidence: 0.4 },
    ],
    artist_tags: [{ name: "artist:foo", confidence: 0.99 }],
    copyright_tags: [{ name: "vocaloid", confidence: 0.92 }],
    general_tags: [
      { name: "aqua_hair", confidence: 0.9 },
      { name: "twintails", confidence: 0.85 },
      { name: "hatsune_miku", confidence: 0.8 },
    ],
    rating_tags: [{ name: "general", confidence: 0.99 }],
  };

  const defaultSource = tagsToExtractSource(result);
  assert.deepEqual(defaultSource, {
    prompt: "hatsune miku, kagamine rin, aqua hair, twintails",
    negative: "",
    kind: "interrogator",
  });

  const checkedSource = tagsToExtractSource(result, {
    character: { hatsune_miku: true, kagamine_rin: false },
    copyright: { vocaloid: true },
    general: { aqua_hair: true, twintails: false, hatsune_miku: true },
  });
  assert.deepEqual(checkedSource, {
    prompt: "hatsune miku, vocaloid, aqua hair",
    negative: "",
    kind: "interrogator",
  });

  assert.equal(
    tagsToExtractSource(result, {
      character: { hatsune_miku: false, kagamine_rin: false },
      copyright: { vocaloid: false },
      general: { aqua_hair: false, twintails: false },
    }),
    null,
  );
});

test("targetStyle converts tag input to natural prose without grounding stripping it", () => {
  const tagPrompt = "1girl, hatsune miku, aqua hair, long twintails, necktie";
  const reqNatural = characterExtractRequest(tagPrompt, "", [], "natural");
  assert.match(reqNatural.system, /natural-language/i);

  const proseItem = [
    found(
      "Hatsune Miku",
      "Hatsune Miku with long aqua hair tied in twintails and a teal necktie",
    ),
  ];
  assert.deepEqual(groundExtracted(proseItem, tagPrompt, "", "natural"), proseItem);
  // Without targetStyle="natural", tag-level grounding rejects free-form prose phrases.
  assert.deepEqual(groundExtracted(proseItem, tagPrompt, ""), []);

  const prosePrompt = "A silver-haired knight wearing battered plate armor stands in the rain.";
  const reqTags = characterExtractRequest(prosePrompt, "", [], "tags");
  assert.match(reqTags.system, /comma-separated appearance tags/i);
  const tagItem = [found("Silver Knight", "silver hair, plate armor")];
  assert.deepEqual(groundExtracted(tagItem, prosePrompt, "", "tags"), tagItem);
});

test("partitionExtracted separates fresh, differing matches, and exact duplicates", () => {
  const saved = [
    { id: "1", name: "Hatsune Miku", prompt: "hatsune miku, aqua hair", negative: "" },
    { id: "2", name: "Reimu", prompt: "hakurei reimu, brown hair", negative: "blue eyes" },
  ];
  const extracted = [
    found("Hatsune Miku", "hatsune miku, aqua hair", ""),
    found("Reimu", "hakurei reimu, brown hair, red bow", "blue eyes"),
    found("Reimu (alt)", "hakurei reimu, brown hair, shrine maiden", "", "Reimu"),
    found("Marisa", "kirisame marisa, blonde hair, witch hat"),
  ];

  const { matches, fresh } = partitionExtracted(extracted, saved);
  assert.equal(fresh.length, 1);
  assert.equal(fresh[0].name, "Marisa");
  assert.equal(matches.length, 1);
  assert.equal(matches[0].saved.id, "2");
  assert.equal(matches[0].item.prompt, "hakurei reimu, brown hair, red bow");
});
