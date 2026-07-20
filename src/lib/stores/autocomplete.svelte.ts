import { ipcStore } from "../utils/ipc.js";
import { triggerSync } from "../utils/syncTrigger.js";
import { locale } from "./locale.svelte.js";
import builtinTags from "../assets/danbooru-tags.json";
import { damerauLevenshtein } from "../utils/promptSpellcheck.js";

export interface TagEntry {
  n: string; // name
  c: number; // category (0=general, 1=artist, 3=copyright, 4=character, 5=meta)
  p: number; // post count
  a?: string[]; // aliases
  /** Truthy when the post count is below the source list's threshold (e.g. <50). UI displays "<50" instead of the numeric `p`. */
  b?: number;
}

interface SearchEntry {
  tag: TagEntry;
  nameLower: string;
  aliasesLower: string[];
}

const STORE_KEY = "autocomplete-settings";
const danbooruTags = builtinTags as TagEntry[];

class AutocompleteStore {
  /** Active tag list used for suggestions */
  // Large tag lists are always replaced wholesale, so raw state avoids deep proxying cost.
  tags = $state.raw<TagEntry[]>(danbooruTags);
  /** Max number of suggestions shown in dropdown */
  maxResults = $state(20);
  /** Source mode: "builtin" | "url" | "file" */
  sourceMode = $state<"builtin" | "url" | "file">("builtin");
  /** URL for remote taglist */
  sourceUrl = $state("");
  /** Display name of uploaded file */
  sourceFileName = $state("");
  /** Whether tag autocomplete is enabled */
  enabled = $state(true);
  /** Whether prompt textareas show the clickable tag/weight overlay */
  clickableOverlayEnabled = $state(true);
  /** Whether the Danbooru tag-aware spell checker (underlines + right-click suggestions) is on */
  spellcheckEnabled = $state(true);
  /** Whether a custom taglist is currently loading */
  loading = $state(false);
  /** Error message if loading failed */
  error = $state<string | null>(null);

  private _storeReady = false;
  private _customTags: TagEntry[] | null = null;
  private _searchEntries: SearchEntry[] = [];
  /** First-character bucket index on names. Lets prefix queries skip 90%+ of the corpus. */
  private _nameFirstChar: Map<string, SearchEntry[]> = new Map();
  /** First-character bucket index on aliases. */
  private _aliasFirstChar: Map<string, SearchEntry[]> = new Map();
  /** Normalized known tag names + aliases for spell-check membership tests. */
  private _knownTagSet: Set<string> = new Set();
  /** Bumps every rebuild so a stale chunked build can abort itself. */
  private _indexVersion = 0;
  private _isAnima = false;
  private _animaTags: TagEntry[] | null = null;
  private _animaTagsPromise: Promise<TagEntry[]> | null = null;
  // Supplemental danbooru artist tags (lazy chunk), merged into the builtin corpus
  // on non-Anima models where danbooru-tags.json carries only ~269 artists.
  private _artistSupplementTags: TagEntry[] | null = null;
  private _artistSupplementPromise: Promise<TagEntry[]> | null = null;
  // Ignore slower builtin swaps if the model/source changed before a lazy import resolved.
  private _builtinSwapToken = 0;

  constructor() {
    this.rebuildSearchIndex(this.tags);
  }

  private normalizeQuery(text: string): string {
    return text.toLowerCase().trim().replace(/\s+/g, "_").replace(/\\/g, "");
  }

  /** Build a single tag → SearchEntry slot and bucket it into the supplied indexes. */
  private indexOne(
    tag: TagEntry,
    nameBuckets: Map<string, SearchEntry[]>,
    aliasBuckets: Map<string, SearchEntry[]>,
    knownSet: Set<string>,
  ): SearchEntry {
    const nameLower = tag.n.toLowerCase();
    const aliasesLower = tag.a ? tag.a.map((alias) => alias.toLowerCase()) : [];
    const entry: SearchEntry = { tag, nameLower, aliasesLower };

    if (nameLower.length > 0) {
      knownSet.add(this.normalizeQuery(nameLower));
      const key = nameLower.charAt(0);
      let bucket = nameBuckets.get(key);
      if (!bucket) {
        bucket = [];
        nameBuckets.set(key, bucket);
      }
      bucket.push(entry);
    }

    if (aliasesLower.length > 0) {
      const seen = new Set<string>();
      for (const alias of aliasesLower) {
        if (alias.length === 0) continue;
        knownSet.add(this.normalizeQuery(alias));
        const key = alias.charAt(0);
        if (seen.has(key)) continue;
        seen.add(key);
        let bucket = aliasBuckets.get(key);
        if (!bucket) {
          bucket = [];
          aliasBuckets.set(key, bucket);
        }
        bucket.push(entry);
      }
    }

    return entry;
  }

  /**
   * Build the search index. For small corpora this is synchronous so existing
   * call sites stay simple. For large corpora (>20k entries — i.e. Anima/custom
   * lists) the work is split across event-loop turns so we never block the main
   * thread with a 100–300 ms freeze when the user is mid-keystroke. Searches
   * keep working against the previous index until the new one swaps in atomically.
   */
  private rebuildSearchIndex(tags: TagEntry[]): void {
    const version = ++this._indexVersion;
    const SYNC_THRESHOLD = 20000;
    const nameBuckets = new Map<string, SearchEntry[]>();
    const aliasBuckets = new Map<string, SearchEntry[]>();
    const knownSet = new Set<string>();

    if (tags.length <= SYNC_THRESHOLD) {
      const entries: SearchEntry[] = new Array(tags.length);
      for (let i = 0; i < tags.length; i++) {
        entries[i] = this.indexOne(tags[i], nameBuckets, aliasBuckets, knownSet);
      }
      this._searchEntries = entries;
      this._nameFirstChar = nameBuckets;
      this._aliasFirstChar = aliasBuckets;
      this._knownTagSet = knownSet;
      return;
    }

    const entries: SearchEntry[] = new Array(tags.length);
    const CHUNK = 8000;
    const buildChunk = (start: number) => {
      if (version !== this._indexVersion) return;
      const end = Math.min(start + CHUNK, tags.length);
      for (let i = start; i < end; i++) {
        entries[i] = this.indexOne(tags[i], nameBuckets, aliasBuckets, knownSet);
      }
      if (end < tags.length) {
        setTimeout(() => buildChunk(end), 0);
        return;
      }
      if (version !== this._indexVersion) return;
      this._searchEntries = entries;
      this._nameFirstChar = nameBuckets;
      this._aliasFirstChar = aliasBuckets;
      this._knownTagSet = knownSet;
    };
    buildChunk(0);
  }

  private setTags(tags: TagEntry[]) {
    this.tags = tags;
    this.rebuildSearchIndex(tags);
  }

  private cancelBuiltinSwap() {
    this._builtinSwapToken += 1;
  }

  private async loadAnimaTags(): Promise<TagEntry[]> {
    if (this._animaTags) {
      return this._animaTags;
    }

    if (!this._animaTagsPromise) {
      this._animaTagsPromise = import("../assets/anima-tags.json")
        .then((module) => {
          this._animaTags = module.default as TagEntry[];
          return this._animaTags;
        })
        .catch((error) => {
          this._animaTagsPromise = null;
          throw error;
        });
    }

    return this._animaTagsPromise;
  }

  private async loadArtistSupplement(): Promise<TagEntry[]> {
    if (this._artistSupplementTags) {
      return this._artistSupplementTags;
    }

    if (!this._artistSupplementPromise) {
      this._artistSupplementPromise = import("../assets/danbooru-artists.json")
        .then((module) => {
          this._artistSupplementTags = module.default as TagEntry[];
          return this._artistSupplementTags;
        })
        .catch((error) => {
          this._artistSupplementPromise = null;
          throw error;
        });
    }

    return this._artistSupplementPromise;
  }

  private async syncBuiltinTags() {
    if (this.sourceMode !== "builtin") return;

    const swapToken = ++this._builtinSwapToken;
    const isAnima = this._isAnima;

    if (!isAnima) {
      try {
        const supplement = await this.loadArtistSupplement();
        if (swapToken !== this._builtinSwapToken || this.sourceMode !== "builtin" || this._isAnima !== isAnima) {
          return;
        }
        const mainNames = new Set(danbooruTags.map((t) => t.n));
        const extra = supplement.filter((t) => !mainNames.has(t.n));
        this.setTags(extra.length > 0 ? [...danbooruTags, ...extra] : danbooruTags);
      } catch (e) {
        if (swapToken !== this._builtinSwapToken || this.sourceMode !== "builtin" || this._isAnima !== isAnima) {
          return;
        }
        // Supplement unavailable (missing asset, or fetch failure in browser mode):
        // fall back to the base danbooru corpus so autocomplete still works.
        console.error("Failed to load danbooru artist supplement:", e);
        this.setTags(danbooruTags);
      }
      return;
    }

    try {
      const tags = await this.loadAnimaTags();
      if (swapToken !== this._builtinSwapToken || this.sourceMode !== "builtin" || this._isAnima !== isAnima) {
        return;
      }
      this.setTags(tags);
    } catch (e) {
      if (swapToken !== this._builtinSwapToken || this.sourceMode !== "builtin" || this._isAnima !== isAnima) {
        return;
      }
      console.error("Failed to load Anima autocomplete tags:", e);
      this.setTags(danbooruTags);
    }
  }

  private insertTopByCount(list: TagEntry[], tag: TagEntry, limit: number) {
    if (limit <= 0) return;
    if (list.length === limit && list[list.length - 1].p >= tag.p) return;

    let insertAt = list.length;
    while (insertAt > 0 && list[insertAt - 1].p < tag.p) {
      insertAt -= 1;
    }

    list.splice(insertAt, 0, tag);
    if (list.length > limit) {
      list.pop();
    }
  }

  search(queryText: string, limit = this.maxResults, categoryFilter: number | null = null): TagEntry[] {
    if (!this.enabled) return [];
    const normalizedQuery = this.normalizeQuery(queryText);
    if (!normalizedQuery) return [];

    const safeLimit = Math.max(1, Math.min(50, limit));
    const firstChar = normalizedQuery.charAt(0);
    // "contains" scans the entire corpus; gate it to ≥3 chars to keep typing responsive.
    const allowContains = normalizedQuery.length >= 3;

    const prefixMatches: TagEntry[] = [];
    const aliasMatches: TagEntry[] = [];
    const containsMatches: TagEntry[] = [];
    const seenNames = new Set<string>();
    let exactMatch: TagEntry | null = null;

    // Prefix match: scan only the first-char bucket (small slice of the corpus).
    const nameBucket = this._nameFirstChar.get(firstChar);
    if (nameBucket) {
      for (let i = 0; i < nameBucket.length; i++) {
        const entry = nameBucket[i];
        if (categoryFilter !== null && entry.tag.c !== categoryFilter) continue;
        if (entry.nameLower.startsWith(normalizedQuery)) {
          if (entry.nameLower === normalizedQuery) {
            exactMatch = entry.tag;
          }
          this.insertTopByCount(prefixMatches, entry.tag, safeLimit);
          seenNames.add(entry.tag.n);
        }
      }
    }

    // Alias prefix match: same first-char bucket on aliases.
    const aliasBucket = this._aliasFirstChar.get(firstChar);
    if (aliasBucket && prefixMatches.length < safeLimit) {
      for (let i = 0; i < aliasBucket.length; i++) {
        const entry = aliasBucket[i];
        if (seenNames.has(entry.tag.n)) continue;
        if (categoryFilter !== null && entry.tag.c !== categoryFilter) continue;
        const aliases = entry.aliasesLower;
        let matched = false;
        for (let a = 0; a < aliases.length; a++) {
          if (aliases[a].startsWith(normalizedQuery)) {
            matched = true;
            break;
          }
        }
        if (matched) {
          this.insertTopByCount(aliasMatches, entry.tag, safeLimit);
          seenNames.add(entry.tag.n);
        }
      }
    }

    // Substring match: only when we still need more results AND the query is long
    // enough to be meaningful. This is the only path that scans the full corpus.
    if (allowContains && prefixMatches.length + aliasMatches.length < safeLimit) {
      const all = this._searchEntries;
      for (let i = 0; i < all.length; i++) {
        const entry = all[i];
        if (seenNames.has(entry.tag.n)) continue;
        if (categoryFilter !== null && entry.tag.c !== categoryFilter) continue;
        if (entry.nameLower.includes(normalizedQuery)) {
          this.insertTopByCount(containsMatches, entry.tag, safeLimit);
          seenNames.add(entry.tag.n);
        }
      }
    }

    const combined = [...prefixMatches, ...aliasMatches, ...containsMatches];
    if (!exactMatch) {
      return combined.slice(0, safeLimit);
    }

    return [exactMatch, ...combined.filter((tag) => tag.n !== exactMatch.n)].slice(0, safeLimit);
  }

  /**
   * True if `name` normalizes to a known tag name or alias in the active corpus.
   * Blank input and an empty/not-yet-built index both return true so nothing is
   * spuriously flagged as misspelled.
   */
  isKnownTag(name: string): boolean {
    if (this._knownTagSet.size === 0) return true;
    const q = this.normalizeQuery(name);
    if (!q) return true;
    return this._knownTagSet.has(q);
  }

  /**
   * Fuzzy "did you mean" matches for an unknown tag. Damerau-Levenshtein over the
   * active corpus, length-windowed and early-exited so it stays cheap even on large
   * custom lists. Only ever called on a right-click, never per keystroke.
   */
  suggestSimilar(name: string, limit = 6): TagEntry[] {
    const q = this.normalizeQuery(name);
    if (!q) return [];
    const maxDist = q.length <= 4 ? 1 : q.length <= 8 ? 2 : 3;

    const scored: { tag: TagEntry; dist: number }[] = [];
    const entries = this._searchEntries;
    for (let i = 0; i < entries.length; i++) {
      const n = entries[i].nameLower;
      if (Math.abs(n.length - q.length) > maxDist) continue;
      const d = damerauLevenshtein(q, n, maxDist);
      if (d <= maxDist) scored.push({ tag: entries[i].tag, dist: d });
    }

    scored.sort((a, b) => a.dist - b.dist || b.tag.p - a.tag.p);
    return scored.slice(0, Math.max(1, limit)).map((s) => s.tag);
  }

  async loadSettings() {
    try {
      const saved = await ipcStore.get<Record<string, any>>(STORE_KEY);
      if (saved) {
        if (saved.enabled === false) this.enabled = false;
        if (saved.clickableOverlayEnabled === false) this.clickableOverlayEnabled = false;
        if (saved.spellcheckEnabled === false) this.spellcheckEnabled = false;
        if (saved.maxResults) this.maxResults = saved.maxResults;
        if (saved.sourceMode) this.sourceMode = saved.sourceMode;
        if (saved.sourceUrl) this.sourceUrl = saved.sourceUrl;
        if (saved.sourceFileName) this.sourceFileName = saved.sourceFileName;
        const customTags = Array.isArray(saved.customTags) ? saved.customTags : null;
        if (this.sourceMode !== "builtin") {
          this.cancelBuiltinSwap();
          if (customTags && customTags.length > 0) {
            this._customTags = customTags;
            this.setTags(customTags);
          } else {
            // Empty/broken custom list — fall back so autocomplete still works
            this._customTags = null;
            this.sourceMode = "builtin";
            this.sourceUrl = "";
            this.sourceFileName = "";
            await this.syncBuiltinTags();
          }
        } else if (customTags) {
          this._customTags = customTags;
        }
      }
      if (this.sourceMode === "builtin") {
        await this.syncBuiltinTags();
      }
    } catch (e) {
      console.error("Failed to load autocomplete settings:", e);
    } finally {
      // See generation.loadSettings(): only ready once the persisted values
      // (and builtin tags) are in memory, so early saves can't clobber them.
      this._storeReady = true;
    }
  }

  async saveSettings() {
    if (!this._storeReady) return;
    try {
      await ipcStore.set(STORE_KEY, {
        enabled: this.enabled,
        clickableOverlayEnabled: this.clickableOverlayEnabled,
        spellcheckEnabled: this.spellcheckEnabled,
        maxResults: this.maxResults,
        sourceMode: this.sourceMode,
        sourceUrl: this.sourceUrl,
        sourceFileName: this.sourceFileName,
        customTags: this._customTags,
      });
      triggerSync();
    } catch (e) {
      console.error("Failed to save autocomplete settings:", e);
    }
  }

  /** Parse CSV taglist (one tag per line: name,category,postCount,aliases) */
  private parseCsv(text: string): TagEntry[] {
    const lines = text.split("\n").filter((l) => l.trim());
    const tags: TagEntry[] = [];
    for (const line of lines) {
      const parts = line.split(",");
      if (parts.length < 1) continue;
      const name = parts[0].trim();
      if (!name) continue;
      const category = parts.length > 1 ? parseInt(parts[1]) || 0 : 0;
      const postCount = parts.length > 2 ? parseInt(parts[2]) || 0 : 0;
      const aliases =
        parts.length > 3
          ? parts
              .slice(3)
              .map((a) => a.trim())
              .filter(Boolean)
          : undefined;
      tags.push({ n: name, c: category, p: postCount, a: aliases });
    }
    return tags;
  }

  /** Parse tag data from text — auto-detects JSON or CSV */
  private parseTagData(text: string): TagEntry[] {
    const trimmed = text.trim();
    if (trimmed.startsWith("[")) {
      // JSON array
      const parsed = JSON.parse(trimmed);
      if (!Array.isArray(parsed)) throw new Error(locale.t("settings.autocomplete.error.json_array"));
      return parsed.map((entry: any) => ({
        n: entry.n || entry.name || "",
        c: entry.c ?? entry.category ?? 0,
        p: entry.p ?? entry.post_count ?? entry.count ?? 0,
        a: entry.a || entry.aliases || undefined,
      }));
    }
    // CSV fallback
    return this.parseCsv(trimmed);
  }

  /** Load tags from a URL */
  async loadFromUrl(url: string) {
    this.cancelBuiltinSwap();
    this.loading = true;
    this.error = null;
    try {
      const resp = await fetch(url);
      if (!resp.ok) throw new Error(`HTTP ${resp.status}: ${resp.statusText}`);
      const text = await resp.text();
      const tags = this.parseTagData(text);
      if (tags.length === 0) throw new Error(locale.t("settings.autocomplete.error.no_tags"));
      this._customTags = tags;
      this.setTags(tags);
      this.sourceMode = "url";
      this.sourceUrl = url;
      this.sourceFileName = "";
      await this.saveSettings();
    } catch (e: any) {
      this.error = e.message || String(e);
    } finally {
      this.loading = false;
    }
  }

  /** Load tags from uploaded file content */
  async loadFromFile(text: string, fileName: string) {
    this.cancelBuiltinSwap();
    this.loading = true;
    this.error = null;
    try {
      const tags = this.parseTagData(text);
      if (tags.length === 0) throw new Error(locale.t("settings.autocomplete.error.no_tags"));
      this._customTags = tags;
      this.setTags(tags);
      this.sourceMode = "file";
      this.sourceFileName = fileName;
      this.sourceUrl = "";
      await this.saveSettings();
    } catch (e: any) {
      this.error = e.message || String(e);
    } finally {
      this.loading = false;
    }
  }

  /** Reset to built-in tags (model-aware: Anima gets its own tag list) */
  async resetToBuiltin() {
    this._customTags = null;
    this.sourceMode = "builtin";
    this.sourceUrl = "";
    this.sourceFileName = "";
    this.error = null;
    await this.syncBuiltinTags();
    await this.saveSettings();
  }

  /** Notify autocomplete that the active model changed. Swaps builtin tags for Anima when appropriate. */
  notifyModelChanged(isAnima: boolean) {
    if (this._isAnima === isAnima) return;
    this._isAnima = isAnima;
    if (this.sourceMode === "builtin") {
      void this.syncBuiltinTags();
    } else {
      this.cancelBuiltinSwap();
    }
  }

  /** Update max results and persist */
  async setMaxResults(n: number) {
    this.maxResults = Math.max(1, Math.min(50, n));
    await this.saveSettings();
  }

  /** Collect autocomplete settings for server-side sync. */
  collectPrefs(): Record<string, unknown> {
    return {
      clickableOverlayEnabled: this.clickableOverlayEnabled,
      spellcheckEnabled: this.spellcheckEnabled,
      maxResults: this.maxResults,
      sourceMode: this.sourceMode,
      sourceUrl: this.sourceUrl,
      sourceFileName: this.sourceFileName,
      customTags: this._customTags,
    };
  }

  /** Apply autocomplete settings from the server. Writes to ipcStore and re-hydrates. */
  async applyServerPrefs(data: Record<string, any>): Promise<void> {
    try {
      await ipcStore.set(STORE_KEY, data);
      await this.loadSettings();
    } catch (e) {
      console.error("autocomplete: applyServerPrefs failed", e);
    }
  }
}

export const autocomplete = new AutocompleteStore();
