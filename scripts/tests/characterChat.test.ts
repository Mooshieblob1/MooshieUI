import assert from "node:assert/strict";
import test from "node:test";
import {
  characterChatSystemPrompt,
  characterChatUserPrompt,
  parseCharacterChatResponse,
  serializeCharacterChatProposal,
  type CharacterChatTarget,
} from "../../src/lib/utils/characterChat.ts";
import { newMemoryFile } from "../../src/lib/utils/characterMemory.ts";

const naiTarget: CharacterChatTarget = {
  name: "Ani",
  prompt: "girl, long blonde hair, twintails, blue eyes, fang, grey hoodie",
  negative: "red eyes",
  architecture: "novelai",
  architectureLabel: "NovelAI",
  promptStyle: "tags",
};

test("characterChatSystemPrompt tailors instructions per architecture, skill, memory, and session", () => {
  const naiSys = characterChatSystemPrompt(naiTarget, {
    useMemory: true,
    inSession: true,
    naiSkill: "Keep identity tags concise.",
  });
  assert.match(naiSys, /NovelAI V5 Character Card/i);
  assert.match(naiSys, /Keep identity tags concise\./);
  assert.match(naiSys, /MEMORY:/);
  assert.match(naiSys, /Earlier messages are previous requests in this session/);

  const naturalTarget: CharacterChatTarget = {
    name: "Aria",
    prompt: "tall silver-haired knight in steel plate",
    negative: "",
    architecture: "flux",
    architectureLabel: "Flux",
    promptStyle: "natural",
  };
  const naturalSys = characterChatSystemPrompt(naturalTarget, {
    useMemory: false,
    inSession: false,
  });
  assert.match(naturalSys, /Natural-language Character Card/i);
  assert.match(naturalSys, /Do NOT output a MEMORY:/);
  assert.ok(!naturalSys.includes("Earlier messages are previous requests"));
});

test("characterChatUserPrompt includes card state, parent variant info, memory, and reference labels", () => {
  const memFile = newMemoryFile("relationships.md", "- Sister of Yuu", []);
  const user = characterChatUserPrompt("give her a winter coat", {
    ...naiTarget,
    variantOf: { name: "Ani Base", prompt: "girl, long blonde hair, blue eyes" },
    memoryFiles: [memFile],
    memoryBudgetChars: 2000,
  }, {
    useMemory: true,
    referenceLabels: ["coat ref", ""],
  });
  assert.match(user, /Current character: Ani \(NovelAI\)/);
  assert.match(user, /Variant of: Ani Base/);
  assert.match(user, /Sister of Yuu/);
  assert.match(user, /Reference 1: coat ref/);
  assert.match(user, /Reference 2/);
  assert.match(user, /User request:\ngive her a winter coat/);
});

test("parseCharacterChatResponse handles reply-only, labeled edits, memory lines, and JSON fallback", () => {
  const replyOnly = parseCharacterChatResponse(
    "<reply>Here are three pastel jacket ideas for Ani.</reply>",
    "Ani",
    "red eyes",
  );
  assert.equal(replyOnly.reply, "Here are three pastel jacket ideas for Ani.");
  assert.equal(replyOnly.proposal, null);

  const edited = parseCharacterChatResponse(
    `<reply>Swapped the hoodie for a navy peacoat and noted her new scarf.</reply>
VARIANT: winter coat
PROMPT: girl, long blonde hair, twintails, blue eyes, fang, navy peacoat, red plaid scarf
NEGATIVE: hoodie, red eyes
MEMORY:
- wardrobe.md | Always wears a red plaid scarf in winter
- Ani | lore.md | Dislikes heavy wool mittens`,
    "Ani",
    "red eyes",
  );
  assert.equal(edited.reply, "Swapped the hoodie for a navy peacoat and noted her new scarf.");
  assert.ok(edited.proposal);
  assert.equal(edited.proposal.variant, "winter coat");
  assert.equal(
    edited.proposal.prompt,
    "girl, long blonde hair, twintails, blue eyes, fang, navy peacoat, red plaid scarf",
  );
  assert.equal(edited.proposal.negative, "hoodie, red eyes");
  assert.deepEqual(edited.proposal.memory, [
    { name: "Ani", file: "wardrobe.md", fact: "Always wears a red plaid scarf in winter" },
    { name: "Ani", file: "lore.md", fact: "Dislikes heavy wool mittens" },
  ]);

  const serialized = serializeCharacterChatProposal(edited.proposal);
  const roundTrip = parseCharacterChatResponse(serialized, "Ani", "red eyes");
  assert.deepEqual(roundTrip.proposal?.prompt, edited.proposal.prompt);
  assert.deepEqual(roundTrip.proposal?.negative, edited.proposal.negative);
  assert.deepEqual(roundTrip.proposal?.variant, edited.proposal.variant);

  const jsonParsed = parseCharacterChatResponse(
    '```json\n{"reply":"Trimmed her hair.","prompt":"girl, short blonde hair, blue eyes","negative":""}\n```',
    "Ani",
    "red eyes",
  );
  assert.equal(jsonParsed.reply, "Trimmed her hair.");
  assert.equal(jsonParsed.proposal?.prompt, "girl, short blonde hair, blue eyes");
  assert.equal(jsonParsed.proposal?.negative, "");
});
