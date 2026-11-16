import assert from "node:assert/strict";
import test from "node:test";
import {
  findTrigger,
  insertTrigger,
  normalizeTrigger,
  removeTrigger,
} from "../../src/lib/utils/loraTriggerText.ts";

test("insert appends once, as its own piece", () => {
  assert.equal(insertTrigger("", "@yue"), "@yue");
  assert.equal(insertTrigger("1girl, smile", "@yue"), "1girl, smile, @yue");
  assert.equal(insertTrigger("1girl, smile, ", "@yue"), "1girl, smile, @yue");
  assert.equal(insertTrigger("1girl, @yue, smile", "@yue"), "1girl, @yue, smile");
  assert.equal(insertTrigger("1girl\n", "glowing eyes"), "1girl, glowing eyes");
});

test("a trigger with commas is one unit", () => {
  const trigger = "yae miko,  pink hair";
  assert.equal(normalizeTrigger(trigger), "yae miko, pink hair");
  const added = insertTrigger("1girl", trigger);
  assert.equal(added, "1girl, yae miko, pink hair");
  assert.equal(insertTrigger(added, trigger), added);
  assert.equal(removeTrigger(added, trigger), "1girl");
  assert.equal(removeTrigger("yae miko, pink hair, 1girl", trigger), "1girl");
});

test("matching ignores case and spacing but not partial tags", () => {
  assert.ok(findTrigger("1girl,Yae Miko ,pink hair", "yae miko, pink hair"));
  assert.equal(findTrigger("1girl, yae miko style", "yae miko"), null);
  assert.equal(findTrigger("1girl, (yae miko:1.2)", "yae miko"), null);
  assert.equal(removeTrigger("1girl, (yae miko:1.2)", "yae miko"), "1girl, (yae miko:1.2)");
});

test("remove keeps the rest of the prompt as written", () => {
  assert.equal(removeTrigger("1girl, @yue, smile", "@yue"), "1girl, smile");
  assert.equal(removeTrigger("@yue, 1girl", "@yue"), "1girl");
  assert.equal(removeTrigger("1girl, smile, @yue", "@yue"), "1girl, smile");
  assert.equal(removeTrigger("@yue", "@yue"), "");
  assert.equal(removeTrigger("1girl,\n@yue\nsmile", "@yue"), "1girl,\nsmile");
  assert.equal(removeTrigger("1girl\n@yue", "@yue"), "1girl");
  assert.equal(removeTrigger("1girl, smile", "@yue"), "1girl, smile");
});

test("insert then remove round-trips", () => {
  for (const prompt of ["", "1girl", "1girl, smile", "masterpiece,\n1girl, smile"]) {
    for (const trigger of ["@yue", "glowing eyes", "a, b, c"]) {
      assert.equal(removeTrigger(insertTrigger(prompt, trigger), trigger), prompt.replace(/\s+$/, ""));
    }
  }
});
