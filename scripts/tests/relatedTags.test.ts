import { test } from "node:test";
import assert from "node:assert/strict";
import { insertTagAfter, normalizePromptTag, promptTagSet } from "../../src/lib/utils/relatedTags.ts";

test("normalizePromptTag strips weights, emphasis and underscores", () => {
  assert.equal(normalizePromptTag(" (Red_Hair:1.2) "), "red hair");
  assert.equal(normalizePromptTag("{{smile}}"), "smile");
  assert.equal(normalizePromptTag("1.3::long hair::"), "long hair");
  assert.equal(normalizePromptTag("-1::hat::"), "hat");
  assert.equal(normalizePromptTag("[blurry]"), "blurry");
  assert.equal(normalizePromptTag("hatsune_miku_\\(append\\)"), "hatsune miku (append)");
  assert.equal(normalizePromptTag(" ( ) "), "");
});

test("promptTagSet collects every tag across lines", () => {
  const set = promptTagSet("1girl, (red hair:1.1)\n{smile}");
  assert.deepEqual([...set].sort(), ["1girl", "red hair", "smile"]);
});

test("insertTagAfter puts the tag after the selected one", () => {
  assert.deepEqual(insertTagAfter("1girl, red hair, smile", 15, "green eyes"), {
    value: "1girl, red hair, green eyes, smile",
    caret: 27,
  });
  // Selection inside a weighted tag: the new tag goes after the whole weight.
  assert.equal(insertTagAfter("(red hair:1.2), smile", 4, "ahoge").value, "(red hair:1.2), ahoge, smile");
  // Last tag on a line, then an empty prompt.
  assert.equal(insertTagAfter("1girl\nsmile", 3, "solo").value, "1girl, solo\nsmile");
  assert.equal(insertTagAfter("", 0, "solo").value, "solo");
});
