import {
  sanitizeEnhanceRecipes,
  serializeRecipesExport,
  slugifyRecipe,
  type EnhanceRecipe,
  type EnhanceRecipeDraft,
} from "../utils/enhanceRecipes.js";
import { userScopedKey } from "../utils/ipc.js";
import { triggerSync } from "../utils/syncTrigger.js";

const STORAGE_KEY = "mooshieui.enhanceRecipes.v1";
const ACTIVE_KEY = "mooshieui.enhanceRecipes.active.v1";
const STORE_VERSION = 1;

interface PersistedRecipes {
  version: number;
  recipes: EnhanceRecipe[];
}

function genId(): string {
  return `rcp_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

function loadRecipes(): EnhanceRecipe[] {
  try {
    const raw = localStorage.getItem(userScopedKey(STORAGE_KEY));
    if (!raw) return [];
    const parsed = JSON.parse(raw) as Partial<PersistedRecipes> | EnhanceRecipe[];
    const list = Array.isArray(parsed) ? parsed : parsed?.recipes;
    return sanitizeEnhanceRecipes(list);
  } catch {
    return [];
  }
}

function loadActiveIds(recipes: EnhanceRecipe[]): string[] {
  try {
    const raw = localStorage.getItem(userScopedKey(ACTIVE_KEY));
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    const valid = new Set(recipes.map((r) => r.id));
    return [...new Set(parsed.filter((id): id is string => typeof id === "string" && valid.has(id)))];
  } catch {
    return [];
  }
}

class EnhanceRecipesStore {
  recipes = $state<EnhanceRecipe[]>([]);
  activeIds = $state<string[]>([]);
  /** True while silent LLM adaptation is running on Generate. */
  isApplyingAtGenerate = $state(false);
  private _enhancedFingerprints = new Set<string>();

  constructor() {
    const initial = loadRecipes();
    this.recipes = initial;
    this.activeIds = loadActiveIds(initial);
  }

  get activeRecipes(): EnhanceRecipe[] {
    const active = new Set(this.activeIds);
    return this.recipes.filter((r) => active.has(r.id));
  }

  recordEnhancedFingerprint(fp: string): void {
    if (!fp) return;
    if (this._enhancedFingerprints.has(fp)) this._enhancedFingerprints.delete(fp);
    this._enhancedFingerprints.add(fp);
    if (this._enhancedFingerprints.size > 32) {
      const oldest = this._enhancedFingerprints.values().next().value;
      if (oldest !== undefined) this._enhancedFingerprints.delete(oldest);
    }
  }

  isEnhancedFingerprint(fp: string): boolean {
    return this._enhancedFingerprints.has(fp);
  }

  private saveLocal(): void {
    try {
      const payload: PersistedRecipes = { version: STORE_VERSION, recipes: this.recipes };
      localStorage.setItem(userScopedKey(STORAGE_KEY), JSON.stringify(payload));
    } catch (e) {
      console.warn("[enhanceRecipes] could not save recipes:", e);
    }
  }

  private saveActiveLocal(): void {
    try {
      localStorage.setItem(userScopedKey(ACTIVE_KEY), JSON.stringify(this.activeIds));
    } catch {
      /* storage unavailable */
    }
  }

  private commit(): void {
    this.saveLocal();
    triggerSync();
  }

  private uniqueSlug(base: string, excludeId?: string): string {
    const stem = slugifyRecipe(base) || "recipe";
    const taken = new Set(this.recipes.filter((r) => r.id !== excludeId).map((r) => r.slug));
    if (!taken.has(stem)) return stem;
    for (let i = 2; i < 1000; i++) {
      const candidate = `${stem.slice(0, 35)}-${i}`;
      if (!taken.has(candidate)) return candidate;
    }
    return `${stem.slice(0, 30)}-${Date.now().toString(36)}`;
  }

  getById(id: string): EnhanceRecipe | undefined {
    return this.recipes.find((r) => r.id === id);
  }

  isActive(id: string): boolean {
    return this.activeIds.includes(id);
  }

  toggleActive(id: string): void {
    if (!this.getById(id)) return;
    if (this.isActive(id)) {
      this.activeIds = this.activeIds.filter((x) => x !== id);
    } else {
      this.activeIds = [...this.activeIds, id];
    }
    this.saveActiveLocal();
  }

  setActive(id: string, on: boolean): void {
    if (!this.getById(id)) return;
    if (on && !this.isActive(id)) {
      this.activeIds = [...this.activeIds, id];
      this.saveActiveLocal();
    } else if (!on && this.isActive(id)) {
      this.activeIds = this.activeIds.filter((x) => x !== id);
      this.saveActiveLocal();
    }
  }

  clearActive(): void {
    if (this.activeIds.length === 0) return;
    this.activeIds = [];
    this.saveActiveLocal();
  }

  add(draft: EnhanceRecipeDraft): EnhanceRecipe {
    const now = Date.now();
    const name = draft.name.trim() || "Untitled recipe";
    const slug = this.uniqueSlug(draft.slug || name);
    const recipe: EnhanceRecipe = {
      id: genId(),
      name,
      slug,
      intent: draft.intent.trim(),
      include: draft.include.trim(),
      avoid: draft.avoid.trim(),
      neverUc: draft.neverUc.trim(),
      context: draft.context.trim(),
      scope: draft.scope,
      applyMode: draft.applyMode ?? "ai",
      triggers: [...new Set(draft.triggers.map((t) => t.trim().toLowerCase()).filter(Boolean))],
      createdAt: now,
      updatedAt: now,
    };
    this.recipes = [...this.recipes, recipe];
    this.commit();
    return recipe;
  }

  update(id: string, patch: Partial<EnhanceRecipeDraft>): EnhanceRecipe | null {
    const idx = this.recipes.findIndex((r) => r.id === id);
    if (idx === -1) return null;
    const prev = this.recipes[idx];
    const name = patch.name !== undefined ? patch.name.trim() || prev.name : prev.name;
    const slug =
      patch.slug !== undefined
        ? this.uniqueSlug(patch.slug || name, id)
        : prev.slug;
    const next: EnhanceRecipe = {
      ...prev,
      name,
      slug,
      intent: patch.intent !== undefined ? patch.intent.trim() : prev.intent,
      include: patch.include !== undefined ? patch.include.trim() : prev.include,
      avoid: patch.avoid !== undefined ? patch.avoid.trim() : prev.avoid,
      neverUc: patch.neverUc !== undefined ? patch.neverUc.trim() : prev.neverUc,
      context: patch.context !== undefined ? patch.context.trim() : prev.context,
      scope: patch.scope ?? prev.scope,
      applyMode: patch.applyMode ?? prev.applyMode,
      triggers:
        patch.triggers !== undefined
          ? [...new Set(patch.triggers.map((t) => t.trim().toLowerCase()).filter(Boolean))]
          : prev.triggers,
      updatedAt: Date.now(),
    };
    this.recipes = this.recipes.map((r, i) => (i === idx ? next : r));
    this.commit();
    return next;
  }

  remove(id: string): void {
    if (!this.recipes.some((r) => r.id === id)) return;
    this.recipes = this.recipes.filter((r) => r.id !== id);
    if (this.activeIds.includes(id)) {
      this.activeIds = this.activeIds.filter((x) => x !== id);
      this.saveActiveLocal();
    }
    this.commit();
  }

  duplicate(id: string): EnhanceRecipe | null {
    const src = this.getById(id);
    if (!src) return null;
    return this.add({
      name: `${src.name} (copy)`,
      slug: `${src.slug}-copy`,
      intent: src.intent,
      include: src.include,
      avoid: src.avoid,
      neverUc: src.neverUc,
      context: src.context,
      scope: src.scope,
      applyMode: src.applyMode,
      triggers: [...src.triggers],
    });
  }

  importRecipes(incoming: EnhanceRecipe[]): number {
    if (incoming.length === 0) return 0;
    const next = [...this.recipes];
    let count = 0;
    for (const item of incoming) {
      const byId = next.findIndex((r) => r.id === item.id);
      const bySlug = byId === -1 ? next.findIndex((r) => r.slug === item.slug) : -1;
      const targetIdx = byId !== -1 ? byId : bySlug;
      if (targetIdx !== -1) {
        next[targetIdx] = {
          ...item,
          id: next[targetIdx].id,
          createdAt: next[targetIdx].createdAt,
          updatedAt: Date.now(),
        };
      } else {
        const slugTaken = next.some((r) => r.slug === item.slug);
        next.push({
          ...item,
          slug: slugTaken ? this.uniqueSlug(item.slug) : item.slug,
        });
      }
      count++;
    }
    this.recipes = next;
    this.commit();
    return count;
  }

  exportJson(): string {
    return serializeRecipesExport(this.recipes);
  }

  collectPrefs(): unknown {
    return {
      version: STORE_VERSION,
      recipes: this.recipes,
    };
  }

  applyServerPrefs(data: unknown): void {
    try {
      const list = Array.isArray(data)
        ? data
        : data && typeof data === "object" && Array.isArray((data as Record<string, unknown>).recipes)
          ? (data as Record<string, unknown>).recipes
          : null;
      if (!list) return;
      const sanitized = sanitizeEnhanceRecipes(list);
      this.recipes = sanitized;
      const valid = new Set(sanitized.map((r) => r.id));
      const pruned = this.activeIds.filter((id) => valid.has(id));
      if (pruned.length !== this.activeIds.length) {
        this.activeIds = pruned;
        this.saveActiveLocal();
      }
      this.saveLocal();
    } catch (e) {
      console.warn("[enhanceRecipes] applyServerPrefs failed:", e);
    }
  }
}

export const enhanceRecipes = new EnhanceRecipesStore();
