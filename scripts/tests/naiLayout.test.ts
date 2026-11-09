import assert from "node:assert/strict";
import test from "node:test";
import { normalizeNaiResponse, parseNaiResponse, serializeNaiFields, validateNaiResponse } from "../../src/lib/utils/naiParse.ts";
import { naiRewriteSystemPrompt } from "../../src/lib/utils/naiPrompt.ts";
import { layoutObject, layoutProseProblems, tidyNaiLayout } from "../../src/lib/utils/naiLayout.ts";

const parse = (raw: string) => normalizeNaiResponse(parseNaiResponse(raw));

test("layoutObject finds the object a relative tag points at", () => {
  assert.equal(layoutObject("behind table"), "table");
  assert.equal(layoutObject("behind wooden table"), "table");
  assert.equal(layoutObject("in front of the counter"), "counter");
  assert.equal(layoutObject("table in foreground"), "table");
  assert.equal(layoutObject("hands behind back"), null);
  assert.equal(layoutObject("behind back"), null);
  assert.equal(layoutObject("behind her"), null);
  assert.equal(layoutObject("on the left"), null);
});

test("object-relative tags leave the box for the BASE tag line, frame placement stays", () => {
  const out = tidyNaiLayout({
    base: "1girl, high complexity, cafe, table in foreground\n\nShe sits in a chair behind the table.",
    uc: "",
    boxes: ["girl, red hair,\nsmile,\nsitting, behind table, hands on table, table, in the center"],
    ucs: [null],
  });
  assert.equal(
    out.base,
    "1girl, high complexity, cafe, table in foreground, behind table, hands on table\n\nShe sits in a chair behind the table.",
  );
  assert.equal(out.boxes[0], "girl, red hair,\nsmile,\nsitting, in the center");
});

test("assertion tags leave positive fields", () => {
  const out = tidyNaiLayout({
    base: "1girl, table, opaque, not transparent, not on table",
    uc: "",
    boxes: ["girl, solid, sitting"],
    ucs: [null],
  });
  assert.equal(out.base, "1girl, table");
  assert.equal(out.boxes[0], "girl, sitting");
});

test("UC never negates an object the scene contains", () => {
  const out = tidyNaiLayout({
    base: "1girl, behind table, sitting in chair",
    uc: "table, on table, sitting on table, extra arms",
    boxes: ["girl, sitting"],
    ucs: ["on the table, school uniform, table"],
  });
  assert.equal(out.uc, "extra arms");
  assert.equal(out.ucs[0], "school uniform");
});

test("a box UC keeps poses BASE mentions, only objects go", () => {
  const out = tidyNaiLayout({
    base: "2girls, sitting, table",
    uc: "",
    boxes: ["girl, sitting, on the left", "girl, standing, on the right"],
    ucs: ["standing", "sitting"],
  });
  assert.deepEqual(out.ucs, ["standing", "sitting"]);
});

test("a prompt with no layout is left alone", () => {
  const fields = {
    base: "2girls, high complexity, bus stop, rain\n\nTwo girls wait out the rain.",
    uc: "train platform",
    boxes: ["girl, white hoodie, standing, on the left", "girl, school uniform, standing, on the right"],
    ucs: ["school uniform", "hoodie"],
  };
  assert.deepEqual(tidyNaiLayout(fields), fields);
});

test("prose negations are reported, plain layout prose is not", () => {
  assert.deepEqual(layoutProseProblems("1girl, table\n\nShe sits behind the table, not on the table."), [
    "not on the table",
  ]);
  assert.equal(
    layoutProseProblems("1girl\n\nThe table is solid and fully opaque, never transparent.").length,
    2,
  );
  assert.deepEqual(layoutProseProblems("1girl, table\n\nShe sits in a chair behind the table."), []);
  assert.deepEqual(layoutProseProblems("2girls\n\nThe girl on the left waves, not the one on the right."), []);
});

test("the parser runs the layout cleanup and the validator asks for a rewrite of negating prose", () => {
  const parsed = parse(`BASE:
1girl, high complexity, cafe, table in foreground

She sits in a chair behind the table, not on top of the table, and the table is fully opaque.

UC:
on table

CHAR 1:
girl, red hair,
sitting, behind table`);
  assert.equal(parsed.uc, "");
  assert.equal(parsed.characters[0], "girl, red hair,\nsitting");
  assert.match(parsed.base, /table in foreground, behind table\n/);
  const problems = validateNaiResponse(parsed);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /not on top of the table/);
});

test("the spec carries the layout rules", () => {
  const spec = naiRewriteSystemPrompt({
    variant: "full",
    ucPreset: 0,
    characterCount: 1,
    existing: null,
    language: "en",
    references: [],
  });
  assert.match(spec, /LAYOUT AND OCCLUSION/);
  assert.match(spec, /A CHAR block never names a scene object/);
  assert.match(spec, /Moving something is not retiring it/);
});

test("serialised fields read back as the same prompt, without the one-off lines", () => {
  const fields = {
    base: "2girls, cafe, table in foreground\n\nTwo friends share a table by the window.",
    uc: "crowd",
    characters: ["girl, red hair, blue dress", "", "girl, black hair, white shirt"],
    characterUcs: ["white shirt", null, null],
  };
  const text = serializeNaiFields(fields);
  assert.equal(
    text,
    "BASE:\n2girls, cafe, table in foreground\n\nTwo friends share a table by the window.\n\nUC:\ncrowd\n\nCHAR 1:\ngirl, red hair, blue dress\n\nCHAR 1 UC:\nwhite shirt\n\nCHAR 2:\ngirl, black hair, white shirt",
  );
  const back = parseNaiResponse(text);
  assert.equal(back.base, fields.base);
  assert.equal(back.uc, "crowd");
  assert.deepEqual(back.characters, ["girl, red hair, blue dress", "girl, black hair, white shirt"]);
  assert.deepEqual(back.characterUcs, ["white shirt", null]);
  assert.deepEqual(back.memory, []);
  assert.equal(back.note, "");
});

test("serialising needs a base and tolerates an empty UC", () => {
  assert.equal(serializeNaiFields({ base: "  ", uc: "x", characters: ["girl"] }), "");
  const text = serializeNaiFields({ base: "1girl, beach", uc: "", characters: [] });
  assert.equal(text, "BASE:\n1girl, beach\n\nUC:");
  const back = parseNaiResponse(text);
  assert.equal(back.base, "1girl, beach");
  assert.equal(back.uc, "");
});
