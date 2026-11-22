import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_SHORTCUTS,
  canonicalCombo,
  detectConflict,
  effectiveComboFor,
  formatCombo,
  isTypingTarget,
  matchesCombo,
  normalizeCombo,
  sanitizeShortcutBindings,
  splitComboParts,
} from "../../src/lib/utils/shortcuts.ts";

function makeKey(overrides: Partial<{
  key: string;
  code: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}> = {}) {
  return {
    key: "",
    code: "",
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    ...overrides,
  };
}

test("DEFAULT_SHORTCUTS have unique IDs and no default conflicts", () => {
  const seen = new Set<string>();
  for (const def of DEFAULT_SHORTCUTS) {
    assert.equal(seen.has(def.id), false, `Duplicate shortcut id: ${def.id}`);
    seen.add(def.id);
    if (!def.readOnly) {
      const conflict = detectConflict(def.id, def.defaultCombo, {});
      assert.equal(
        conflict,
        null,
        `Default conflict for ${def.id} (${def.defaultCombo}) with ${conflict?.conflictingId}`,
      );
    }
  }
});

test("normalizeCombo normalizes modifiers, letters, Space, and Shift+?", () => {
  assert.equal(normalizeCombo(makeKey({ key: "Control", ctrlKey: true })), null);
  assert.equal(normalizeCombo(makeKey({ key: "Shift", shiftKey: true })), null);
  assert.equal(normalizeCombo(makeKey({ key: "Meta", metaKey: true })), null);

  assert.equal(normalizeCombo(makeKey({ key: "Enter", ctrlKey: true })), "Ctrl+Enter");
  assert.equal(normalizeCombo(makeKey({ key: "Enter", metaKey: true })), "Ctrl+Enter");
  assert.equal(normalizeCombo(makeKey({ key: "z", ctrlKey: true, shiftKey: true })), "Ctrl+Shift+Z");
  assert.equal(normalizeCombo(makeKey({ key: "b" })), "B");
  assert.equal(normalizeCombo(makeKey({ key: " ", code: "Space" })), "Space");
  assert.equal(normalizeCombo(makeKey({ key: "?", shiftKey: true })), "Shift+?");
  assert.equal(normalizeCombo(makeKey({ key: "ArrowUp", ctrlKey: true })), "Ctrl+ArrowUp");
});

test("canonicalCombo, splitComboParts, and formatCombo format combos cleanly", () => {
  assert.equal(canonicalCombo("shift+ctrl+z"), "Ctrl+Shift+Z");
  assert.equal(canonicalCombo("cmd+enter"), "Ctrl+Enter");
  assert.equal(canonicalCombo(" "), "Space");
  assert.equal(canonicalCombo("{}"), "{}");

  assert.deepEqual(splitComboParts("Ctrl+ArrowUp"), ["Ctrl", "↑"]);
  assert.deepEqual(splitComboParts("Ctrl+ArrowUp", true), ["Cmd", "↑"]);
  assert.equal(formatCombo("Ctrl+Enter"), "Ctrl+Enter");
  assert.equal(formatCombo("Ctrl+Enter", true), "Cmd+Enter");
  assert.equal(formatCombo("ArrowLeft"), "←");
  assert.equal(formatCombo("Escape"), "Esc");
  assert.equal(formatCombo("Delete"), "Del");
});

test("matchesCombo matches canonical combos and layout tolerances", () => {
  assert.equal(matchesCombo(makeKey({ key: "Enter", ctrlKey: true }), "Ctrl+Enter"), true);
  assert.equal(matchesCombo(makeKey({ key: "Enter", metaKey: true }), "Ctrl+Enter"), true);
  assert.equal(matchesCombo(makeKey({ key: "Enter" }), "Ctrl+Enter"), false);

  assert.equal(matchesCombo(makeKey({ key: "?", shiftKey: true }), "Shift+?"), true);
  assert.equal(matchesCombo(makeKey({ key: "?", shiftKey: false }), "Shift+?"), true);
  assert.equal(matchesCombo(makeKey({ key: "?", ctrlKey: true, shiftKey: true }), "Shift+?"), false);

  assert.equal(matchesCombo(makeKey({ key: "/", code: "Slash", ctrlKey: true }), "Ctrl+/"), true);
  assert.equal(matchesCombo(makeKey({ key: "-", code: "Slash", ctrlKey: true }), "Ctrl+/"), true);

  assert.equal(matchesCombo(makeKey({ key: "{", shiftKey: true }), "{}"), false);
});

test("detectConflict catches same-scope and overlapping-scope collisions", () => {
  // Same scope collision (lightbox: F vs N)
  const sameScope = detectConflict("lightbox.nsfw", "F", {});
  assert.equal(sameScope?.conflictingId, "lightbox.favourite");

  // Global collision from any scope
  const globalOverlap = detectConflict("gallery.delete_selected", "Shift+?", {});
  assert.equal(globalOverlap?.conflictingId, "global.cheat_sheet");

  // Generate vs Prompt overlap
  const genPrompt = detectConflict("prompt.find", "Ctrl+Enter", {});
  assert.equal(genPrompt?.conflictingId, "generate.submit");

  // Non-overlapping scopes allowed (Gallery vs Canvas both use Delete by default)
  const crossOk = detectConflict("gallery.delete_selected", "B", {});
  assert.equal(crossOk, null);

  // Respects custom bindings when checking conflict
  const customBindings = { "lightbox.favourite": "G" };
  assert.equal(detectConflict("lightbox.nsfw", "F", customBindings), null);
  assert.equal(detectConflict("lightbox.nsfw", "G", customBindings)?.conflictingId, "lightbox.favourite");

  // Read-only shortcuts cannot be collided with
  const readOnlyCollision = detectConflict("lightbox.nsfw", "Escape", {});
  assert.equal(readOnlyCollision?.conflictingId, "lightbox.close");
  assert.equal(readOnlyCollision?.def.readOnly, true);
});

test("sanitizeShortcutBindings and effectiveComboFor filter invalid or default entries", () => {
  const sanitized = sanitizeShortcutBindings({
    "lightbox.favourite": "g",
    "lightbox.nsfw": "N", // same as default -> dropped
    "lightbox.close": "X", // readOnly -> dropped
    "unknown.action": "K", // unknown -> dropped
    "gallery.select_all": "", // empty -> dropped
  });
  assert.deepEqual(sanitized, { "lightbox.favourite": "G" });

  const favDef = DEFAULT_SHORTCUTS.find((d) => d.id === "lightbox.favourite")!;
  assert.equal(effectiveComboFor(favDef, sanitized), "G");
  assert.equal(effectiveComboFor(favDef, {}), "F");
});

test("isTypingTarget identifies editable elements safely", () => {
  assert.equal(isTypingTarget(null), false);
  assert.equal(isTypingTarget({ tagName: "BUTTON" }), false);
  assert.equal(isTypingTarget({ tagName: "INPUT" }), true);
  assert.equal(isTypingTarget({ tagName: "textarea" }), true);
  assert.equal(isTypingTarget({ tagName: "SELECT" }), true);
  assert.equal(isTypingTarget({ tagName: "DIV", isContentEditable: true }), true);
  assert.equal(
    isTypingTarget({
      tagName: "SPAN",
      closest: (sel: string) => (sel.includes("contenteditable") ? {} : null),
    }),
    true,
  );
});
