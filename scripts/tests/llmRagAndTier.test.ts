import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_MEMORY_BUDGET,
  LOCAL_LARGE_MEMORY_BUDGET,
  LOCAL_SMALL_MEMORY_BUDGET,
  memoryBudgetFor,
  memoryContext,
  type CharacterMemoryFile,
} from "../../src/lib/utils/characterMemory.ts";
import { isProviderRefusal, isTextRefusal } from "../../src/lib/utils/contentSafety.ts";
import { h3RewriteSystemPrompt } from "../../src/lib/utils/h3Prompt.ts";
import { resolveLlmModelTier, supportsExternalJsonMode } from "../../src/lib/utils/llmTier.ts";
import {
  balanceEmphasisBrackets,
  normalizeNaiResponse,
  parseNaiEnvelope,
  stripRestatedBoxTagsFromBase,
} from "../../src/lib/utils/naiParse.ts";
import { naiRewriteSystemPromptParts, selectNaiRuleModules } from "../../src/lib/utils/naiPrompt.ts";

test("resolveLlmModelTier classifies local, open-weight, mini, and frontier models", () => {
  assert.equal(
    resolveLlmModelTier({ setting: "full", usesExternal: false, localModelId: "qwen3-4b-instruct" }),
    "full",
  );
  assert.equal(
    resolveLlmModelTier({ setting: "auto", usesExternal: false, localModelId: "qwen3-4b-instruct" }),
    "compact",
  );
  assert.equal(
    resolveLlmModelTier({ setting: "auto", usesExternal: false, localModelId: "qwen3-vl-8b-instruct" }),
    "compact",
  );
  assert.equal(
    resolveLlmModelTier({ setting: "auto", usesExternal: false, localModelId: "qwen36-35b-a3b" }),
    "standard",
  );
  assert.equal(
    resolveLlmModelTier({ setting: "auto", usesExternal: true, provider: "custom", model: "qwen3:8b" }),
    "compact",
  );
  assert.equal(
    resolveLlmModelTier({ setting: "auto", usesExternal: true, provider: "openrouter", model: "qwen/qwen3-32b" }),
    "standard",
  );
  assert.equal(
    resolveLlmModelTier({ setting: "auto", usesExternal: true, provider: "nous", model: "Hermes-4-405B" }),
    "full",
  );
  assert.equal(
    resolveLlmModelTier({ setting: "auto", usesExternal: true, provider: "openai", model: "gpt-4o-mini" }),
    "standard",
  );
  assert.equal(
    resolveLlmModelTier({ setting: "auto", usesExternal: true, provider: "anthropic", model: "claude-haiku-4-5" }),
    "standard",
  );
  assert.equal(
    resolveLlmModelTier({
      setting: "auto",
      usesExternal: true,
      provider: "custom",
      baseUrl: "http://127.0.0.1:11434/v1",
      model: "my-local-model",
    }),
    "standard",
  );
  assert.equal(
    resolveLlmModelTier({ setting: "auto", usesExternal: true, provider: "xai", model: "grok-3" }),
    "full",
  );
});

test("supportsExternalJsonMode skips providers without OpenAI response_format", () => {
  assert.equal(supportsExternalJsonMode("openai"), true);
  assert.equal(supportsExternalJsonMode("xai"), true);
  assert.equal(supportsExternalJsonMode("openrouter"), true);
  assert.equal(supportsExternalJsonMode("nous"), true);
  assert.equal(supportsExternalJsonMode("custom"), true);
  assert.equal(supportsExternalJsonMode("anthropic"), false);
  assert.equal(supportsExternalJsonMode("chatgpt"), false);
  assert.equal(supportsExternalJsonMode("gemini-cli"), false);
});

test("naiRewriteSystemPromptParts slices optional modules on compact while preserving full prefix", () => {
  const simpleCtx = {
    variant: "full" as const,
    ucPreset: 0,
    characterCount: 1,
    existing: null,
    language: "en" as const,
    references: [],
    requestText: "1girl smiling in a spring park",
  };
  const fullParts = naiRewriteSystemPromptParts({ ...simpleCtx, tier: "full", ragContext: "GROUNDED REFERENCE\n..." });
  const compactParts = naiRewriteSystemPromptParts({
    ...simpleCtx,
    tier: "compact",
    structuredJson: true,
    ragContext: "GROUNDED REFERENCE\n...",
  });
  assert.ok(compactParts.cached.length < fullParts.cached.length * 0.45);
  assert.equal(compactParts.cached.includes("MULTI-CHARACTER"), false);
  assert.equal(compactParts.cached.includes("LAYOUT & OCCLUSION"), false);
  assert.equal(compactParts.dynamic.includes("OUTPUT FORMAT (JSON ONLY)"), true);
  assert.equal(fullParts.dynamic.includes("GROUNDED REFERENCE"), true);

  const triggered = selectNaiRuleModules({
    ...simpleCtx,
    characterCount: 2,
    requestText: 'Two girls behind a table holding a sign saying "HELLO" with transparent background',
  });
  assert.equal(triggered.multiChar, true);
  assert.equal(triggered.occlusion, true);
  assert.equal(triggered.comicsAndText, true);
  assert.equal(triggered.transparency, true);

  const standardMulti = naiRewriteSystemPromptParts({
    ...simpleCtx,
    tier: "standard",
    characterCount: 2,
    requestText: "two girls hugging at a table",
  });
  assert.equal(standardMulti.cached.includes("MULTI-CHARACTER EXAMPLE"), true);
  assert.equal(standardMulti.cached.includes("LAYOUT & OCCLUSION"), true);
});

test("h3RewriteSystemPrompt trims unused dialogue/sign rules on compact tier", () => {
  const baseCtx = {
    taskType: "i2va" as const,
    frames: 124,
    durationSeconds: "5.17",
    referenceImageCount: 0,
    requestText: "gentle camera push in on wind swaying tree branches",
  };
  const full = h3RewriteSystemPrompt({ ...baseCtx, tier: "full" });
  const compact = h3RewriteSystemPrompt({ ...baseCtx, tier: "compact" });
  assert.ok(compact.length < full.length);
  assert.equal(compact.includes("<scenetrans>"), false);
  assert.equal(compact.includes("overall_soundscape:"), true);

  const spoken = h3RewriteSystemPrompt({
    ...baseCtx,
    tier: "compact",
    requestText: 'girl shouts "Look out!" across the street',
  });
  assert.equal(spoken.includes("<d>[English]"), true);
});

test("parseNaiEnvelope handles think tags, fenced JSON, chat replies, bracket balance, and base dedup", () => {
  const raw = `<think>reasoning about prompt</think>
\`\`\`json
{
  "base_tags": "2girls, high complexity, red hair, blue dress, park",
  "base_prose": "Two girls stand together under sunlit trees. The girl in the red coat stands on the left.",
  "text": "WAIT UP",
  "uc": "watermark",
  "characters": [
    { "prompt": "girl, red hair, green eyes, {smile, red coat, on the left", "uc": "blue dress" },
    { "prompt": "girl, black hair, blue dress, on the right", "uc": "red coat" }
  ],
  "note": "",
  "reply": "Updated the scene for two characters."
}
\`\`\``;
  const env = parseNaiEnvelope(raw, true);
  assert.equal(env.reply, "Updated the scene for two characters.");
  assert.equal(env.parsed.characters.length, 2);
  assert.equal(env.parsed.base.includes("Text:\nWAIT UP"), true);

  const normalized = normalizeNaiResponse(env.parsed);
  assert.equal(normalized.characters[0].includes("{smile"), true);
  assert.equal(normalized.characters[0].includes("}"), true);
  const cleanedBase = stripRestatedBoxTagsFromBase(normalized.base, normalized.characters);
  assert.equal(cleanedBase.startsWith("2girls, high complexity, park"), true);
  assert.equal(cleanedBase.includes("red hair"), false);
  assert.equal(cleanedBase.includes("red coat"), true);

  assert.equal(balanceEmphasisBrackets("{smile}}"), "{smile}");
  assert.equal(stripRestatedBoxTagsFromBase("1girl, red dress\n\nShe smiles.", ["girl, red dress"]), "1girl, red dress\n\nShe smiles.");
});

test("characterMemory ranks query-matching files/lines under tight budget", () => {
  assert.deepEqual(memoryBudgetFor(true, null, "compact"), LOCAL_SMALL_MEMORY_BUDGET);
  assert.deepEqual(memoryBudgetFor(true, null, "standard"), LOCAL_LARGE_MEMORY_BUDGET);
  assert.deepEqual(memoryBudgetFor(false, "qwen3-4b-instruct", "full"), DEFAULT_MEMORY_BUDGET);

  const files: CharacterMemoryFile[] = [
    { id: "1", name: "backstory.md", content: "- Childhood history\n".repeat(40), updatedAt: 1 },
    { id: "2", name: "outfits.md", content: "- Always carries an umbrella and wears a navy trenchcoat\n", updatedAt: 2 },
  ];
  const ctx = memoryContext(files, 240, "rainy street wearing trenchcoat");
  assert.equal(ctx.includes("navy trenchcoat"), true);
});

test("isProviderRefusal and isTextRefusal identify safety refusals without flagging real prompts", () => {
  assert.equal(isProviderRefusal("403 permission-denied: can't help with that"), true);
  assert.equal(isProviderRefusal("HTTP 400 content policy violation"), true);
  assert.equal(isProviderRefusal("provider_refused"), true);
  assert.equal(isTextRefusal("I'm sorry, I cannot assist with that request."), true);
  assert.equal(isTextRefusal("BASE:\n1girl, smile\n\nShe stands calmly."), false);
  assert.equal(isTextRefusal('{"base_tags":"1girl"}'), false);
});
