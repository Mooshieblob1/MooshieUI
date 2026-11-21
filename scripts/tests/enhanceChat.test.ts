import assert from "node:assert/strict";
import test from "node:test";
import { CHAT_CLAUSE, joinReply, splitReply } from "../../src/lib/utils/enhanceChat.ts";
import {
  emptySessions,
  hasAnyTurns,
  MAX_SESSION_TURNS,
  parseStoredTabSessions,
  pruneTabSessions,
} from "../../src/lib/utils/enhancerSession.ts";

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

test("hasAnyTurns detects non-empty sessions across any flow", () => {
  const empty = emptySessions();
  assert.equal(hasAnyTurns(empty), false);
  const withNai = { ...empty, nai: [{ user: "u", assistant: "a", at: 1 }] };
  assert.equal(hasAnyTurns(withNai), true);
});

test("parseStoredTabSessions keeps valid non-empty tabs and caps turns", () => {
  const manyTurns = Array.from({ length: MAX_SESSION_TURNS + 5 }, (_, i) => ({
    user: `u${i}`,
    assistant: `a${i}`,
    at: i + 1,
  }));
  const raw = JSON.stringify({
    "tab-1": { nai: [{ user: "hi", assistant: "hello", at: 10 }, { bad: true }], enhance: [] },
    "tab-empty": { nai: [], enhance: [], h3: [], compose: [] },
    "": { nai: [{ user: "ignored", assistant: "ignored", at: 1 }] },
    "tab-capped": { enhance: manyTurns },
    "tab-bad": "not-an-object",
  });
  const parsed = parseStoredTabSessions(raw);
  assert.deepEqual(Object.keys(parsed).sort(), ["tab-1", "tab-capped"]);
  assert.deepEqual(parsed["tab-1"].nai, [{ user: "hi", assistant: "hello", at: 10 }]);
  assert.equal(parsed["tab-capped"].enhance.length, MAX_SESSION_TURNS);
  assert.equal(parsed["tab-capped"].enhance[0].at, 6);
  assert.deepEqual(parseStoredTabSessions("not json"), {});
  assert.deepEqual(parseStoredTabSessions(null), {});
});

test("pruneTabSessions keeps open parked non-empty tabs and drops closed or active tabs", () => {
  const turn = (at: number) => ({ user: `u${at}`, assistant: `a${at}`, at });
  const parked = {
    "tab-active": { ...emptySessions(), nai: [turn(1)] },
    "tab-parked": { ...emptySessions(), enhance: [turn(2)] },
    "tab-closed": { ...emptySessions(), h3: [turn(3)] },
    "tab-empty": emptySessions(),
  };
  const pruned = pruneTabSessions(parked, ["tab-active", "tab-parked", "tab-empty"], "tab-active");
  assert.deepEqual(Object.keys(pruned), ["tab-parked"]);
  assert.deepEqual(pruned["tab-parked"].enhance, [turn(2)]);
});
