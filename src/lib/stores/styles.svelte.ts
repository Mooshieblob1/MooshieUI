/**
 * Artist Styles store.
 *
 * A "Style" is a user-curated bundle of artist tags, each with its own
 * weight, multiplied by an overall style weight. Styles can be activated to
 * contribute tags to the generated prompt WITHOUT appearing in the prompt
 * textbox — they are injected downstream in `generation.toParams()`.
 *
 * Storage: localStorage under `mooshieui.styles.v1`.
 * Export/import: JSON envelope with kind + version for forward-compat.
 */

const STORAGE_KEY = "mooshieui.styles.v1";
const ACTIVE_KEY = "mooshieui.styles.active.v1";
const EXPORT_KIND = "mooshieui.artist-styles";
const EXPORT_VERSION = 1;

/** Max dimension (px) for thumbnails stored with the style. Keeps localStorage / exports small. */
const THUMBNAIL_MAX_DIM = 384;

export interface StyleArtist {
  /** Artist tag as it will appear in the prompt (e.g. "@dairi" or "dairi"). Escaping handled at injection time. */
  tag: string;
  /** Optional slug to cross-link with the artist gallery (for UI enrichment). */
  slug?: string;
  /** Per-artist weight. Multiplied by the parent style's overallWeight at injection. */
  weight: number;
}

export interface ArtistStyle {
  id: string;
  name: string;
  artists: StyleArtist[];
  /** Multiplier applied on top of each artist's individual weight. */
  overallWeight: number;
  /** Base64 data URL. Omitted when the style has no thumbnail. */
  thumbnail: string | null;
  createdAt: number;
  updatedAt: number;
}

interface PersistedState {
  version: number;
  styles: ArtistStyle[];
}

export interface StylesExport {
  kind: typeof EXPORT_KIND;
  version: number;
  exportedAt: string;
  styles: ArtistStyle[];
}

function genId(): string {
  return `sty_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

function clampWeight(w: unknown, fallback = 1.0): number {
  const n = typeof w === "number" ? w : Number(w);
  if (!Number.isFinite(n)) return fallback;
  return Math.max(0, Math.min(3, n));
}

function sanitizeArtist(raw: any): StyleArtist | null {
  if (!raw || typeof raw.tag !== "string") return null;
  const tag = raw.tag.trim();
  if (!tag) return null;
  return {
    tag,
    slug: typeof raw.slug === "string" ? raw.slug : undefined,
    weight: clampWeight(raw.weight, 1.0),
  };
}

function sanitizeStyle(raw: any): ArtistStyle | null {
  if (!raw || typeof raw.id !== "string" || typeof raw.name !== "string") return null;
  const artists = Array.isArray(raw.artists)
    ? (raw.artists.map(sanitizeArtist).filter(Boolean) as StyleArtist[])
    : [];
  const now = Date.now();
  return {
    id: raw.id,
    name: raw.name.trim() || "Untitled style",
    artists,
    overallWeight: clampWeight(raw.overallWeight, 1.0),
    thumbnail: typeof raw.thumbnail === "string" && raw.thumbnail.startsWith("data:") ? raw.thumbnail : null,
    createdAt: typeof raw.createdAt === "number" && raw.createdAt > 0 ? raw.createdAt : now,
    updatedAt: typeof raw.updatedAt === "number" && raw.updatedAt > 0 ? raw.updatedAt : now,
  };
}

/**
 * Escape a tag for the ComfyUI/A1111 weight syntax. Raw parentheses that
 * are part of the artist's name itself must be backslash-escaped so they
 * aren't parsed as attention brackets.
 */
function escapeTagForPrompt(tag: string): string {
  return tag.replace(/([()\[\]])/g, "\\$1");
}

/** Round to 2 decimal places for a stable, compact prompt representation. */
function round2(n: number): number {
  return Math.round(n * 100) / 100;
}

/**
 * Resize an image (blob/dataUrl) down to THUMBNAIL_MAX_DIM and return a JPEG
 * data URL. Keeps exports/localStorage reasonably sized.
 */
export async function resizeImageToDataUrl(source: Blob | string): Promise<string> {
  const blobUrl = typeof source === "string" ? source : URL.createObjectURL(source);
  try {
    const img = await new Promise<HTMLImageElement>((resolve, reject) => {
      const el = new Image();
      el.crossOrigin = "anonymous";
      el.onload = () => resolve(el);
      el.onerror = () => reject(new Error("Failed to load image for thumbnail"));
      el.src = blobUrl;
    });
    const scale = Math.min(1, THUMBNAIL_MAX_DIM / Math.max(img.naturalWidth, img.naturalHeight));
    const w = Math.max(1, Math.round(img.naturalWidth * scale));
    const h = Math.max(1, Math.round(img.naturalHeight * scale));
    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("Canvas 2D context unavailable");
    ctx.drawImage(img, 0, 0, w, h);
    return canvas.toDataURL("image/jpeg", 0.82);
  } finally {
    if (typeof source !== "string") URL.revokeObjectURL(blobUrl);
  }
}

class StylesStore {
  styles = $state<ArtistStyle[]>([]);
  /** IDs of currently-active styles. Their tags are injected into every generation. */
  activeIds = $state<string[]>([]);

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
        if (parsed && Array.isArray(parsed.styles)) {
          this.styles = parsed.styles.map(sanitizeStyle).filter(Boolean) as ArtistStyle[];
        }
      }
    } catch (e) {
      console.error("styles: load failed", e);
    }
    try {
      const raw = localStorage.getItem(ACTIVE_KEY);
      if (raw) {
        const parsed = JSON.parse(raw) as string[];
        if (Array.isArray(parsed)) {
          const ids = new Set(this.styles.map((s) => s.id));
          this.activeIds = parsed.filter((id) => typeof id === "string" && ids.has(id));
        }
      }
    } catch (e) {
      console.error("styles: load active failed", e);
    }
  }

  private saveSettings() {
    try {
      const payload: PersistedState = { version: EXPORT_VERSION, styles: this.styles };
      localStorage.setItem(STORAGE_KEY, JSON.stringify(payload));
    } catch (e) {
      console.error("styles: save failed", e);
    }
  }

  private saveActive() {
    try {
      localStorage.setItem(ACTIVE_KEY, JSON.stringify(this.activeIds));
    } catch (e) {
      console.error("styles: save active failed", e);
    }
  }

  // ---------------------------------------------------------------------------
  // Queries
  // ---------------------------------------------------------------------------

  getById(id: string): ArtistStyle | null {
    return this.styles.find((s) => s.id === id) ?? null;
  }

  isActive(id: string): boolean {
    return this.activeIds.includes(id);
  }

  get activeStyles(): ArtistStyle[] {
    const byId = new Map(this.styles.map((s) => [s.id, s]));
    return this.activeIds.map((id) => byId.get(id)).filter(Boolean) as ArtistStyle[];
  }

  /**
   * Build the prompt fragment (comma-separated weighted tags) contributed by
   * all currently-active styles. Returns empty string when nothing active.
   */
  buildPromptFragment(): string {
    const parts: string[] = [];
    const seen = new Set<string>();
    for (const style of this.activeStyles) {
      for (const a of style.artists) {
        const tag = a.tag.trim();
        if (!tag) continue;
        const key = tag.toLowerCase();
        if (seen.has(key)) continue;
        seen.add(key);
        const w = round2(clampWeight(a.weight) * clampWeight(style.overallWeight));
        if (w <= 0) continue;
        const safeTag = escapeTagForPrompt(tag);
        // Always emit (tag:weight) so downstream code has stable weights, even at 1.0.
        parts.push(`(${safeTag}:${w})`);
      }
    }
    return parts.join(", ");
  }

  // ---------------------------------------------------------------------------
  // Activation
  // ---------------------------------------------------------------------------

  activate(id: string) {
    if (!this.getById(id)) return;
    if (this.activeIds.includes(id)) return;
    this.activeIds = [...this.activeIds, id];
    this.saveActive();
  }

  deactivate(id: string) {
    if (!this.activeIds.includes(id)) return;
    this.activeIds = this.activeIds.filter((x) => x !== id);
    this.saveActive();
  }

  toggleActive(id: string): boolean {
    if (this.activeIds.includes(id)) {
      this.deactivate(id);
      return false;
    }
    this.activate(id);
    return true;
  }

  clearActive() {
    if (this.activeIds.length === 0) return;
    this.activeIds = [];
    this.saveActive();
  }

  // ---------------------------------------------------------------------------
  // Mutations
  // ---------------------------------------------------------------------------

  create(name: string, artists: StyleArtist[] = [], overallWeight = 1.0): ArtistStyle {
    const now = Date.now();
    const style: ArtistStyle = {
      id: genId(),
      name: name.trim() || "Untitled style",
      artists: artists.map((a) => ({ ...a, weight: clampWeight(a.weight, 1.0) })),
      overallWeight: clampWeight(overallWeight, 1.0),
      thumbnail: null,
      createdAt: now,
      updatedAt: now,
    };
    this.styles = [style, ...this.styles];
    this.saveSettings();
    return style;
  }

  update(id: string, patch: Partial<Omit<ArtistStyle, "id" | "createdAt">>): void {
    let changed = false;
    this.styles = this.styles.map((s) => {
      if (s.id !== id) return s;
      changed = true;
      return {
        ...s,
        name: typeof patch.name === "string" ? patch.name.trim() || s.name : s.name,
        artists: Array.isArray(patch.artists)
          ? patch.artists
              .map((a) => sanitizeArtist(a))
              .filter(Boolean) as StyleArtist[]
          : s.artists,
        overallWeight:
          typeof patch.overallWeight === "number"
            ? clampWeight(patch.overallWeight, s.overallWeight)
            : s.overallWeight,
        thumbnail:
          patch.thumbnail === null
            ? null
            : typeof patch.thumbnail === "string" && patch.thumbnail.startsWith("data:")
              ? patch.thumbnail
              : s.thumbnail,
        updatedAt: Date.now(),
      };
    });
    if (changed) this.saveSettings();
  }

  duplicate(id: string): ArtistStyle | null {
    const src = this.getById(id);
    if (!src) return null;
    const now = Date.now();
    const copy: ArtistStyle = {
      ...src,
      id: genId(),
      name: `${src.name} (copy)`,
      artists: src.artists.map((a) => ({ ...a })),
      createdAt: now,
      updatedAt: now,
    };
    this.styles = [copy, ...this.styles];
    this.saveSettings();
    return copy;
  }

  remove(id: string): void {
    if (!this.getById(id)) return;
    this.styles = this.styles.filter((s) => s.id !== id);
    this.saveSettings();
    if (this.activeIds.includes(id)) {
      this.activeIds = this.activeIds.filter((x) => x !== id);
      this.saveActive();
    }
  }

  // Convenience mutations used by the editor UI.
  addArtist(id: string, artist: StyleArtist): void {
    const style = this.getById(id);
    if (!style) return;
    const tagLower = artist.tag.trim().toLowerCase();
    if (!tagLower) return;
    if (style.artists.some((a) => a.tag.trim().toLowerCase() === tagLower)) return;
    this.update(id, { artists: [...style.artists, { ...artist, weight: clampWeight(artist.weight, 1.0) }] });
  }

  updateArtist(styleId: string, index: number, patch: Partial<StyleArtist>): void {
    const style = this.getById(styleId);
    if (!style) return;
    if (index < 0 || index >= style.artists.length) return;
    const next = style.artists.map((a, i) => (i === index ? { ...a, ...patch } : a));
    this.update(styleId, { artists: next });
  }

  removeArtist(styleId: string, index: number): void {
    const style = this.getById(styleId);
    if (!style) return;
    this.update(styleId, { artists: style.artists.filter((_, i) => i !== index) });
  }

  setThumbnail(id: string, dataUrl: string | null): void {
    this.update(id, { thumbnail: dataUrl });
  }

  // ---------------------------------------------------------------------------
  // Export / import
  // ---------------------------------------------------------------------------

  exportJSON(styleIds?: string[]): string {
    const selected = styleIds
      ? this.styles.filter((s) => styleIds.includes(s.id))
      : this.styles;
    const payload: StylesExport = {
      kind: EXPORT_KIND,
      version: EXPORT_VERSION,
      exportedAt: new Date().toISOString(),
      styles: selected,
    };
    return JSON.stringify(payload, null, 2);
  }

  /**
   * Import styles from a JSON string.
   * - "merge": new IDs are appended; existing IDs are skipped (use "replace-id" to overwrite).
   * - "replace": wipes existing styles entirely and installs the imported set.
   */
  importJSON(raw: string, mode: "merge" | "replace" = "merge"): { added: number; skipped: number } {
    const parsed = JSON.parse(raw) as Partial<StylesExport>;
    if (!parsed || typeof parsed !== "object") throw new Error("Invalid JSON payload");
    if (parsed.kind !== EXPORT_KIND) throw new Error("Not a MooshieUI styles export");
    const incoming = Array.isArray(parsed.styles)
      ? (parsed.styles.map(sanitizeStyle).filter(Boolean) as ArtistStyle[])
      : [];

    if (mode === "replace") {
      this.styles = incoming;
      this.saveSettings();
      // Purge active IDs that no longer exist.
      const ids = new Set(this.styles.map((s) => s.id));
      this.activeIds = this.activeIds.filter((id) => ids.has(id));
      this.saveActive();
      return { added: incoming.length, skipped: 0 };
    }

    const existing = new Set(this.styles.map((s) => s.id));
    let added = 0;
    let skipped = 0;
    const fresh: ArtistStyle[] = [];
    for (const s of incoming) {
      if (existing.has(s.id)) {
        skipped++;
        continue;
      }
      fresh.push(s);
      added++;
    }
    if (fresh.length > 0) {
      this.styles = [...fresh, ...this.styles];
      this.saveSettings();
    }
    return { added, skipped };
  }
}

export const styles = new StylesStore();
