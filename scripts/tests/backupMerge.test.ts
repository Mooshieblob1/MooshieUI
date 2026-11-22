import assert from "node:assert/strict";
import test from "node:test";
import {
  mergeCharacterPortrait,
  mergeComicBooks,
  mergeGenerationTabs,
  mergeLocalExtras,
  mergeShortcuts,
  mergeWildcardsOrder,
} from "../../src/lib/utils/backupMerge.ts";
import { mergeUserPrefs } from "../../src/lib/utils/prefsMerge.ts";

test("mergeUserPrefs merges lora_presets presets and triggers together", () => {
  const merged = mergeUserPrefs(
    {
      lora_presets: {
        presets: [{ id: "lp1", name: "Primary", loras: [], createdAt: 10, updatedAt: 20 }],
        triggers: { "a.safetensors": "alpha_primary", "shared.safetensors": "primary_tag" },
      },
    },
    {
      lora_presets: {
        presets: [{ id: "lp2", name: "Secondary", loras: [], createdAt: 10, updatedAt: 15 }],
        triggers: { "b.safetensors": "beta_secondary", "shared.safetensors": "secondary_tag" },
      },
    },
  );
  const lp = merged.lora_presets as {
    presets: Array<{ id: string }>;
    triggers: Record<string, string>;
  };
  assert.deepEqual(
    lp.presets.map((p) => p.id),
    ["lp1", "lp2"],
  );
  assert.deepEqual(lp.triggers, {
    "a.safetensors": "alpha_primary",
    "b.safetensors": "beta_secondary",
    "shared.safetensors": "primary_tag",
  });
});

test("mergeComicBooks unions books by id and keeps valid activeBookId", () => {
  const merged = mergeComicBooks(
    { books: [{ id: "b1", title: "One" }], activeBookId: "b1" },
    { books: [{ id: "b1", title: "Old" }, { id: "b2", title: "Two" }], activeBookId: "b2" },
  );
  assert.equal(merged.activeBookId, "b1");
  assert.equal(merged.books?.length, 2);
  assert.equal((merged.books?.[0] as { title: string }).title, "One");
  assert.equal((merged.books?.[1] as { title: string }).title, "Two");
});

test("mergeGenerationTabs unions tabs by id and drops inactive tabs without snapshot", () => {
  const merged = mergeGenerationTabs(
    { activeId: "t1", tabs: [{ id: "t1", snapshot: null }, { id: "t2", snapshot: { steps: 20 } }] },
    { activeId: "t3", tabs: [{ id: "t3", snapshot: null }, { id: "t4", snapshot: { steps: 30 } }] },
  );
  assert.equal(merged?.activeId, "t1");
  assert.deepEqual(
    (merged?.tabs as Array<{ id: string }>).map((t) => t.id),
    ["t1", "t2", "t4"],
  );
});

test("mergeCharacterPortrait, mergeWildcardsOrder, and mergeShortcuts combine cleanly", () => {
  const portrait = mergeCharacterPortrait(
    { artists: "", chunkIds: ["c1"], styleIds: ["s1"] },
    { artists: "krenz", chunkIds: ["c1", "c2"], styleIds: ["s2"] },
  );
  assert.deepEqual(portrait, {
    artists: "krenz",
    chunkIds: ["c1", "c2"],
    styleIds: ["s1", "s2"],
  });

  const wc = mergeWildcardsOrder(
    { ordered: ["hair"], counters: { hair: 3 } },
    { ordered: ["hair", "eyes"], counters: { hair: 1, eyes: 2 } },
  );
  assert.deepEqual(wc, {
    ordered: ["hair", "eyes"],
    counters: { hair: 3, eyes: 2 },
  });

  assert.deepEqual(
    mergeShortcuts({ generate: "Ctrl+G" }, { generate: "Ctrl+Enter", gallery_all: "Ctrl+A" }),
    { generate: "Ctrl+G", gallery_all: "Ctrl+A" },
  );
});

test("mergeLocalExtras only merges selected keys", () => {
  const out = mergeLocalExtras(
    { comic: { books: [{ id: "b1" }], activeBookId: "b1" }, shortcuts: { a: "X" } },
    { comic: { books: [{ id: "b2" }], activeBookId: "b2" }, shortcuts: { b: "Y" } },
    ["comic"],
  );
  assert.equal(out.comic?.books?.length, 2);
  assert.deepEqual(out.shortcuts, { a: "X" });
});
