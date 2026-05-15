/**
 * Prompt Presets store.
 *
 * A "Preset" is a named chunk of prompt text (any tags, not just artists)
 * that the user can activate to inject into generations. On activation the
 * user picks a mode:
 *   - "prepend"  — injected at the start of the positive prompt
 *   - "append"   — injected at the end
 *   - "wildcard" — exactly ONE of the newline-separated tag groups inside
 *                  `content` is chosen at random for each generation (re-rolled
 *                  every time `resolve()` is called). Commas within a line
 *                  keep tags grouped together, so `1girl, solo` on one line
 *                  is picked as the whole block rather than "1girl" or "solo"
 *                  in isolation.
 *   - "wildcard_ordered" — picks the next newline-separated tag group each
 *                  generation, wrapping back to the first line after the last.
 *                  The generate button can use this list length to queue a
 *                  full ordered run in one click.
 *
 * Like Artist Styles, presets live outside the prompt textbox — users see
 * badges, not tags — and survive reloads via localStorage.
 */

const STORAGE_KEY = "mooshieui.promptPresets.v1";
const ACTIVE_KEY = "mooshieui.promptPresets.active.v1";
const EXPORT_KIND = "mooshieui.prompt-presets";
const EXPORT_VERSION = 1;

import { triggerSync } from "../utils/syncTrigger.js";

export type PresetMode = "prepend" | "append" | "wildcard" | "wildcard_ordered";

export interface PromptPreset {
  id: string;
  name: string;
  /** Free-form prompt text. For wildcard mode, newlines split choices; commas keep tags grouped within a choice. */
  content: string;
  createdAt: number;
  updatedAt: number;
}

export interface ActivePreset {
  id: string;
  mode: PresetMode;
  /** Next choice index for ordered wildcard mode. */
  wildcardIndex?: number;
}

export interface OrderedWildcardRun {
  presetId: string;
  presetName: string;
  count: number;
  nextIndex: number;
}

export interface ResolvePromptPresetOptions {
  fixedChoices?: ReadonlyMap<string, string>;
  skipIds?: ReadonlySet<string>;
  advanceFixedOrdered?: boolean;
}

interface PersistedState {
  version: number;
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

function isPresetMode(mode: unknown): mode is PresetMode {
  return mode === "prepend" || mode === "append" || mode === "wildcard" || mode === "wildcard_ordered";
}

function sanitizeStoredIndex(index: unknown): number {
  if (typeof index !== "number" || !Number.isFinite(index) || index < 0) return 0;
  return Math.trunc(index);
}

function normalizeChoiceIndex(index: unknown, length: number): number {
  if (length <= 0) return 0;
  const stored = sanitizeStoredIndex(index);
  return stored % length;
}

function activePresetWithMode(id: string, mode: PresetMode, wildcardIndex = 0): ActivePreset {
  if (mode === "wildcard_ordered") {
    return { id, mode, wildcardIndex: sanitizeStoredIndex(wildcardIndex) };
  }
  return { id, mode };
}

function sanitizeActivePreset(raw: any, validIds: Set<string>): ActivePreset | null {
  if (!raw || typeof raw.id !== "string" || !validIds.has(raw.id) || !isPresetMode(raw.mode)) return null;
  return activePresetWithMode(raw.id, raw.mode, raw.wildcardIndex);
}

function dedupeOrderedActive(active: ActivePreset[]): ActivePreset[] {
  let hasOrdered = false;
  const out: ActivePreset[] = [];
  for (const preset of active) {
    if (preset.mode === "wildcard_ordered") {
      if (hasOrdered) continue;
      hasOrdered = true;
    }
    out.push(preset);
  }
  return out;
}

/**
 * Split wildcard content into individual choices. Newlines delimit choices;
 * commas within a line are preserved so multi-tag groups (e.g. `1girl, solo`)
 * are treated as a single wildcard block rather than as separate candidates.
 */
function splitWildcardChoices(content: string): string[] {
  return content
    .split(/\r?\n/)
    .map((s) => s.trim().replace(/^,+|,+$/g, "").trim())
    .filter((s) => s.length > 0);
}

/**
 * Derive a URL/token-safe slug from a preset display name. Lowercased, with
 * runs of non-alphanumeric chars collapsed to a single underscore. Used as
 * the inline `@preset:<slug>` token form so users can drop a preset at any
 * point in the prompt without worrying about case or punctuation.
 */
export function presetSlug(name: string): string {
  return (
    name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "_")
      .replace(/^_+|_+$/g, "") || "preset"
  );
}

/**
 * Reserved keywords that must NOT be treated as artist tags when they appear
 * after `@`. Anything matching `@<keyword>:` is a MooshieUI directive, not
 * an Anima artist reference. Kept tiny on purpose — easy to scan, easy to
 * extend later (e.g. `style:`, `lora:`).
 */
export const RESERVED_AT_KEYWORDS: ReadonlySet<string> = new Set(["preset"]);

/**
 * Regex that matches inline preset directives: `@preset:<slug>`. The slug
 * captures `[a-z0-9_]` only (matches `presetSlug()` output). Case-insensitive
 * on the keyword for forgiveness; slug must already be lowercased.
 */
export const INLINE_PRESET_REGEX = /@preset:([a-z0-9_]+)/gi;

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
          this.active = dedupeOrderedActive(parsed.map((a) => sanitizeActivePreset(a, ids)).filter(Boolean) as ActivePreset[]);
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
      triggerSync();
    } catch (e) {
      console.error("prompt-presets: save failed", e);
    }
  }

  private saveActive() {
    try {
      localStorage.setItem(ACTIVE_KEY, JSON.stringify(this.active));
      triggerSync();
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

  get activeEntries(): Array<{ preset: PromptPreset; mode: PresetMode; wildcardIndex?: number }> {
    const byId = new Map(this.presets.map((p) => [p.id, p]));
    const out: Array<{ preset: PromptPreset; mode: PresetMode; wildcardIndex?: number }> = [];
    for (const a of this.active) {
      const preset = byId.get(a.id);
      if (preset) out.push({ preset, mode: a.mode, wildcardIndex: a.wildcardIndex });
    }
    return out;
  }

  get orderedWildcardRun(): OrderedWildcardRun | null {
    const byId = new Map(this.presets.map((p) => [p.id, p]));
    for (const activePreset of this.active) {
      if (activePreset.mode !== "wildcard_ordered") continue;
      const preset = byId.get(activePreset.id);
      if (!preset) continue;
      const count = splitWildcardChoices(preset.content).length;
      if (count === 0) continue;
      return {
        presetId: preset.id,
        presetName: preset.name,
        count,
        nextIndex: normalizeChoiceIndex(activePreset.wildcardIndex, count),
      };
    }
    return null;
  }

  get orderedWildcardRunCount(): number {
    return this.orderedWildcardRun?.count ?? 0;
  }

  wildcardChoices(id: string): string[] {
    const preset = this.getById(id);
    return preset ? splitWildcardChoices(preset.content) : [];
  }

  setOrderedWildcardIndex(id: string, index: number): void {
    const choices = this.wildcardChoices(id);
    const nextIndex = normalizeChoiceIndex(index, choices.length);
    let changed = false;
    this.active = this.active.map((activePreset) => {
      if (activePreset.id !== id || activePreset.mode !== "wildcard_ordered") return activePreset;
      if (activePreset.wildcardIndex === nextIndex) return activePreset;
      changed = true;
      return { ...activePreset, wildcardIndex: nextIndex };
    });
    if (changed) this.saveActive();
  }

  inlinePresetIds(text: string): Set<string> {
    const ids = new Set<string>();
    if (!text || !text.includes("@preset:")) return ids;
    const lookup = this.bySlug;
    for (const match of text.matchAll(INLINE_PRESET_REGEX)) {
      const preset = lookup.get(match[1].toLowerCase());
      if (preset) ids.add(preset.id);
    }
    return ids;
  }

  /**
   * Resolve active presets into concrete text fragments. Wildcard modes pick
   * one choice from the preset content. Called once per generation.
   */
  resolve(options: ResolvePromptPresetOptions = {}): { prepend: string; append: string } {
    const prepends: string[] = [];
    const appends: string[] = [];
    const byId = new Map(this.presets.map((p) => [p.id, p]));
    let nextActive: ActivePreset[] | null = null;

    for (const [activeIndex, activePreset] of this.active.entries()) {
      if (options.skipIds?.has(activePreset.id)) continue;
      const preset = byId.get(activePreset.id);
      if (!preset) continue;
      const mode = activePreset.mode;

      if (mode === "wildcard" || mode === "wildcard_ordered") {
        const fixedChoice = options.fixedChoices?.get(preset.id)?.trim();
        const choices = fixedChoice ? [] : splitWildcardChoices(preset.content);
        if (!fixedChoice && choices.length === 0) continue;
        let picked: string;
        if (mode === "wildcard_ordered") {
          if (fixedChoice) {
            picked = fixedChoice;
            if (options.advanceFixedOrdered !== false) {
              const storedChoices = splitWildcardChoices(preset.content);
              const choiceIndex = storedChoices.findIndex((choice) => choice === fixedChoice);
              if (choiceIndex >= 0) {
                const nextIndex = (choiceIndex + 1) % storedChoices.length;
                if (activePreset.wildcardIndex !== nextIndex) {
                  nextActive ??= this.active.slice();
                  nextActive[activeIndex] = { ...activePreset, wildcardIndex: nextIndex };
                }
              }
            }
          } else {
            const choiceIndex = normalizeChoiceIndex(activePreset.wildcardIndex, choices.length);
            picked = choices[choiceIndex];
            const nextIndex = (choiceIndex + 1) % choices.length;
            if (activePreset.wildcardIndex !== nextIndex) {
              nextActive ??= this.active.slice();
              nextActive[activeIndex] = { ...activePreset, wildcardIndex: nextIndex };
            }
          }
        } else {
          picked = fixedChoice || choices[Math.floor(Math.random() * choices.length)];
        }
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
    if (nextActive) {
      this.active = nextActive;
      this.saveActive();
    }

    return {
      prepend: prepends.join(", "),
      append: appends.join(", "),
    };
  }

  /**
   * Map of slug → preset for inline `@preset:<slug>` lookups. Recomputed
   * each access so it stays in sync with renames; the list is small enough
   * that caching isn't worth the bookkeeping.
   */
  get bySlug(): Map<string, PromptPreset> {
    const map = new Map<string, PromptPreset>();
    for (const p of this.presets) {
      const slug = presetSlug(p.name);
      // First-write-wins on collisions — the renaming logic in importTxt and
      // the natural uniqueness of display names means dupes are rare.
      if (!map.has(slug)) map.set(slug, p);
    }
    return map;
  }

  /** Convenience getter: just the slug strings, for highlight rendering. */
  get slugs(): Set<string> {
    return new Set(this.bySlug.keys());
  }

  /**
   * Resolve `@preset:<slug>` directives inline within a prompt string.
   * - Single-line preset content is inserted verbatim (trimmed).
   * - Multi-line preset content picks one random line per occurrence
   *   (independent rolls — `@preset:foo, @preset:foo` rolls twice).
   * - Empty presets resolve to an empty string; adjacent commas/whitespace
   *   are tidied so the prompt doesn't end up with `, ,` artefacts.
   * - Unknown slugs are left untouched (so typos are debuggable).
   */
  resolveInline(text: string, options: Pick<ResolvePromptPresetOptions, "fixedChoices"> = {}): string {
    if (!text || !text.includes("@preset:")) return text;
    const lookup = this.bySlug;
    let resolved = text.replace(INLINE_PRESET_REGEX, (full, slug: string) => {
      const preset = lookup.get(slug.toLowerCase());
      if (!preset) return full;
      const fixedChoice = options.fixedChoices?.get(preset.id)?.trim();
      if (fixedChoice) return fixedChoice;
      const choices = splitWildcardChoices(preset.content);
      if (choices.length === 0) {
        const verbatim = preset.content.trim();
        return verbatim;
      }
      if (choices.length === 1) return choices[0];
      return choices[Math.floor(Math.random() * choices.length)];
    });
    // Tidy up commas/whitespace left behind when a preset resolved to "".
    resolved = resolved
      .replace(/,\s*,/g, ",")
      .replace(/\(\s*,/g, "(")
      .replace(/,\s*\)/g, ")")
      .replace(/^\s*,\s*/, "")
      .replace(/\s*,\s*$/, "")
      .replace(/[ \t]{2,}/g, " ")
      .trim();
    return resolved;
  }

  // ---------------------------------------------------------------------------
  // Activation
  // ---------------------------------------------------------------------------

  activate(id: string, mode: PresetMode) {
    if (!this.getById(id)) return;
    let filtered = this.active.filter((a) => a.id !== id);
    if (mode === "wildcard_ordered") {
      filtered = filtered.filter((a) => a.mode !== "wildcard_ordered");
    }
    this.active = dedupeOrderedActive([...filtered, activePresetWithMode(id, mode)]);
    this.saveActive();
  }

  deactivate(id: string) {
    if (!this.isActive(id)) return;
    this.active = this.active.filter((a) => a.id !== id);
    this.saveActive();
  }

  setMode(id: string, mode: PresetMode) {
    if (!this.isActive(id)) return;
    let nextActive = this.active.map((a) => (a.id === id ? activePresetWithMode(a.id, mode, a.wildcardIndex) : a));
    if (mode === "wildcard_ordered") {
      nextActive = nextActive.filter((a) => a.id === id || a.mode !== "wildcard_ordered");
    }
    this.active = dedupeOrderedActive(nextActive);
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
  // Export / import (.txt)
  // ---------------------------------------------------------------------------

  /** Render a preset as a .txt file (filename + contents). */
  exportTxt(id: string): { filename: string; content: string } | null {
    const p = this.getById(id);
    if (!p) return null;
    return {
      filename: `${sanitizeFilename(p.name)}.txt`,
      content: p.content,
    };
  }

  /**
   * Import a preset from a plain .txt file. Filename (minus extension) becomes
   * the preset name; file contents become the `content` field verbatim.
   *
   * If a preset with the same name already exists, a numeric suffix is added
   * (e.g. `cats (2)`). Returns the created preset and whether it was renamed.
   */
  importTxt(filename: string, content: string): { preset: PromptPreset; renamed: boolean } {
    const baseName = stripExtension(filename).trim() || "Imported preset";
    const { name, renamed } = this.uniqueName(baseName);
    const preset = this.create(name);
    this.update(preset.id, { content });
    return { preset: this.getById(preset.id) ?? preset, renamed };
  }

  private uniqueName(base: string): { name: string; renamed: boolean } {
    const existing = new Set(this.presets.map((p) => p.name));
    if (!existing.has(base)) return { name: base, renamed: false };
    for (let i = 2; i < 1000; i++) {
      const candidate = `${base} (${i})`;
      if (!existing.has(candidate)) return { name: candidate, renamed: true };
    }
    return { name: `${base} (${Date.now()})`, renamed: true };
  }

  /** Collect preset state for server-side sync. */
  collectPrefs(): unknown {
    return {
      presets: this.presets,
      active: this.active,
    };
  }

  /** Apply prompt presets fetched from the server. Replaces local storage and re-hydrates. */
  applyServerPrefs(data: any): void {
    try {
      if (Array.isArray(data?.presets)) {
        localStorage.setItem(STORAGE_KEY, JSON.stringify({ version: EXPORT_VERSION, presets: data.presets }));
      }
      if (Array.isArray(data?.active)) {
        localStorage.setItem(ACTIVE_KEY, JSON.stringify(data.active));
      }
      // Re-hydrate from the newly-written localStorage
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw) {
        const parsed = JSON.parse(raw) as Partial<PersistedState>;
        if (Array.isArray(parsed?.presets)) {
          this.presets = parsed.presets.map(sanitizePreset).filter(Boolean) as PromptPreset[];
        }
      }
      const rawActive = localStorage.getItem(ACTIVE_KEY);
      if (rawActive) {
        const parsed = JSON.parse(rawActive) as ActivePreset[];
        if (Array.isArray(parsed)) {
          const ids = new Set(this.presets.map((p) => p.id));
          this.active = dedupeOrderedActive(parsed.map((a) => sanitizeActivePreset(a, ids)).filter(Boolean) as ActivePreset[]);
        }
      }
    } catch (e) {
      console.error("prompt-presets: applyServerPrefs failed", e);
    }
  }
}

function stripExtension(filename: string): string {
  return filename.replace(/\.[^./\\]+$/, "");
}

function sanitizeFilename(name: string): string {
  // Replace reserved chars across platforms with underscores; collapse runs.
  return name.replace(/[\\/:*?"<>|\x00-\x1f]+/g, "_").replace(/_+/g, "_").trim() || "preset";
}

export const promptPresets = new PromptPresetsStore();
