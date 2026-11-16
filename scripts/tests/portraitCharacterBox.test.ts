import assert from "node:assert/strict";
import test from "node:test";
import { splitCountTags } from "../../src/lib/utils/portraitCharacterBox.ts";

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
