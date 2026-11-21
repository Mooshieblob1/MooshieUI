import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_MEMORY_BUDGET,
  LOCAL_LARGE_MEMORY_BUDGET,
  LOCAL_SMALL_MEMORY_BUDGET,
  buildPendingLearnedFacts,
  memoryBudgetFor,
  memoryForRequest,
  type CharacterMemoryFile,
} from "../../src/lib/utils/characterMemory.ts";

function file(id: string, name: string, content: string): CharacterMemoryFile {
  return { id, name, content, updatedAt: 1 };
}

test("memoryBudgetFor picks tighter limits for smaller bundled local models", () => {
  assert.deepEqual(memoryBudgetFor(true, "qwen3-4b-instruct"), DEFAULT_MEMORY_BUDGET);
  assert.deepEqual(memoryBudgetFor(false, "qwen3-4b-instruct"), LOCAL_SMALL_MEMORY_BUDGET);
  assert.deepEqual(memoryBudgetFor(false, "qwen25-7b-instruct"), LOCAL_SMALL_MEMORY_BUDGET);
  assert.deepEqual(memoryBudgetFor(false, "qwen38-27b"), LOCAL_LARGE_MEMORY_BUDGET);
  assert.ok(LOCAL_SMALL_MEMORY_BUDGET.total < LOCAL_LARGE_MEMORY_BUDGET.total);
  assert.ok(LOCAL_LARGE_MEMORY_BUDGET.total < DEFAULT_MEMORY_BUDGET.total);
});

test("memoryForRequest trims character notes to the supplied budget", () => {
  const longNote = "a".repeat(5000);
  const cards = [
    { id: "1", ownerId: "1", name: "Alice", prompt: "1girl", memory: [file("f1", "notes.md", longNote)] },
    { id: "2", ownerId: "2", name: "Bob", prompt: "1boy", memory: [file("f2", "notes.md", longNote)] },
    { id: "3", ownerId: "3", name: "Carol", prompt: "1girl", memory: [file("f3", "notes.md", longNote)] },
  ];
  const picks = memoryForRequest(cards, ["Alice, Bob, Carol"], "", LOCAL_SMALL_MEMORY_BUDGET);
  assert.equal(picks.length, LOCAL_SMALL_MEMORY_BUDGET.maxCharacters);
  assert.ok(picks.every((p) => p.notes.length <= LOCAL_SMALL_MEMORY_BUDGET.perCharacter + 40));
  const totalLen = picks.reduce((sum, p) => sum + p.notes.length, 0);
  assert.ok(totalLen <= LOCAL_SMALL_MEMORY_BUDGET.total + 80);
});

test("buildPendingLearnedFacts filters duplicates and matches owners for review", () => {
  const alice = {
    id: "alice",
    name: "Alice (casual)",
    memory: [file("f1", "personality.md", "- Cheerful and energetic")],
  };
  const owner = { ...alice };
  const learned = [
    { name: "Alice", file: "personality.md", fact: "Cheerful and energetic" },
    { name: "Alice", file: "outfits.md", fact: "Wears a red scarf in winter" },
    { name: "Alice", file: "outfits.md", fact: "Wears a red scarf in winter" },
    { name: "Unknown", file: "notes.md", fact: "Should be ignored" },
  ];
  const pending = buildPendingLearnedFacts(learned, [alice], () => owner);
  assert.equal(pending.length, 1);
  assert.equal(pending[0].ownerId, "alice");
  assert.equal(pending[0].file, "outfits.md");
  assert.equal(pending[0].fact, "Wears a red scarf in winter");
  assert.equal(pending[0].keep, true);
});
