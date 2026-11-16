import assert from "node:assert/strict";
import test from "node:test";
import { findTagCore } from "../../src/lib/utils/promptTagAffixes.ts";

/** The tag name findTagCore picks out of the comma-bounded piece `piece`. */
function core(prompt: string, piece: string): string {
  const start = prompt.indexOf(piece);
  assert.notEqual(start, -1, `${piece} is not in ${prompt}`);
  const found = findTagCore(prompt, start, start + piece.length);
  return prompt.slice(found.start, found.end);
}

const UC = "-2::lowres, sketch::, english text";

test("the closing :: of a negative NovelAI span is not part of the last tag", () => {
  assert.equal(core(UC, "sketch::"), "sketch");
});

test("the opener of a negative NovelAI span is not part of the first tag", () => {
  assert.equal(core(UC, "-2::lowres"), "lowres");
});

test("a tag after a closed span is the whole piece", () => {
  assert.equal(core(UC, "english text"), "english text");
});

test("a single-tag span keeps both ends", () => {
  assert.equal(core("1.2::sketch::, smile", "1.2::sketch::"), "sketch");
  assert.equal(core(".5::sketch::", ".5::sketch::"), "sketch");
});

test("an open span with no closer yet leaves the tag alone", () => {
  assert.equal(core("1.2::rain, nig", "nig"), "nig");
});

test("a tag ending in :: outside any span keeps its colons", () => {
  assert.equal(core("1girl, odd::", "odd::"), "odd::");
});

test("NovelAI braces and brackets stay out of the tag", () => {
  assert.equal(core("{{sketch}}, smile", "{{sketch}}"), "sketch");
  assert.equal(core("[[lowres, sketch]]", "sketch]]"), "sketch");
  assert.equal(core("{1.2::sketch::}", "{1.2::sketch::}"), "sketch");
});

test("an A1111 weight and its parenthesis stay out of the tag", () => {
  assert.equal(core("(sketch:1.2), smile", "(sketch:1.2)"), "sketch");
  assert.equal(core("(lowres, sketch:0.8), smile", "sketch:0.8)"), "sketch");
});

test("parentheses that belong to the tag stay with it", () => {
  assert.equal(core("1girl, hatsune miku (vocaloid), smile", "hatsune miku (vocaloid)"), "hatsune miku (vocaloid)");
  assert.equal(core("hatsune miku \\(vocaloid\\)", "hatsune miku \\(vocaloid\\)"), "hatsune miku \\(vocaloid\\)");
  assert.equal(core("1girl, :)", ":)"), ":)");
});

test("a fragment that is all syntax falls back to the whole piece", () => {
  assert.equal(core("1.2::rain, ::", "::"), "::");
});

test("replacing only the core keeps the user's NovelAI span intact", () => {
  // What accepting the autocomplete suggestion `sketch` with the caret in `sketch::` does.
  const start = UC.indexOf("sketch::");
  const found = findTagCore(UC, start, start + "sketch::".length);
  const next = UC.slice(0, found.start) + "sketch" + UC.slice(found.end);
  assert.equal(next, UC);
});
