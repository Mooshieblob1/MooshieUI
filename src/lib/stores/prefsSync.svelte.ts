/**
 * Central hub for the user's saved data (characters, presets, styles, artist
 * favourites, prompt history, settings...).
 *
 * The backend keeps one copy per account (`user_prefs.rs`). Desktop mode and
 * browser mode on the same machine both use the owner's copy, so whatever is
 * saved in one mode shows up in the other. Each store still keeps its own
 * local cache (localStorage / the Tauri store) for a fast start.
 *
 * - Startup: `loadAndApply()` pulls the shared copy and applies it, or seeds
 *   it from local state when nothing is saved yet.
 * - Saving: stores call `triggerSync()`; this debounces (2 s) and pushes only
 *   the categories that changed, so a window cannot overwrite another
 *   window's newer edit to a different category.
 * - Switching: when the window regains focus it pulls again and applies any
 *   category the other mode changed meanwhile.
 * - One-time merge: before this, each mode kept its own copy. The first start
 *   of each mode merges its local copy into the shared one (`prefsMerge.ts`),
 *   so data saved on either side survives.
 */

import { registerSyncHandler } from "../utils/syncTrigger.js";
import {
  fetchServerPrefs,
  pushServerPrefs,
  serverPrefsAvailable,
  type UserPrefsData,
} from "../utils/serverPrefs.js";
import { mergeUserPrefs } from "../utils/prefsMerge.js";
import { getAuthToken, isTauri, userScopedKey } from "../utils/ipc.js";
import { generation } from "./generation.svelte.js";
import { promptPresets } from "./promptPresets.svelte.js";
import { styles } from "./styles.svelte.js";
import { loraPresets } from "./loraPresets.svelte.js";
import { profiles } from "./profiles.svelte.js";
import { artistFavourites } from "../artist-gallery/favourites.svelte.js";
import { gallery } from "./gallery.svelte.js";
import { accessibility } from "./accessibility.svelte.js";
import { locale } from "./locale.svelte.js";
import { autocomplete } from "./autocomplete.svelte.js";
import { notes } from "./notes.svelte.js";
import { savedCharacters } from "./savedCharacters.svelte.js";
import { artistLocalPreviews } from "./artistLocalPreviews.svelte.js";
import { enhancerSessions } from "./enhancerSessions.svelte.js";
import { styleCreator } from "./styleCreator.svelte.js";
import { animeScene } from "./animeScene.svelte.js";
// Also the load-bearing import that registers the timeline compiler with
// `utils/timelineProvider.ts` before the first generation.
import { videoTimeline } from "./videoTimeline.svelte.js";

/** Set (per mode, per account) once this mode's local copy has been merged in. */
const MERGED_FLAG_KEY = "mooshieui.prefsMerged.v1";
/** Ignore focus bursts: one pull per this many ms is plenty. */
const REFRESH_MIN_INTERVAL_MS = 3000;
/** Browsers reject keepalive bodies over 64 KB; stay under it. */
const KEEPALIVE_MAX_BYTES = 60_000;

const CATEGORIES = [
  "generation",
  "prompt_history",
  "prompt_presets",
  "styles",
  "lora_presets",
  "profiles",
  "artist_favourites",
  "gallery_boards",
  "autocomplete",
  "accessibility",
  "notes",
  "characters",
  "video_timeline",
  "locale",
  "artist_local_previews",
  "enhancer_sessions",
  "style_creator",
  "anime_scene",
] as const satisfies readonly (keyof UserPrefsData)[];

type Category = (typeof CATEGORIES)[number];

function sortKeys(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sortKeys);
  if (value && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const key of Object.keys(value).sort()) out[key] = sortKeys((value as Record<string, unknown>)[key]);
    return out;
  }
  return value;
}

/**
 * Order-insensitive JSON, for telling whether a category changed. The server
 * stores objects with sorted keys, so plain `JSON.stringify` would see every
 * round trip as a change.
 */
function canonical(value: unknown): string {
  return JSON.stringify(sortKeys(value)) ?? "";
}

function readFlag(): boolean {
  try {
    return localStorage.getItem(userScopedKey(MERGED_FLAG_KEY)) === "1";
  } catch {
    return false;
  }
}

function writeFlag(): void {
  try {
    localStorage.setItem(userScopedKey(MERGED_FLAG_KEY), "1");
  } catch {
    /* storage blocked: the merge runs again next start, which is harmless */
  }
}

class PrefsSyncStore {
  private _syncTimer: ReturnType<typeof setTimeout> | null = null;
  private _syncing = false;
  private _pending = false;
  /** True once the shared copy has been loaded; nothing is pushed before. */
  private _ready = false;
  private _loading: Promise<void> | null = null;
  /** Set while applying pulled data, so the stores' own saves don't echo it back. */
  private _applying = false;
  private _lastRefresh = 0;
  /** Per category: this window's state as of the last push or pull. */
  private _lastLocal = new Map<Category, string>();
  /** Per category: the shared copy as of the last push or pull. */
  private _lastServer = new Map<Category, string>();

  constructor() {
    if (!serverPrefsAvailable) return;
    registerSyncHandler(() => this.scheduleSync());
    // A change made in the last two seconds before the tab closes would
    // otherwise sit in the debounce timer and never reach the server, and the
    // next launch's `loadAndApply` would then overwrite it with the stale
    // snapshot. `pagehide` is the last reliable event on both desktop and
    // mobile browsers; hidden tabs count too, because a backgrounded tab may
    // be discarded without ever firing `pagehide`.
    if (typeof window !== "undefined") {
      window.addEventListener("pagehide", () => this.flush());
      document.addEventListener("visibilitychange", () => {
        if (document.visibilityState === "hidden") this.flush();
        else void this.refresh();
      });
      // Coming back from the other mode: pick up what was saved there.
      window.addEventListener("focus", () => void this.refresh());
    }
  }

  /** Gather the current state of all participating stores. */
  collectAll(): UserPrefsData {
    return {
      generation: generation.collectPrefs(),
      prompt_history: generation.collectPromptHistory(),
      prompt_presets: promptPresets.collectPrefs(),
      styles: styles.collectPrefs(),
      lora_presets: loraPresets.collectPrefs(),
      profiles: profiles.collectPrefs(),
      artist_favourites: artistFavourites.collectPrefs(),
      gallery_boards: gallery.collectPrefs(),
      autocomplete: autocomplete.collectPrefs(),
      accessibility: accessibility.collectPrefs(),
      notes: notes.collectPrefs(),
      characters: savedCharacters.collectPrefs(),
      video_timeline: videoTimeline.collectPrefs(),
      locale: locale.current,
      artist_local_previews: artistLocalPreviews.collectPrefs(),
      enhancer_sessions: enhancerSessions.collectPrefs(),
      style_creator: styleCreator.collectPrefs(),
      anime_scene: animeScene.collectPrefs(),
    };
  }

  private async applyCategory(category: Category, value: unknown): Promise<void> {
    const data = value as any;
    switch (category) {
      case "generation":
        await generation.applyServerPrefs(data).catch((e) => { console.warn("Failed to apply server prefs (generation):", e); });
        return;
      case "prompt_history":
        if (Array.isArray(data)) generation.applyPromptHistory(data);
        return;
      case "prompt_presets": return promptPresets.applyServerPrefs(data);
      case "styles": return styles.applyServerPrefs(data);
      case "lora_presets": return loraPresets.applyServerPrefs(data);
      case "profiles": return profiles.applyServerPrefs(data);
      case "artist_favourites": return artistFavourites.applyServerPrefs(data);
      case "gallery_boards": return gallery.applyServerPrefs(data);
      case "autocomplete":
        await autocomplete.applyServerPrefs(data).catch((e) => { console.warn("Failed to apply server prefs (autocomplete):", e); });
        return;
      case "accessibility": return accessibility.applyServerPrefs(data);
      case "notes": return notes.applyServerPrefs(data);
      case "characters": return savedCharacters.applyServerPrefs(data);
      case "video_timeline": return videoTimeline.applyServerPrefs(data);
      case "locale":
        if (typeof data === "string") locale.applyServerPrefs(data);
        return;
      case "artist_local_previews": return artistLocalPreviews.applyServerPrefs(data);
      case "enhancer_sessions": return enhancerSessions.applyServerPrefs(data);
      case "style_creator": return styleCreator.applyServerPrefs(data);
      case "anime_scene": return animeScene.applyServerPrefs(data);
    }
  }

  /**
   * Apply every category of `prefs` that differs from the shared copy this
   * window last saw (all of them with `force`), and remember both sides.
   */
  private async applyChanged(prefs: UserPrefsData, force = false): Promise<void> {
    const applied: Category[] = [];
    this._applying = true;
    try {
      for (const category of CATEGORIES) {
        const value = prefs[category];
        if (value === undefined || value === null) continue;
        const serverJson = canonical(value);
        if (!force && this._lastServer.get(category) === serverJson) continue;
        try {
          await this.applyCategory(category, value);
        } catch (e) {
          console.warn(`Failed to apply server prefs (${category}):`, e);
        }
        this._lastServer.set(category, serverJson);
        applied.push(category);
      }
    } finally {
      this._applying = false;
    }
    if (applied.length === 0) return;
    // What the stores hold now (after their own sanitising) is in sync.
    const local = this.collectAll();
    for (const category of applied) this._lastLocal.set(category, canonical(local[category]));
  }

  /** Whether this mode still has to merge its own copy into the shared one. */
  private needsMerge(): boolean {
    if (readFlag()) return false;
    // A signed-in browser account already synced through the server before
    // the copies were unified, so it has nothing separate to merge.
    return isTauri || !getAuthToken();
  }

  /**
   * One-time merge of this mode's local copy into the shared one. Lists are
   * combined; single settings follow the desktop app's copy (see
   * `prefsMerge.ts`). The server keeps the file as it was before.
   */
  private async mergeOnce(server: UserPrefsData | null): Promise<boolean> {
    const local = this.collectAll();
    let merged: UserPrefsData;
    if (!server) merged = local;
    // The desktop app's copy is the primary one: here, or already in the
    // shared copy once the desktop app has merged.
    else if (!isTauri && server.desktop_merged) merged = mergeUserPrefs(server, local);
    else merged = mergeUserPrefs(local, server);

    const body: UserPrefsData = { merge_source: isTauri ? "desktop" : "browser" };
    for (const category of CATEGORIES) {
      if (merged[category] !== undefined) (body as Record<string, unknown>)[category] = merged[category];
    }
    if (isTauri) body.desktop_merged = true;
    if ((await pushServerPrefs(body)) === null) return false;
    writeFlag();
    await this.applyChanged(body, true);
    return true;
  }

  /**
   * Pull the shared copy and apply it (merging this mode's own copy in first
   * on its first start). Seeds the shared copy from local state when nothing
   * is saved yet.
   */
  loadAndApply(): Promise<void> {
    if (!serverPrefsAvailable) return Promise.resolve();
    this._loading ??= this._load()
      .catch((e) => { console.warn("Failed to load/apply server prefs (offline or server unavailable):", e); })
      .finally(() => { this._loading = null; });
    return this._loading;
  }

  private async _load(): Promise<void> {
    const result = await fetchServerPrefs();
    // Unknown state: push nothing, or local data could replace the real copy.
    if (result.status === "unavailable") return;
    const server = result.status === "ok" ? result.prefs : null;
    if (this.needsMerge()) {
      if (!(await this.mergeOnce(server))) return;
    } else if (server) {
      await this.applyChanged(server, !this._ready);
    }
    this._ready = true;
    this._lastRefresh = Date.now();
    // Push whatever the shared copy lacks: everything when it was empty, or
    // categories added since it was last saved.
    void this._doSync();
  }

  /** Pull and apply what the other mode saved since this window last synced. */
  async refresh(): Promise<void> {
    if (!serverPrefsAvailable || this._loading) return;
    if (!this._ready) {
      await this.loadAndApply();
      return;
    }
    const now = Date.now();
    if (now - this._lastRefresh < REFRESH_MIN_INTERVAL_MS) return;
    this._lastRefresh = now;
    // Our own unsent edits go first, so the pull cannot undo them.
    if (this._syncTimer !== null) {
      clearTimeout(this._syncTimer);
      this._syncTimer = null;
      await this._doSync();
    }
    const result = await fetchServerPrefs();
    if (result.status === "ok") await this.applyChanged(result.prefs);
  }

  /** Debounce: collapses rapid consecutive saves into one server push. */
  scheduleSync(): void {
    if (this._applying) return;
    if (this._syncTimer !== null) clearTimeout(this._syncTimer);
    this._syncTimer = setTimeout(() => {
      this._syncTimer = null;
      this._doSync().catch((e) => { console.warn("Prefs sync failed:", e); });
    }, 2000);
  }

  /**
   * Push now if a debounced push is waiting. A no-op when nothing has changed
   * since the last push, so it is safe to call on every hide.
   */
  flush(): void {
    if (this._syncTimer === null) return;
    clearTimeout(this._syncTimer);
    this._syncTimer = null;
    this._doSync({ keepalive: true }).catch((e) => { console.warn("Prefs sync (flush) failed:", e); });
  }

  private async _doSync(options: { keepalive?: boolean } = {}): Promise<void> {
    if (!this._ready) return;
    // If a push is already in flight, mark that another is needed rather than
    // dropping it — changes made mid-flight would otherwise never reach the
    // server until the next unrelated save.
    if (this._syncing) {
      this._pending = true;
      return;
    }
    this._syncing = true;
    try {
      const all = this.collectAll();
      const body: UserPrefsData = {};
      const sent = new Map<Category, string>();
      for (const category of CATEGORIES) {
        const value = all[category];
        if (value === undefined) continue;
        const json = canonical(value);
        if (this._lastLocal.get(category) === json) continue;
        (body as Record<string, unknown>)[category] = value;
        sent.set(category, json);
      }
      if (sent.size > 0) {
        const keepalive = options.keepalive === true && JSON.stringify(body).length < KEEPALIVE_MAX_BYTES;
        if ((await pushServerPrefs(body, { keepalive })) !== null) {
          for (const [category, json] of sent) {
            this._lastLocal.set(category, json);
            this._lastServer.set(category, json);
          }
        }
      }
    } finally {
      this._syncing = false;
    }
    if (this._pending) {
      this._pending = false;
      // Re-run once to flush the state that changed during the last push.
      this._doSync().catch((e) => { console.warn("Prefs sync (re-flush) failed:", e); });
    }
  }
}

export const prefsSync = new PrefsSyncStore();
