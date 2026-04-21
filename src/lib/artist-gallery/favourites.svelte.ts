/**
 * Artist gallery favourites store.
 *
 * Persists a user's favourited artist slugs along with user-created
 * categories (name + colour) and the mapping between them.
 *
 * Storage: localStorage under `mooshieui.artist-gallery.favourites.v1`.
 * Import/export: JSON blob with the same shape as the persisted payload,
 * wrapped in a small envelope for forward-compatibility.
 */

const STORAGE_KEY = "mooshieui.artist-gallery.favourites.v1";
const EXPORT_KIND = "mooshieui.artist-gallery.favourites";
const EXPORT_VERSION = 1;

export interface FavouriteCategory {
  id: string;
  name: string;
  /** CSS colour string (hex). */
  color: string;
}

export interface FavouriteEntry {
  slug: string;
  /** Category id, or null if the favourite is uncategorised. */
  categoryId: string | null;
  /** Epoch millis when the favourite was added. */
  addedAt: number;
}

interface PersistedState {
  version: number;
  favourites: FavouriteEntry[];
  categories: FavouriteCategory[];
}

export interface FavouritesExport {
  kind: typeof EXPORT_KIND;
  version: number;
  exportedAt: string;
  favourites: FavouriteEntry[];
  categories: FavouriteCategory[];
}

function genId(): string {
  // Short, collision-resistant enough for a handful of categories per user.
  return `cat_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

/** Default palette offered when creating a new category. */
export const CATEGORY_COLOR_PALETTE = [
  "#ef4444", // red
  "#f97316", // orange
  "#eab308", // yellow
  "#22c55e", // green
  "#14b8a6", // teal
  "#3b82f6", // blue
  "#6366f1", // indigo
  "#a855f7", // purple
  "#ec4899", // pink
  "#94a3b8", // slate
];

class ArtistFavouritesStore {
  /** slug → FavouriteEntry. */
  favourites = $state<Record<string, FavouriteEntry>>({});
  categories = $state<FavouriteCategory[]>([]);

  constructor() {
    this.loadSettings();
  }

  // ---------------------------------------------------------------------------
  // Persistence
  // ---------------------------------------------------------------------------

  private loadSettings() {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as Partial<PersistedState>;
      this.hydrate(parsed);
    } catch (e) {
      console.error("artist-gallery favourites: load failed", e);
    }
  }

  private hydrate(parsed: Partial<PersistedState>) {
    const categories = Array.isArray(parsed.categories)
      ? parsed.categories.filter(
          (c): c is FavouriteCategory =>
            !!c && typeof c.id === "string" && typeof c.name === "string" && typeof c.color === "string",
        )
      : [];
    const catIds = new Set(categories.map((c) => c.id));

    const favMap: Record<string, FavouriteEntry> = {};
    if (Array.isArray(parsed.favourites)) {
      for (const f of parsed.favourites) {
        if (!f || typeof f.slug !== "string") continue;
        const categoryId =
          typeof f.categoryId === "string" && catIds.has(f.categoryId) ? f.categoryId : null;
        const addedAt = typeof f.addedAt === "number" && f.addedAt > 0 ? f.addedAt : Date.now();
        favMap[f.slug] = { slug: f.slug, categoryId, addedAt };
      }
    }

    this.categories = categories;
    this.favourites = favMap;
  }

  private saveSettings() {
    try {
      const payload: PersistedState = {
        version: EXPORT_VERSION,
        favourites: Object.values(this.favourites),
        categories: this.categories,
      };
      localStorage.setItem(STORAGE_KEY, JSON.stringify(payload));
    } catch (e) {
      console.error("artist-gallery favourites: save failed", e);
    }
  }

  // ---------------------------------------------------------------------------
  // Queries
  // ---------------------------------------------------------------------------

  isFavourite(slug: string): boolean {
    return slug in this.favourites;
  }

  get count(): number {
    return Object.keys(this.favourites).length;
  }

  /** Number of favourites per category id. Uncategorised counted under "". */
  get countsByCategory(): Record<string, number> {
    const counts: Record<string, number> = {};
    for (const f of Object.values(this.favourites)) {
      const key = f.categoryId ?? "";
      counts[key] = (counts[key] ?? 0) + 1;
    }
    return counts;
  }

  categoryOf(slug: string): FavouriteCategory | null {
    const entry = this.favourites[slug];
    if (!entry || !entry.categoryId) return null;
    return this.categories.find((c) => c.id === entry.categoryId) ?? null;
  }

  // ---------------------------------------------------------------------------
  // Mutations — favourites
  // ---------------------------------------------------------------------------

  toggle(slug: string): boolean {
    if (slug in this.favourites) {
      const next = { ...this.favourites };
      delete next[slug];
      this.favourites = next;
      this.saveSettings();
      return false;
    }
    this.favourites = {
      ...this.favourites,
      [slug]: { slug, categoryId: null, addedAt: Date.now() },
    };
    this.saveSettings();
    return true;
  }

  add(slug: string, categoryId: string | null = null): void {
    if (slug in this.favourites) {
      this.setCategory(slug, categoryId);
      return;
    }
    this.favourites = {
      ...this.favourites,
      [slug]: { slug, categoryId, addedAt: Date.now() },
    };
    this.saveSettings();
  }

  remove(slug: string): void {
    if (!(slug in this.favourites)) return;
    const next = { ...this.favourites };
    delete next[slug];
    this.favourites = next;
    this.saveSettings();
  }

  setCategory(slug: string, categoryId: string | null): void {
    const existing = this.favourites[slug];
    if (!existing) return;
    if (categoryId && !this.categories.some((c) => c.id === categoryId)) return;
    this.favourites = {
      ...this.favourites,
      [slug]: { ...existing, categoryId },
    };
    this.saveSettings();
  }

  // ---------------------------------------------------------------------------
  // Mutations — categories
  // ---------------------------------------------------------------------------

  createCategory(name: string, color: string): FavouriteCategory | null {
    const trimmed = name.trim();
    if (!trimmed) return null;
    const cat: FavouriteCategory = { id: genId(), name: trimmed, color };
    this.categories = [...this.categories, cat];
    this.saveSettings();
    return cat;
  }

  updateCategory(id: string, patch: Partial<Omit<FavouriteCategory, "id">>): void {
    let changed = false;
    this.categories = this.categories.map((c) => {
      if (c.id !== id) return c;
      changed = true;
      const nextName = typeof patch.name === "string" ? patch.name.trim() || c.name : c.name;
      const nextColor = typeof patch.color === "string" ? patch.color : c.color;
      return { ...c, name: nextName, color: nextColor };
    });
    if (changed) this.saveSettings();
  }

  deleteCategory(id: string): void {
    if (!this.categories.some((c) => c.id === id)) return;
    this.categories = this.categories.filter((c) => c.id !== id);
    // Unassign any favourites that referenced the removed category.
    const nextFavs: Record<string, FavouriteEntry> = {};
    for (const [slug, entry] of Object.entries(this.favourites)) {
      nextFavs[slug] = entry.categoryId === id ? { ...entry, categoryId: null } : entry;
    }
    this.favourites = nextFavs;
    this.saveSettings();
  }

  // ---------------------------------------------------------------------------
  // Export / import
  // ---------------------------------------------------------------------------

  exportJSON(): string {
    const payload: FavouritesExport = {
      kind: EXPORT_KIND,
      version: EXPORT_VERSION,
      exportedAt: new Date().toISOString(),
      favourites: Object.values(this.favourites),
      categories: this.categories,
    };
    return JSON.stringify(payload, null, 2);
  }

  /**
   * Import favourites from a JSON string.
   * @param mode "replace" wipes existing state; "merge" keeps existing
   *  favourites & categories and adds new ones (categories matched by id).
   */
  importJSON(
    raw: string,
    mode: "replace" | "merge" = "merge",
  ): { added: number; updated: number; categoriesAdded: number } {
    const parsed = JSON.parse(raw) as Partial<FavouritesExport>;
    if (!parsed || typeof parsed !== "object") throw new Error("Invalid JSON payload");
    if (parsed.kind !== EXPORT_KIND) throw new Error("Not a MooshieUI artist favourites export");

    const incomingCats: FavouriteCategory[] = Array.isArray(parsed.categories)
      ? parsed.categories.filter(
          (c): c is FavouriteCategory =>
            !!c && typeof c.id === "string" && typeof c.name === "string" && typeof c.color === "string",
        )
      : [];
    const incomingFavs: FavouriteEntry[] = Array.isArray(parsed.favourites)
      ? parsed.favourites.filter(
          (f): f is FavouriteEntry => !!f && typeof f.slug === "string",
        )
      : [];

    if (mode === "replace") {
      this.categories = incomingCats;
      const map: Record<string, FavouriteEntry> = {};
      const catIds = new Set(incomingCats.map((c) => c.id));
      for (const f of incomingFavs) {
        map[f.slug] = {
          slug: f.slug,
          categoryId:
            typeof f.categoryId === "string" && catIds.has(f.categoryId) ? f.categoryId : null,
          addedAt: typeof f.addedAt === "number" && f.addedAt > 0 ? f.addedAt : Date.now(),
        };
      }
      this.favourites = map;
      this.saveSettings();
      return {
        added: Object.keys(map).length,
        updated: 0,
        categoriesAdded: incomingCats.length,
      };
    }

    // Merge mode
    let categoriesAdded = 0;
    const catsById = new Map(this.categories.map((c) => [c.id, c]));
    for (const c of incomingCats) {
      if (!catsById.has(c.id)) {
        catsById.set(c.id, c);
        categoriesAdded++;
      }
    }
    const mergedCats = Array.from(catsById.values());
    const validCatIds = new Set(mergedCats.map((c) => c.id));

    let added = 0;
    let updated = 0;
    const mergedFavs: Record<string, FavouriteEntry> = { ...this.favourites };
    for (const f of incomingFavs) {
      const categoryId =
        typeof f.categoryId === "string" && validCatIds.has(f.categoryId) ? f.categoryId : null;
      const addedAt = typeof f.addedAt === "number" && f.addedAt > 0 ? f.addedAt : Date.now();
      if (mergedFavs[f.slug]) {
        // Prefer incoming category when it is more specific (non-null).
        if (categoryId && mergedFavs[f.slug].categoryId !== categoryId) {
          mergedFavs[f.slug] = { ...mergedFavs[f.slug], categoryId };
          updated++;
        }
      } else {
        mergedFavs[f.slug] = { slug: f.slug, categoryId, addedAt };
        added++;
      }
    }

    this.categories = mergedCats;
    this.favourites = mergedFavs;
    this.saveSettings();
    return { added, updated, categoriesAdded };
  }
}

export const artistFavourites = new ArtistFavouritesStore();
