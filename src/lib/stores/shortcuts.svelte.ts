import { triggerSync } from "../utils/syncTrigger.js";
import { userScopedKey } from "../utils/ipc.js";
import {
  DEFAULT_SHORTCUT_MAP,
  canonicalCombo,
  effectiveComboFor,
  matchesCombo,
  sanitizeShortcutBindings,
  type KeyEventLike,
} from "../utils/shortcuts.js";

const SHORTCUTS_SETTINGS_KEY = "mooshieui.shortcuts.v1";

class ShortcutsStore {
  customBindings = $state<Record<string, string>>({});
  cheatSheetOpen = $state(false);

  constructor() {
    this.loadSettings();
  }

  loadSettings(): void {
    try {
      const raw = localStorage.getItem(userScopedKey(SHORTCUTS_SETTINGS_KEY));
      if (!raw) {
        this.customBindings = {};
        return;
      }
      const parsed = JSON.parse(raw) as { customBindings?: unknown } | null;
      this.customBindings = sanitizeShortcutBindings(parsed?.customBindings);
    } catch (e) {
      console.error("Failed to load keyboard shortcuts:", e);
      this.customBindings = {};
    }
  }

  saveSettings(): void {
    try {
      localStorage.setItem(
        userScopedKey(SHORTCUTS_SETTINGS_KEY),
        JSON.stringify({ customBindings: this.customBindings }),
      );
      triggerSync();
    } catch (e) {
      console.error("Failed to save keyboard shortcuts:", e);
    }
  }

  effectiveCombo(id: string): string {
    const def = DEFAULT_SHORTCUT_MAP.get(id);
    if (!def) return "";
    return effectiveComboFor(def, this.customBindings);
  }

  isCustomized(id: string): boolean {
    const def = DEFAULT_SHORTCUT_MAP.get(id);
    if (!def || def.readOnly) return false;
    return id in this.customBindings && this.customBindings[id] !== canonicalCombo(def.defaultCombo);
  }

  matches(id: string, e: KeyEventLike): boolean {
    const combo = this.effectiveCombo(id);
    if (!combo) return false;
    return matchesCombo(e, combo);
  }

  setBinding(id: string, combo: string): void {
    const def = DEFAULT_SHORTCUT_MAP.get(id);
    if (!def || def.readOnly) return;
    const normalized = canonicalCombo(combo);
    if (!normalized) return;

    const next = { ...this.customBindings };
    if (normalized === canonicalCombo(def.defaultCombo)) {
      delete next[id];
    } else {
      next[id] = normalized;
    }
    this.customBindings = next;
    this.saveSettings();
  }

  swapBindings(actionId: string, newCombo: string, conflictId: string): void {
    const targetDef = DEFAULT_SHORTCUT_MAP.get(actionId);
    const conflictDef = DEFAULT_SHORTCUT_MAP.get(conflictId);
    if (!targetDef || targetDef.readOnly || !conflictDef || conflictDef.readOnly) return;

    const normalizedNew = canonicalCombo(newCombo);
    if (!normalizedNew) return;
    const previousTargetCombo = effectiveComboFor(targetDef, this.customBindings);

    const next = { ...this.customBindings };
    if (normalizedNew === canonicalCombo(targetDef.defaultCombo)) {
      delete next[actionId];
    } else {
      next[actionId] = normalizedNew;
    }

    const normalizedPrev = canonicalCombo(previousTargetCombo);
    if (!normalizedPrev || normalizedPrev === canonicalCombo(conflictDef.defaultCombo)) {
      delete next[conflictId];
    } else {
      next[conflictId] = normalizedPrev;
    }

    this.customBindings = next;
    this.saveSettings();
  }

  resetBinding(id: string): void {
    if (!(id in this.customBindings)) return;
    const next = { ...this.customBindings };
    delete next[id];
    this.customBindings = next;
    this.saveSettings();
  }

  resetAll(): void {
    if (Object.keys(this.customBindings).length === 0) return;
    this.customBindings = {};
    this.saveSettings();
  }

  openCheatSheet(): void {
    this.cheatSheetOpen = true;
  }

  closeCheatSheet(): void {
    this.cheatSheetOpen = false;
  }

  toggleCheatSheet(): void {
    this.cheatSheetOpen = !this.cheatSheetOpen;
  }

  collectPrefs(): unknown {
    return {
      customBindings: { ...this.customBindings },
    };
  }

  applyServerPrefs(data: unknown): void {
    try {
      const parsed = data && typeof data === "object" ? (data as { customBindings?: unknown }) : null;
      this.customBindings = sanitizeShortcutBindings(parsed?.customBindings);
      this.saveSettings();
    } catch (e) {
      console.error("Failed to apply server prefs (shortcuts):", e);
    }
  }
}

export const shortcuts = new ShortcutsStore();
