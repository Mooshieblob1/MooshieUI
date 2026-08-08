/**
 * Central hub for server-side user preference sync.
 *
 * All stores call `triggerSync()` whenever they persist settings.
 * This store registers itself as the sync handler and debounces those calls
 * into a single push to the server (2 s window).
 *
 * On startup/login, `loadAndApply()` pulls the server snapshot and distributes
 * it to every participating store.  If no server snapshot exists yet the
 * current local state is seeded to the server.
 *
 * Only active in browser / LAN mode — `pushServerPrefs` / `fetchServerPrefs`
 * are no-ops in Tauri desktop mode.
 */

import { registerSyncHandler } from "../utils/syncTrigger.js";
import { fetchServerPrefs, pushServerPrefs, type UserPrefsData } from "../utils/serverPrefs.js";
import { generation } from "./generation.svelte.js";
import { promptPresets } from "./promptPresets.svelte.js";
import { styles } from "./styles.svelte.js";
import { loraPresets } from "./loraPresets.svelte.js";
import { artistFavourites } from "../artist-gallery/favourites.svelte.js";
import { gallery } from "./gallery.svelte.js";
import { accessibility } from "./accessibility.svelte.js";
import { locale } from "./locale.svelte.js";
import { autocomplete } from "./autocomplete.svelte.js";
import { notes } from "./notes.svelte.js";
// Also the load-bearing import that registers the timeline compiler with
// `utils/timelineProvider.ts` before the first generation.
import { videoTimeline } from "./videoTimeline.svelte.js";

class PrefsSyncStore {
  private _syncTimer: ReturnType<typeof setTimeout> | null = null;
  private _syncing = false;
  private _pending = false;

  constructor() {
    registerSyncHandler(() => this.scheduleSync());
  }

  /** Gather the current state of all participating stores. */
  collectAll(): UserPrefsData {
    return {
      generation: generation.collectPrefs(),
      prompt_history: generation.collectPromptHistory(),
      prompt_presets: promptPresets.collectPrefs(),
      styles: styles.collectPrefs(),
      lora_presets: loraPresets.collectPrefs(),
      artist_favourites: artistFavourites.collectPrefs(),
      gallery_boards: gallery.collectPrefs(),
      autocomplete: autocomplete.collectPrefs(),
      accessibility: accessibility.collectPrefs(),
      notes: notes.collectPrefs(),
      video_timeline: videoTimeline.collectPrefs(),
      locale: locale.current,
    };
  }

  /** Distribute a server snapshot to every participating store. */
  async applyAll(prefs: UserPrefsData): Promise<void> {
    if (prefs.generation) {
      await generation.applyServerPrefs(prefs.generation as Record<string, any>).catch(() => {});
    }
    if (Array.isArray(prefs.prompt_history)) {
      generation.applyPromptHistory(prefs.prompt_history as any[]);
    }
    if (prefs.prompt_presets) {
      promptPresets.applyServerPrefs(prefs.prompt_presets);
    }
    if (prefs.styles) {
      styles.applyServerPrefs(prefs.styles);
    }
    if (prefs.lora_presets) {
      loraPresets.applyServerPrefs(prefs.lora_presets);
    }
    if (prefs.artist_favourites) {
      artistFavourites.applyServerPrefs(prefs.artist_favourites);
    }
    if (prefs.gallery_boards) {
      gallery.applyServerPrefs(prefs.gallery_boards);
    }
    if (prefs.autocomplete) {
      await autocomplete.applyServerPrefs(prefs.autocomplete as Record<string, any>).catch(() => {});
    }
    if (prefs.accessibility) {
      accessibility.applyServerPrefs(prefs.accessibility);
    }
    if (prefs.notes) {
      notes.applyServerPrefs(prefs.notes);
    }
    if (prefs.video_timeline) {
      videoTimeline.applyServerPrefs(prefs.video_timeline);
    }
    if (typeof prefs.locale === "string") {
      locale.applyServerPrefs(prefs.locale);
    }
  }

  /**
   * Fetch server prefs and apply them.  If the server has no prefs yet,
   * seed it with the current local state.
   */
  async loadAndApply(): Promise<void> {
    try {
      const prefs = await fetchServerPrefs();
      if (prefs) {
        await this.applyAll(prefs);
      } else {
        // No snapshot on server yet — push current local state to seed it.
        await pushServerPrefs(this.collectAll());
      }
    } catch {
      // Non-fatal — offline or server unavailable.
    }
  }

  /** Debounce: collapses rapid consecutive saves into one server push. */
  scheduleSync(): void {
    if (this._syncTimer !== null) clearTimeout(this._syncTimer);
    this._syncTimer = setTimeout(() => {
      this._syncTimer = null;
      this._doSync().catch(() => {});
    }, 2000);
  }

  private async _doSync(): Promise<void> {
    // If a push is already in flight, mark that another is needed rather than
    // dropping it — changes made mid-flight would otherwise never reach the
    // server until the next unrelated save.
    if (this._syncing) {
      this._pending = true;
      return;
    }
    this._syncing = true;
    try {
      await pushServerPrefs(this.collectAll());
    } finally {
      this._syncing = false;
    }
    if (this._pending) {
      this._pending = false;
      // Re-run once to flush the state that changed during the last push.
      this._doSync().catch(() => {});
    }
  }
}

export const prefsSync = new PrefsSyncStore();
