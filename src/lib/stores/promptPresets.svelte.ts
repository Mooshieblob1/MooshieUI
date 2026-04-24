/**
 * Prompt Presets store.
 *
 * A "Preset" is a named chunk of prompt text (any tags, not just artists)
 * that the user can activate to inject into generations. On activation the
 * user picks a mode:
 *   - "prepend"  — injected at the start of the positive prompt
 *   - "append"   — injected at the end
 *   - "wildcard" — exactly ONE of the comma-separated tags inside `content`
 *                  is chosen at random for each generation (re-rolled every
 *                  time `resolve()` is called)
 *
 * Like Artist Styles, presets live outside the prompt textbox — users see
 * badges, not tags — and survive reloads via localStorage.
 */

const STORAGE_KEY = "mooshieui.promptPresets.v1";
const ACTIVE_KEY = "mooshieui.promptPresets.active.v1";
const EXPORT_KIND = "mooshieui.prompt-presets";
const EXPORT_VERSION = 1;

export type PresetMode = "prepend" | "append" | "wildcard";

export interface PromptPreset {
  id: string;
  name: string;
  /** Free-form prompt text. For wildcard mode, this is split on commas/newlines. */
  content: string;
  createdAt: number;
  updatedAt: number;
}

export interface ActivePreset {
  id: string;
  mode: PresetMode;
}

interface PersistedState {
  version: number;
  presets: PromptPreset[];
}

export interface PresetsExport {
  kind: typeof EXPORT_KIND;
  version: number;
  exportedAt: string;
  presets: PromptPreset[];
}

function genId(): string {
  return `pst_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

function sanitizePreset(raw: any): PromptPreset | null {
  if (!raw || typeof raw.id !== "string" || typeof raw.name !== "string") return null;
  const now = Date.now();
  return {
    id: raw.id,
    name: raw.name.trim() || "Untitled preset",
    content: typeof raw.content === "string" ? raw.content : "",
    createdAt: typeof raw.createdAt === "number" && raw.createdAt > 0 ? raw.createdAt : now,
    updatedAt: typeof raw.updatedAt === "number" && raw.updatedAt > 0 ? raw.updatedAt : now,
  };
}

/** Split wildcard content into individual tag candidates. */
function splitWildcardChoices(content: string): string[] {
  return content
    .split(/[,\n]/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

class PromptPresetsStore {
  presets = $state<PromptPreset[]>([]);
  active = $state<ActivePreset[]>([]);

  constructor() {
    this.loadSettings();
  }

  // ---------------------------------------------------------------------------
  // Persistence
  // ---------------------------------------------------------------------------

  private loadSettings() {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw) {
        const parsed = JSON.parse(raw) as Partial<PersistedState>;
        if (parsed && Array.isArray(parsed.presets)) {
          this.presets = parsed.presets.map(sanitizePreset).filter(Boolean) as PromptPreset[];
        }
      }
    } catch (e) {
      console.error("prompt-presets: load failed", e);
    }
    try {
      const raw = localStorage.getItem(ACTIVE_KEY);
      if (raw) {
        const parsed = JSON.parse(raw) as ActivePreset[];
        if (Array.isArray(parsed)) {
          const ids = new Set(this.presets.map((p) => p.id));
          this.active = parsed.filter(
            (a): a is ActivePreset =>
              !!a &&
              typeof a.id === "string" &&
              ids.has(a.id) &&
              (a.mode === "prepend" || a.mode === "append" || a.mode === "wildcard"),
          );
        }
      }
    } catch (e) {
      console.error("prompt-presets: load active failed", e);
    }
  }

  private saveSettings() {
    try {
      const payload: PersistedState = { version: EXPORT_VERSION, presets: this.presets };
      localStorage.setItem(STORAGE_KEY, JSON.stringify(payload));
    } catch (e) {
      console.error("prompt-presets: save failed", e);
    }
  }

  private saveActive() {
    try {
      localStorage.setItem(ACTIVE_KEY, JSON.stringify(this.active));
    } catch (e) {
      console.error("prompt-presets: save active failed", e);
    }
  }

  // ---------------------------------------------------------------------------
  // Queries
  // ---------------------------------------------------------------------------

  getById(id: string): PromptPreset | null {
    return this.presets.find((p) => p.id === id) ?? null;
  }

  activeMode(id: string): PresetMode | null {
    return this.active.find((a) => a.id === id)?.mode ?? null;
  }

  isActive(id: string): boolean {
    return this.active.some((a) => a.id === id);
  }

  get activeEntries(): Array<{ preset: PromptPreset; mode: PresetMode }> {
    const byId = new Map(this.presets.map((p) => [p.id, p]));
    const out: Array<{ preset: PromptPreset; mode: PresetMode }> = [];
    for (const a of this.active) {
      const preset = byId.get(a.id);
      if (preset) out.push({ preset, mode: a.mode });
    }
    return out;
  }

  /**
   * Resolve active presets into concrete text fragments. Wildcard mode picks
   * one random choice from the preset content. Called once per generation.
   */
  resolve(): { prepend: string; append: string } {
    const prepends: string[] = [];
    const appends: string[] = [];
    for (const { preset, mode } of this.activeEntries) {
      if (mode === "wildcard") {
        const choices = splitWildcardChoices(preset.content);
        if (choices.length === 0) continue;
        const picked = choices[Math.floor(Math.random() * choices.length)];
        // Wildcard picks are appended by convention (same semantic weight
        // regardless of position within the prompt).
        appends.push(picked);
      } else if (mode === "prepend") {
        const text = preset.content.trim();
        if (text) prepends.push(text);
      } else {
        const text = preset.content.trim();
        if (text) appends.push(text);
      }
    }
    return {
      prepend: prepends.join(", "),
      append: appends.join(", "),
    };
  }

  // ---------------------------------------------------------------------------
  // Activation
  // ---------------------------------------------------------------------------

  activate(id: string, mode: PresetMode) {
    if (!this.getById(id)) return;
    const filtered = this.active.filter((a) => a.id !== id);
    this.active = [...filtered, { id, mode }];
    this.saveActive();
  }

  deactivate(id: string) {
    if (!this.isActive(id)) return;
    this.active = this.active.filter((a) => a.id !== id);
    this.saveActive();
  }

  setMode(id: string, mode: PresetMode) {
    if (!this.isActive(id)) return;
    this.active = this.active.map((a) => (a.id === id ? { ...a, mode } : a));
    this.saveActive();
  }

  clearActive() {
    if (this.active.length === 0) return;
    this.active = [];
    this.saveActive();
  }

  // ---------------------------------------------------------------------------
  // Mutations
  // ---------------------------------------------------------------------------

  create(name: string, content = ""): PromptPreset {
    const now = Date.now();
    const preset: PromptPreset = {
      id: genId(),
      name: name.trim() || "Untitled preset",
      content,
      createdAt: now,
      updatedAt: now,
    };
    this.presets = [preset, ...this.presets];
    this.saveSettings();
    return preset;
  }

  update(id: string, patch: Partial<Omit<PromptPreset, "id" | "createdAt">>): void {
    let changed = false;
    this.presets = this.presets.map((p) => {
      if (p.id !== id) return p;
      changed = true;
      return {
        ...p,
        name: typeof patch.name === "string" ? patch.name.trim() || p.name : p.name,
        content: typeof patch.content === "string" ? patch.content : p.content,
        updatedAt: Date.now(),
      };
    });
    if (changed) this.saveSettings();
  }

  duplicate(id: string): PromptPreset | null {
    const src = this.getById(id);
    if (!src) return null;
    const now = Date.now();
    const copy: PromptPreset = {
      ...src,
      id: genId(),
      name: `${src.name} (copy)`,
      createdAt: now,
      updatedAt: now,
    };
    this.presets = [copy, ...this.presets];
    this.saveSettings();
    return copy;
  }

  remove(id: string): void {
    if (!this.getById(id)) return;
    this.presets = this.presets.filter((p) => p.id !== id);
    this.saveSettings();
    if (this.isActive(id)) {
      this.active = this.active.filter((a) => a.id !== id);
      this.saveActive();
    }
  }

  /** Count of comma/newline-separated choices — surfaced in the editor UI. */
  countChoices(id: string): number {
    const p = this.getById(id);
    if (!p) return 0;
    return splitWildcardChoices(p.content).length;
  }

  // ---------------------------------------------------------------------------
  // Export / import
  // ---------------------------------------------------------------------------

  exportJSON(ids?: string[]): string {
    const selected = ids ? this.presets.filter((p) => ids.includes(p.id)) : this.presets;
    const payload: PresetsExport = {
      kind: EXPORT_KIND,
      version: EXPORT_VERSION,
      exportedAt: new Date().toISOString(),
      presets: selected,
    };
    return JSON.stringify(payload, null, 2);
  }

  importJSON(raw: string, mode: "merge" | "replace" = "merge"): { added: number; skipped: number } {
    const parsed = JSON.parse(raw) as Partial<PresetsExport>;
    if (!parsed || typeof parsed !== "object") throw new Error("Invalid JSON payload");
    if (parsed.kind !== EXPORT_KIND) throw new Error("Not a MooshieUI prompt presets export");
    const incoming = Array.isArray(parsed.presets)
      ? (parsed.presets.map(sanitizePreset).filter(Boolean) as PromptPreset[])
      : [];

    if (mode === "replace") {
      this.presets = incoming;
      this.saveSettings();
      const ids = new Set(this.presets.map((p) => p.id));
      this.active = this.active.filter((a) => ids.has(a.id));
      this.saveActive();
      return { added: incoming.length, skipped: 0 };
    }

    const existing = new Set(this.presets.map((p) => p.id));
    let added = 0;
    let skipped = 0;
    const fresh: PromptPreset[] = [];
    for (const p of incoming) {
      if (existing.has(p.id)) {
        skipped++;
        continue;
      }
      fresh.push(p);
      added++;
    }
    if (fresh.length > 0) {
      this.presets = [...fresh, ...this.presets];
      this.saveSettings();
    }
    return { added, skipped };
  }
}

export const promptPresets = new PromptPresetsStore();
