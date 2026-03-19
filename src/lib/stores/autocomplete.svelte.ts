import { load } from "@tauri-apps/plugin-store";
import builtinTags from "../assets/danbooru-tags.json";

export interface TagEntry {
  n: string; // name
  c: number; // category (0=general, 1=artist, 3=copyright, 4=character, 5=meta)
  p: number; // post count
  a?: string[]; // aliases
}

interface SearchEntry {
  tag: TagEntry;
  nameLower: string;
  aliasesLower: string[];
}

const STORE_KEY = "autocomplete-settings";

class AutocompleteStore {
  /** Active tag list used for suggestions */
  tags = $state<TagEntry[]>(builtinTags as TagEntry[]);
  /** Max number of suggestions shown in dropdown */
  maxResults = $state(10);
  /** Source mode: "builtin" | "url" | "file" */
  sourceMode = $state<"builtin" | "url" | "file">("builtin");
  /** URL for remote taglist */
  sourceUrl = $state("");
  /** Display name of uploaded file */
  sourceFileName = $state("");
  /** Whether a custom taglist is currently loading */
  loading = $state(false);
  /** Error message if loading failed */
  error = $state<string | null>(null);

  private _store: Awaited<ReturnType<typeof load>> | null = null;
  private _customTags: TagEntry[] | null = null;
  private _searchEntries: SearchEntry[] = [];

  constructor() {
    this.rebuildSearchIndex(this.tags);
  }

  private normalizeQuery(text: string): string {
    return text.toLowerCase().trim().replace(/\s+/g, "_");
  }

  private rebuildSearchIndex(tags: TagEntry[]) {
    this._searchEntries = tags.map((tag) => ({
      tag,
      nameLower: tag.n.toLowerCase(),
      aliasesLower: tag.a?.map((alias) => alias.toLowerCase()) ?? [],
    }));
  }

  private setTags(tags: TagEntry[]) {
    this.tags = tags;
    this.rebuildSearchIndex(tags);
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

  search(queryText: string, limit = this.maxResults): TagEntry[] {
    const normalizedQuery = this.normalizeQuery(queryText);
    if (!normalizedQuery) return [];

    const safeLimit = Math.max(1, Math.min(50, limit));
    const prefixMatches: TagEntry[] = [];
    const containsMatches: TagEntry[] = [];
    const aliasMatches: TagEntry[] = [];

    for (const entry of this._searchEntries) {
      if (entry.nameLower.startsWith(normalizedQuery)) {
        this.insertTopByCount(prefixMatches, entry.tag, safeLimit);
      } else if (entry.nameLower.includes(normalizedQuery)) {
        this.insertTopByCount(containsMatches, entry.tag, safeLimit);
      } else if (entry.aliasesLower.some((alias) => alias.startsWith(normalizedQuery) || alias.includes(normalizedQuery))) {
        this.insertTopByCount(aliasMatches, entry.tag, safeLimit);
      }
    }

    const combined = [...prefixMatches, ...containsMatches, ...aliasMatches];
    return combined.slice(0, safeLimit);
  }

  async loadSettings() {
    try {
      this._store = await load("settings.json", { autoSave: true });
      const saved = await this._store.get<Record<string, any>>(STORE_KEY);
      if (saved) {
        if (saved.maxResults) this.maxResults = saved.maxResults;
        if (saved.sourceMode) this.sourceMode = saved.sourceMode;
        if (saved.sourceUrl) this.sourceUrl = saved.sourceUrl;
        if (saved.sourceFileName) this.sourceFileName = saved.sourceFileName;
        if (saved.customTags) {
          this._customTags = saved.customTags;
          if (this.sourceMode !== "builtin" && this._customTags) {
            this.setTags(this._customTags);
          }
        }
      }
    } catch (e) {
      console.error("Failed to load autocomplete settings:", e);
    }
  }

  async saveSettings() {
    if (!this._store) return;
    try {
      await this._store.set(STORE_KEY, {
        maxResults: this.maxResults,
        sourceMode: this.sourceMode,
        sourceUrl: this.sourceUrl,
        sourceFileName: this.sourceFileName,
        customTags: this._customTags,
      });
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
      if (!Array.isArray(parsed)) throw new Error("JSON must be an array");
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
    this.loading = true;
    this.error = null;
    try {
      const resp = await fetch(url);
      if (!resp.ok) throw new Error(`HTTP ${resp.status}: ${resp.statusText}`);
      const text = await resp.text();
      const tags = this.parseTagData(text);
      if (tags.length === 0) throw new Error("No tags found in file");
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
    this.loading = true;
    this.error = null;
    try {
      const tags = this.parseTagData(text);
      if (tags.length === 0) throw new Error("No tags found in file");
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

  /** Reset to built-in danbooru tags */
  async resetToBuiltin() {
    this._customTags = null;
    this.setTags(builtinTags as TagEntry[]);
    this.sourceMode = "builtin";
    this.sourceUrl = "";
    this.sourceFileName = "";
    this.error = null;
    await this.saveSettings();
  }

  /** Update max results and persist */
  async setMaxResults(n: number) {
    this.maxResults = Math.max(1, Math.min(50, n));
    await this.saveSettings();
  }
}

export const autocomplete = new AutocompleteStore();
