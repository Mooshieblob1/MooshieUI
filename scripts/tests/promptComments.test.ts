import assert from "node:assert/strict";
import test from "node:test";
import {
  findCommentRanges,
  stripPromptComments,
  toggleLineComments,
} from "../../src/lib/utils/promptComments.ts";
import { findMatches, replaceAllMatches } from "../../src/lib/utils/promptFind.ts";

test("a whole commented line disappears with its line break", () => {
  assert.equal(stripPromptComments("1girl, solo\n// red hair\nsmile"), "1girl, solo\nsmile");
});

test("a comment after tags cuts the rest of that line only", () => {
  assert.equal(stripPromptComments("1girl, // solo, hat\nsmile"), "1girl,\nsmile");
});

test("a URL or a//b inside a tag is not a comment", () => {
  const prompt = "Text: http://example.com, a//b";
  assert.deepEqual(findCommentRanges(prompt), []);
  assert.equal(stripPromptComments(prompt), prompt);
});

test("a comment right after a comma counts", () => {
  assert.equal(stripPromptComments("1girl,//solo"), "1girl,");
});

test("prompts without comments come back unchanged", () => {
  const prompt = "1girl, {{smile}}, 1.2::rain::\n\nText:\nHello";
  assert.equal(stripPromptComments(prompt), prompt);
});

test("Ctrl+/ comments the caret's line and keeps the caret on its text", () => {
  const r = toggleLineComments("a, b\nc, d", 7, 7);
  assert.equal(r.value, "a, b\n// c, d");
  assert.equal(r.selectionStart, 10);
});

test("Ctrl+/ on a selection toggles every touched line, then back", () => {
  const on = toggleLineComments("a\nb\n\nc", 0, 6);
  assert.equal(on.value, "// a\n// b\n\n// c");
  const off = toggleLineComments(on.value, on.selectionStart, on.selectionEnd);
  assert.equal(off.value, "a\nb\n\nc");
});

test("Ctrl+/ with mixed lines comments all of them", () => {
  assert.equal(toggleLineComments("// a\nb", 0, 6).value, "// // a\n// b");
});

test("find is plain text and case-insensitive by default", () => {
  assert.deepEqual(findMatches("Hat, hat (red)", "hat"), [[0, 3], [5, 8]]);
  assert.deepEqual(findMatches("Hat, hat", "hat", true), [[5, 8]]);
  assert.deepEqual(findMatches("a (b) c", "(b)"), [[2, 5]]);
});

test("replace all keeps $ in the replacement literal", () => {
  assert.equal(replaceAllMatches("x, x", "x", "$&y"), "$&y, $&y");
});
