import assert from "node:assert/strict";
import test from "node:test";
import { CHAT_CLAUSE, joinReply, splitReply } from "../../src/lib/utils/enhanceChat.ts";

test("a reply before the prompt is split off", () => {
  assert.deepEqual(splitReply("<reply>Made it rainy.</reply>\n1girl, rain, night"), {
    reply: "Made it rainy.",
    prompt: "1girl, rain, night",
  });
});

test("a reply alone has no prompt, and a bare prompt no reply", () => {
  assert.deepEqual(splitReply("<REPLY>Use fewer tags.</REPLY>"), { reply: "Use fewer tags.", prompt: "" });
  assert.deepEqual(splitReply("1girl, solo"), { reply: "", prompt: "1girl, solo" });
});

test("an unclosed reply runs to the first blank line", () => {
  assert.deepEqual(splitReply("<reply>Swapped the dress.\n\nBASE:\n1girl, red dress"), {
    reply: "Swapped the dress.",
    prompt: "BASE:\n1girl, red dress",
  });
  assert.deepEqual(splitReply("<reply>Just a thought"), { reply: "Just a thought", prompt: "" });
});

test("a reply after the prompt and a stray closing tag", () => {
  assert.deepEqual(splitReply("1girl, beach\n<reply>Added a beach.</reply>"), {
    reply: "Added a beach.",
    prompt: "1girl, beach",
  });
  assert.equal(splitReply("1girl</reply>").prompt, "1girl");
});

test("the stored shape splits back into the same parts", () => {
  for (const [reply, prompt] of [["Hi", "1girl"], ["", "1girl"], ["Hi", ""]]) {
    assert.deepEqual(splitReply(joinReply(reply, prompt)), { reply, prompt });
  }
});

test("the clause names the block the splitter reads", () => {
  assert.ok(CHAT_CLAUSE.includes("<reply>...</reply>"));
});
