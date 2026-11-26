import assert from "node:assert/strict";
import test from "node:test";
import {
  buildTurnaroundCharacterSlots,
  splitCountTags,
  stripBackViewFacialTags,
  stripConflictingAngleTags,
} from "../../src/lib/utils/portraitCharacterBox.ts";
import { portraitPrProse, portraitPrTags } from "../../src/lib/utils/portraitFilter.ts";

test("count tags leave the look for the main prompt, the box starts with the noun", () => {
  assert.deepEqual(splitCountTags("1girl, solo, yue (arifureta), blonde hair, red eyes"), {
    counts: "1girl",
    look: "girl, yue (arifureta), blonde hair, red eyes",
  });
  assert.deepEqual(splitCountTags("1boy, black hair"), { counts: "1boy", look: "boy, black hair" });
  assert.deepEqual(splitCountTags("1other, robot"), { counts: "1other", look: "other, robot" });
});

test("a box that already says girl does not get it twice", () => {
  assert.deepEqual(splitCountTags("1girl, girl, pink hair"), { counts: "1girl", look: "girl, pink hair" });
});

test("no count tag leaves the look as it is", () => {
  assert.deepEqual(splitCountTags("silver hair, (blue eyes:1.2), hair ribbon"), {
    counts: "",
    look: "silver hair, (blue eyes:1.2), hair ribbon",
  });
});

test("weighted groups and lookalike tags are not counts", () => {
  assert.deepEqual(splitCountTags("1girl, 1.2::girl with a hat::, 10girls_frontline"), {
    counts: "1girl",
    look: "girl, 1.2::girl with a hat::, 10girls_frontline",
  });
});

test("stripBackViewFacialTags removes eyes, mouth, eyewear, expression, and front angles while keeping hair and outfit", () => {
  const input =
    "girl, artist:foo, long silver hair, ponytail, hair ribbon, blue eyes, thick eyebrows, red lips, glasses, smile, looking at viewer, front view, blue dress, high heels";
  assert.equal(
    stripBackViewFacialTags(input, "tags"),
    "girl, artist:foo, long silver hair, ponytail, hair ribbon, blue dress, high heels",
  );
  assert.equal(
    stripConflictingAngleTags("girl, red hair, profile, looking at viewer, back view, coat", "front", "tags"),
    "girl, red hair, looking at viewer, coat",
  );
  assert.equal(
    stripConflictingAngleTags("girl, red hair, front view, back view, standing, coat", "side", "tags"),
    "girl, red hair, standing, coat",
  );
});

test("buildTurnaroundCharacterSlots lays out 3-view and 4-view positioned slots and drops back expression", () => {
  const slots3 = buildTurnaroundCharacterSlots({
    characterPrompt: "girl, silver hair, blue eyes, red coat",
    characterNegative: "hat",
    expression: "gentle smile",
    style: "tags",
    views: 3,
    includeNegative: true,
  });
  assert.equal(slots3.length, 3);
  assert.deepEqual(
    slots3.map((s) => s.center.x),
    [0.18, 0.5, 0.82],
  );
  assert.ok(slots3[0].prompt.includes("gentle smile"));
  assert.ok(slots3[0].prompt.includes("front view"));
  assert.ok(slots3[1].prompt.includes("side view"));
  assert.ok(!slots3[2].prompt.includes("blue eyes"));
  assert.ok(!slots3[2].prompt.includes("gentle smile"));
  assert.ok(slots3[2].prompt.includes("back view, from behind"));
  assert.ok(slots3[2].negative_prompt.includes("face, eyes, mouth, smile"));
  assert.ok(slots3[2].negative_prompt.endsWith(", hat"));

  const slots4 = buildTurnaroundCharacterSlots({
    characterPrompt: "A girl with silver hair and blue eyes looking at the viewer",
    characterNegative: "hat",
    expression: "warm smile",
    style: "natural",
    views: 4,
    includeNegative: false,
  });
  assert.equal(slots4.length, 4);
  assert.deepEqual(
    slots4.map((s) => s.center.x),
    [0.14, 0.38, 0.62, 0.86],
  );
  assert.ok(slots4[1].prompt.includes("facing left"));
  assert.ok(slots4[3].prompt.includes("facing right"));
  assert.ok(!slots4[2].prompt.includes("warm smile"));
  assert.ok(!slots4[2].prompt.includes("looking at the viewer"));
  assert.ok(slots4.every((s) => s.negative_prompt === ""));
});

test("portraitPrTags keeps accessories, heterochromia, and facial marks while stripping clothes, hair, and single eye color", () => {
  const result = portraitPrTags(
    "1girl, solo, artist:foo, yue (arifureta), long blonde hair, blue eyes, white sailor uniform, hair ribbon, round glasses, gold earrings, ribbon choker, mole under eye, ahoge, red dress, smiling",
  );
  assert.equal(
    result.kept,
    "1girl, solo, artist:foo, hair ribbon, round glasses, gold earrings, ribbon choker, mole under eye, ahoge",
  );
  assert.deepEqual(result.dropped, [
    "yue (arifureta)",
    "long blonde hair",
    "blue eyes",
    "white sailor uniform",
    "red dress",
    "smiling",
  ]);
});

test("portraitPrTags preserves eye color tags when heterochromia or mismatched eye colors are present", () => {
  const withCue = portraitPrTags("1girl, heterochromia, red eyes, blue eyes, long black hair, school uniform, glasses");
  assert.equal(withCue.kept, "1girl, heterochromia, red eyes, blue eyes, glasses");
  assert.deepEqual(withCue.dropped, ["long black hair", "school uniform"]);

  const pluralColors = portraitPrTags("1girl, green eyes, purple eyes, black jacket, cat ears");
  assert.equal(pluralColors.kept, "1girl, green eyes, purple eyes, cat ears");
  assert.deepEqual(pluralColors.dropped, ["black jacket"]);

  const singleEyeModifier = portraitPrTags("1girl, dark blue eyes, silver hair, white shirt");
  assert.equal(singleEyeModifier.kept, "1girl");
  assert.deepEqual(singleEyeModifier.dropped, ["dark blue eyes", "silver hair", "white shirt"]);
});

test("portraitPrTags rejects below-neck or shirt-collar phrases even when they contain accessory words", () => {
  const result = portraitPrTags(
    "1girl, arm band, sailor collar, thigh ribbon, chest scar, glasses, sailor cap, (long hair, choker:1.2)",
  );
  assert.equal(result.kept, "1girl, glasses, sailor cap, (choker:1.2)");
  assert.deepEqual(result.dropped, [
    "arm band",
    "sailor collar",
    "thigh ribbon",
    "chest scar",
    "long hair",
  ]);
});

test("portraitPrProse keeps PR-relevant clauses and strips body, clothing, and plain appearance", () => {
  const result = portraitPrProse(
    "A tall girl with long silver hair, heterochromia, one red eye and one blue eye, thin gold glasses, and a black military coat with brass buttons.",
  );
  assert.equal(result.kept, "heterochromia, one red eye and one blue eye, thin gold glasses.");
  assert.deepEqual(result.dropped, [
    "A tall girl with long silver hair",
    "and a black military coat with brass buttons",
  ]);

  const noHeterochromia = portraitPrProse(
    "A young woman with short brown hair, dark green eyes, silver earrings, and a white sailor uniform.",
  );
  assert.equal(noHeterochromia.kept, "silver earrings.");
  assert.deepEqual(noHeterochromia.dropped, [
    "A young woman with short brown hair",
    "dark green eyes",
    "and a white sailor uniform",
  ]);
});
