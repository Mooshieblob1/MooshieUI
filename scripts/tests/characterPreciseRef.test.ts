import assert from "node:assert/strict";
import test from "node:test";
import {
  clampCharacterReferenceFidelity,
  clampCharacterReferenceStrength,
  effectiveCharacterPreciseReferences,
  matchCharacterCardsInText,
  normalizeCharacterReferenceDescription,
} from "../../src/lib/utils/characterPreciseRefLogic.ts";
import {
  NAI_REF_CHARACTER,
  NAI_REF_CHARACTER_MAIN_OUTFIT,
  NAI_REF_CHARACTER_STYLE,
  NAI_REF_CHARACTER_STYLE_ANY_OUTFIT,
  NAI_REF_OUTFIT,
  NAI_REF_STYLE,
} from "../../src/lib/utils/novelaiModels.ts";

test("effectiveCharacterPreciseReferences prefers dedicated entries and falls back to card picture", () => {
  const empty = effectiveCharacterPreciseReferences({ thumbnail: null, reference: null });
  assert.deepEqual(empty, []);

  const thumbOnly = effectiveCharacterPreciseReferences({
    thumbnail: "data:image/webp;base64,thumb",
    reference: null,
  });
  assert.equal(thumbOnly.length, 1);
  assert.equal(thumbOnly[0].id, "__card__");
  assert.equal(thumbOnly[0].reference, "data:image/webp;base64,thumb");
  assert.equal(thumbOnly[0].description, NAI_REF_CHARACTER);
  assert.equal(thumbOnly[0].strength, 1.0);

  const cardWithBoth = effectiveCharacterPreciseReferences({
    thumbnail: "data:image/webp;base64,thumb",
    reference: "data:image/webp;base64,ref",
  });
  assert.equal(cardWithBoth.length, 1);
  assert.equal(cardWithBoth[0].reference, "data:image/webp;base64,ref");
  assert.equal(cardWithBoth[0].thumbnail, "data:image/webp;base64,thumb");

  const dedicated = effectiveCharacterPreciseReferences({
    thumbnail: "data:image/webp;base64,thumb",
    reference: "data:image/webp;base64,ref",
    preciseReferences: [
      {
        id: "r1",
        thumbnail: "data:image/webp;base64,t1",
        reference: "data:image/webp;base64,f1",
        description: NAI_REF_OUTFIT,
        strength: 2.5,
      },
      {
        id: "r2",
        thumbnail: "data:image/webp;base64,t2",
        reference: "data:image/webp;base64,f2",
        description: NAI_REF_STYLE,
        strength: 0.8,
      },
    ],
  });
  assert.equal(dedicated.length, 2);
  assert.equal(dedicated[0].id, "r1");
  assert.equal(dedicated[1].id, "r2");
});

test("normalizeCharacterReferenceDescription handles V5 vs V4.5 capabilities and invalid inputs", () => {
  assert.equal(
    normalizeCharacterReferenceDescription(NAI_REF_OUTFIT, {
      supportsOutfits: true,
      supportsSameOutfit: true,
    }),
    NAI_REF_OUTFIT,
  );
  assert.equal(
    normalizeCharacterReferenceDescription(NAI_REF_OUTFIT, {
      supportsOutfits: false,
      supportsSameOutfit: false,
    }),
    NAI_REF_CHARACTER,
  );
  assert.equal(
    normalizeCharacterReferenceDescription(NAI_REF_CHARACTER_MAIN_OUTFIT, {
      supportsOutfits: false,
      supportsSameOutfit: false,
    }),
    NAI_REF_CHARACTER,
  );
  assert.equal(
    normalizeCharacterReferenceDescription(NAI_REF_CHARACTER_STYLE_ANY_OUTFIT, {
      supportsOutfits: false,
      supportsSameOutfit: false,
    }),
    NAI_REF_CHARACTER_STYLE,
  );
  assert.equal(
    normalizeCharacterReferenceDescription("bogus", {
      supportsOutfits: true,
      supportsSameOutfit: true,
    }),
    NAI_REF_CHARACTER,
  );
});

test("clampCharacterReferenceStrength and clampCharacterReferenceFidelity clamp bounds and fallbacks", () => {
  assert.equal(clampCharacterReferenceStrength(undefined, 5), 1.0);
  assert.equal(clampCharacterReferenceStrength(Number.NaN, 5), 1.0);
  assert.equal(clampCharacterReferenceStrength(-2, 5), 0);
  assert.equal(clampCharacterReferenceStrength(3.5, 5), 3.5);
  assert.equal(clampCharacterReferenceStrength(3.5, 1), 1);

  assert.equal(clampCharacterReferenceFidelity(undefined), 1.0);
  assert.equal(clampCharacterReferenceFidelity(-0.4), 0);
  assert.equal(clampCharacterReferenceFidelity(1.6), 1);
  assert.equal(clampCharacterReferenceFidelity(0.75), 0.75);
});

test("matchCharacterCardsInText matches names, short/CJK names, variant qualifiers, and tag follow-ups without duplicates", () => {
  const cards = [
    {
      id: "julie",
      name: "Julie",
      prompt: "1girl, blonde hair, green eyes, ribbon",
      parentId: null,
    },
    {
      id: "julie-swim",
      name: "Julie (swimsuit)",
      prompt: "1girl, blonde hair, green eyes, striped bikini",
      parentId: "julie",
    },
    {
      id: "rin",
      name: "Rin",
      prompt: "1girl, short black hair, red scarf, yellow eyes",
      parentId: null,
    },
    {
      id: "sakura",
      name: "桜",
      prompt: "1girl, pink kimono, fox ears",
      parentId: null,
    },
    {
      id: "other",
      name: "Alice Evergreen",
      prompt: "1girl, silver hair, blue eyes, mage hat, wand",
      parentId: null,
    },
  ];

  // Explicit name chooses base card when variant qualifier is absent.
  const baseHit = matchCharacterCardsInText(cards, ["Draw Julie in a cafe"], {
    base: "1girl, cafe",
    characters: ["1girl, blonde hair, green eyes"],
  });
  assert.deepEqual(baseHit.map((c) => c.id), ["julie"]);

  // Explicit variant qualifier prefers the variant card over its parent.
  const variantHit = matchCharacterCardsInText(cards, ["Julie in her swimsuit at the beach"], {
    base: "1girl, beach",
    characters: ["1girl, bikini, blonde hair"],
  });
  assert.deepEqual(variantHit.map((c) => c.id), ["julie-swim"]);

  // Short Latin name and CJK name match cleanly without matching unrelated substrings ("bring").
  const shortAndCjk = matchCharacterCardsInText(cards, ["Draw Rin and 桜 together"], {
    base: "2girls",
    characters: ["1girl, short black hair", "1girl, pink kimono"],
  });
  assert.deepEqual(shortAndCjk.map((c) => c.id), ["rin", "sakura"]);

  const noSubstringFalsePositive = matchCharacterCardsInText(cards, ["bring Alice Evergreen"], {
    base: "1girl, silver hair",
  });
  assert.deepEqual(noSubstringFalsePositive.map((c) => c.id), ["other"]);

  // Unnamed follow-up edit falls back to identity/tag detection in the generated prompt.
  const followupHit = matchCharacterCardsInText(cards, ["make her smile warmer"], {
    base: "1girl, warm lighting",
    characters: ["1girl, silver hair, blue eyes, mage hat, warm smile"],
  });
  assert.deepEqual(followupHit.map((c) => c.id), ["other"]);
});
