import assert from "node:assert/strict";
import test from "node:test";
import {
  clampCharacterReferenceFidelity,
  clampCharacterReferenceStrength,
  effectiveCharacterPreciseReferences,
  matchCharacterCardsInText,
  normalizeCharacterReferenceDescription,
  patchCharacterDirectorReference,
  pruneAutoAttachedCharacterReferences,
  reconcileCharacterDirectorReferences,
  removeCharacterDirectorReferences,
  type DirectorRefEntry,
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

test("reconcileCharacterDirectorReferences replaces prior owned and legacy entries in place without doubling", () => {
  const manual: DirectorRefEntry = {
    image: "MANUAL_IMG",
    description: NAI_REF_STYLE,
    strength: 1.0,
    fidelity: 1.0,
    enabled: true,
  };
  const oldLegacyCardPic: DirectorRefEntry = {
    image: "CARD_FALLBACK_IMG",
    description: NAI_REF_CHARACTER,
    strength: 1.0,
    fidelity: 1.0,
    enabled: true,
  };
  const bobRef: DirectorRefEntry = {
    image: "BOB_IMG",
    description: NAI_REF_CHARACTER_MAIN_OUTFIT,
    strength: 1.0,
    fidelity: 1.0,
    enabled: true,
    source_character_id: "bob",
    source_ref_id: "r_bob",
    auto_attached: true,
  };
  const replacementAlice: DirectorRefEntry = {
    image: "NEW_ALICE_PR_IMG",
    description: NAI_REF_CHARACTER_MAIN_OUTFIT,
    strength: 1.25,
    fidelity: 1.0,
    enabled: true,
    source_character_id: "alice",
    source_ref_id: "r_alice_1",
    auto_attached: true,
  };

  // Alice's legacy untagged card picture sits at index 0 ahead of manual & bob.
  const initial = [oldLegacyCardPic, manual, bobRef];
  const res = reconcileCharacterDirectorReferences(initial, "alice", [replacementAlice], {
    legacyImages: new Set(["CARD_FALLBACK_IMG"]),
  });
  assert.equal(res.added.length, 1);
  assert.deepEqual(
    res.next.map((r) => ({ image: r.image, desc: r.description })),
    [
      { image: "NEW_ALICE_PR_IMG", desc: NAI_REF_CHARACTER_MAIN_OUTFIT },
      { image: "MANUAL_IMG", desc: NAI_REF_STYLE },
      { image: "BOB_IMG", desc: NAI_REF_CHARACTER_MAIN_OUTFIT },
    ],
  );

  // Re-attaching Alice's updated ref replaces NEW_ALICE_PR_IMG cleanly, never creating count=2.
  const replacementAlice2: DirectorRefEntry = {
    ...replacementAlice,
    image: "ALICE_V2_IMG",
    description: NAI_REF_CHARACTER,
  };
  const res2 = reconcileCharacterDirectorReferences(res.next, "alice", [replacementAlice2]);
  assert.equal(res2.next.length, 3);
  assert.equal(res2.next[0].image, "ALICE_V2_IMG");
  assert.equal(res2.next[0].description, NAI_REF_CHARACTER);

  // Duplicate against manual ref is skipped, and cap=2 skips overflow.
  const capped = reconcileCharacterDirectorReferences(
    [manual, bobRef],
    "alice",
    [
      { ...replacementAlice, image: "MANUAL_IMG" },
      { ...replacementAlice, image: "ALICE_3" },
    ],
    { max: 2 },
  );
  assert.equal(capped.duplicateCount, 1);
  assert.equal(capped.skippedFull, true);
  assert.equal(capped.next.length, 2);
});

test("removeCharacterDirectorReferences removes by character id, refId, and legacy image while sparing manual refs", () => {
  const manual: DirectorRefEntry = {
    image: "MANUAL_IMG",
    description: NAI_REF_STYLE,
    strength: 1,
    fidelity: 1,
  };
  const legacyUntagged: DirectorRefEntry = {
    image: "LEGACY_CARD_IMG",
    description: NAI_REF_CHARACTER,
    strength: 1,
    fidelity: 1,
  };
  const alice1: DirectorRefEntry = {
    image: "A1",
    description: NAI_REF_CHARACTER,
    strength: 1,
    fidelity: 1,
    source_character_id: "alice",
    source_ref_id: "ref1",
    auto_attached: true,
  };
  const alice2: DirectorRefEntry = {
    image: "A2",
    description: NAI_REF_OUTFIT,
    strength: 2,
    fidelity: 1,
    source_character_id: "alice",
    source_ref_id: "ref2",
    auto_attached: true,
  };

  const singleRemoved = removeCharacterDirectorReferences([manual, alice1, alice2], "alice", {
    refId: "ref1",
  });
  assert.equal(singleRemoved.removed.length, 1);
  assert.deepEqual(
    singleRemoved.next.map((r) => r.image),
    ["MANUAL_IMG", "A2"],
  );

  const allRemoved = removeCharacterDirectorReferences(
    [manual, legacyUntagged, alice1, alice2],
    "alice",
    { legacyImages: new Set(["LEGACY_CARD_IMG"]) },
  );
  assert.equal(allRemoved.removed.length, 3);
  assert.deepEqual(
    allRemoved.next.map((r) => r.image),
    ["MANUAL_IMG"],
  );
});

test("patchCharacterDirectorReference and pruneAutoAttachedCharacterReferences keep live state aligned", () => {
  const manual: DirectorRefEntry = {
    image: "MANUAL",
    description: NAI_REF_STYLE,
    strength: 1,
    fidelity: 1,
  };
  const manualCharRef: DirectorRefEntry = {
    image: "BOB_MANUAL",
    description: NAI_REF_CHARACTER,
    strength: 1,
    fidelity: 1,
    source_character_id: "bob",
    source_ref_id: "__card__",
    auto_attached: false,
  };
  const autoAlice: DirectorRefEntry = {
    image: "ALICE_AUTO",
    description: NAI_REF_CHARACTER,
    strength: 1,
    fidelity: 1,
    source_character_id: "alice",
    source_ref_id: "r1",
    auto_attached: true,
  };

  const patched = patchCharacterDirectorReference([manual, autoAlice], "alice", "r1", {
    description: NAI_REF_CHARACTER_MAIN_OUTFIT,
    strength: 2.5,
  });
  assert.equal(patched.changed, true);
  assert.equal(patched.next[1].description, NAI_REF_CHARACTER_MAIN_OUTFIT);
  assert.equal(patched.next[1].strength, 2.5);

  const pruned = pruneAutoAttachedCharacterReferences(
    [manual, manualCharRef, patched.next[1]],
    new Set(),
  );
  assert.equal(pruned.removed.length, 1);
  assert.equal(pruned.removed[0].image, "ALICE_AUTO");
  assert.deepEqual(
    pruned.next.map((r) => r.image),
    ["MANUAL", "BOB_MANUAL"],
  );
});
