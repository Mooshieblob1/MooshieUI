import assert from "node:assert/strict";
import test from "node:test";
import { naiRewriteSystemPrompt, naiRewriteSystemPromptParts } from "../../src/lib/utils/naiPrompt.ts";

test("naiRewriteSystemPromptParts splits a stable cacheable prefix and preserves exact prompt", () => {
  const baseCtx = {
    variant: "full" as const,
    ucPreset: 0,
    characterCount: 1,
    existing: null,
    language: "en" as const,
    references: [],
    inSession: false,
  };
  const first = naiRewriteSystemPromptParts(baseCtx);
  assert.equal(
    [first.cached, first.dynamic].filter(Boolean).join("\n\n"),
    naiRewriteSystemPrompt(baseCtx),
  );
  // Static V5 spec is well above Anthropic's cache minimum (~4.7k tokens).
  assert.ok(first.cached.length > 15_000, `expected >15k chars, got ${first.cached.length}`);

  // Changing session state, saved characters, existing prompt, or references
  // leaves the cached static prefix byte-for-byte identical.
  const second = naiRewriteSystemPromptParts({
    ...baseCtx,
    inSession: true,
    existing: { base: "1girl", uc: "", characters: ["girl, blue hair"] },
    references: ["outfit"],
    savedCharacters: "SAVED CHARACTERS\n- Name: Alice\n  Appearance: 1girl",
  });
  assert.equal(second.cached, first.cached);
  assert.notEqual(second.dynamic, first.dynamic);
  assert.equal(
    [second.cached, second.dynamic].filter(Boolean).join("\n\n"),
    naiRewriteSystemPrompt({
      ...baseCtx,
      inSession: true,
      existing: { base: "1girl", uc: "", characters: ["girl, blue hair"] },
      references: ["outfit"],
      savedCharacters: "SAVED CHARACTERS\n- Name: Alice\n  Appearance: 1girl",
    }),
  );
});
