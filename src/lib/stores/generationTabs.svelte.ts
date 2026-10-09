import { ipcStore } from "../utils/ipc.js";
import { generation } from "./generation.svelte.js";

const TABS_KEY = "generation_tabs";

/**
 * App-wide preferences that `collectPrefs()` carries alongside the image
 * settings. They stay out of tab snapshots, so changing one in any tab changes
 * it everywhere instead of flipping back when the user switches tabs.
 */
const SHARED_KEYS = new Set([
  "showNovelaiUsage",
  "naiEnhanceLanguage",
  "naiEnhanceIncludeExisting",
  "naiEnhanceScaleChoice",
  "naiEnhanceMagnitude",
  "outputBitDepth",
  "outputFormat",
  "metadataMode",
  "autoQualityTags",
  "customQualityTagsEnabled",
  "customAnimaPositiveQuality",
  "customAnimaNegativeQuality",
  "customIllustriousPositiveQuality",
  "customIllustriousNegativeQuality",
  "customPonyPositiveQuality",
  "customPonyNegativeQuality",
  "customNanosaurPositiveQuality",
  "customNanosaurNegativeQuality",
  "modelFamilyOverrides",
  "osNotificationsEnabled",
  "osNotifyOnlyWhenUnfocused",
  "manualSaveMode",
  "advancedMode",
  "resolutionLocked",
  "autoSaveDirs",
]);

type Snapshot = Record<string, unknown>;

export interface GenerationTab {
  id: string;
  /**
   * The tab's parked settings. Null for the active tab: its settings are the
   * live generation fields, persisted by the generation store as before.
   */
  snapshot: Snapshot | null;
}

function newTabId(): string {
  return crypto.randomUUID?.() || `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

/** The live settings minus app-wide preferences, detached from `$state` proxies. */
function captureSnapshot(): Snapshot {
  const prefs = JSON.parse(JSON.stringify(generation.collectPrefs())) as Snapshot;
  for (const key of SHARED_KEYS) delete prefs[key];
  return prefs;
}

/**
 * Browser-style tabs over the generation settings. Each tab is one full set of
 * image settings; the queue, gallery and canvas stay shared. Only the active
 * tab lives in the generation store, the others wait here as snapshots.
 */
class GenerationTabsStore {
  // Raw: snapshots are plain data, only ever replaced wholesale, never mutated.
  tabs = $state.raw<GenerationTab[]>([{ id: newTabId(), snapshot: null }]);
  activeId = $state(this.tabs[0].id);

  get count(): number {
    return this.tabs.length;
  }

  get activeIndex(): number {
    return Math.max(0, this.tabs.findIndex((tab) => tab.id === this.activeId));
  }

  /** Short label: the prompt's opening words, or the mode when it is empty. */
  label(tab: GenerationTab): { prompt: string; mode: string } {
    const live = tab.id === this.activeId;
    const source = tab.snapshot ?? {};
    const prompt = live ? generation.positivePrompt : String(source.positivePrompt ?? "");
    const mode = live ? generation.mode : String(source.mode ?? "txt2img");
    return { prompt: prompt.replace(/\s+/g, " ").trim(), mode };
  }

  /** Must run after `generation.loadSettings()`, which restores the active tab. */
  async load(): Promise<void> {
    try {
      const saved = await ipcStore.get<{ activeId?: unknown; tabs?: unknown }>(TABS_KEY);
      if (!saved || !Array.isArray(saved.tabs)) return;
      const tabs: GenerationTab[] = saved.tabs
        .filter((tab: any) => !!tab && typeof tab.id === "string" && tab.id)
        .map((tab: any) => ({
          id: tab.id,
          snapshot: tab.snapshot && typeof tab.snapshot === "object" ? tab.snapshot : null,
        }));
      const active = tabs.find((tab) => tab.id === saved.activeId);
      if (!active || tabs.length < 2) return;
      // Every other tab needs a snapshot to switch to; drop any that lost it.
      active.snapshot = null;
      this.tabs = tabs.filter((tab) => tab === active || tab.snapshot);
      this.activeId = active.id;
    } catch (e) {
      console.error("generationTabs: load failed", e);
    }
  }

  private async persist(): Promise<void> {
    try {
      await ipcStore.set(
        TABS_KEY,
        this.tabs.length > 1 ? { activeId: this.activeId, tabs: this.tabs } : null,
      );
    } catch (e) {
      console.error("generationTabs: save failed", e);
    }
  }

  /** Open a copy of the current tab next to it and switch to the copy. */
  newTab(): void {
    const index = this.activeIndex;
    const tab: GenerationTab = { id: newTabId(), snapshot: null };
    // The copy keeps the live settings, so the tab being left parks a snapshot.
    const tabs = this.tabs.map((t) => (t.id === this.activeId ? { ...t, snapshot: captureSnapshot() } : t));
    this.tabs = [...tabs.slice(0, index + 1), tab, ...tabs.slice(index + 1)];
    this.activeId = tab.id;
    void this.persist();
  }

  switchTo(id: string): void {
    if (id === this.activeId) return;
    const target = this.tabs.find((tab) => tab.id === id);
    if (!target?.snapshot) return;
    const parked = captureSnapshot();
    // Cloned so the live fields never share objects with a parked snapshot.
    generation.applySettingsSnapshot(JSON.parse(JSON.stringify(target.snapshot)));
    this.tabs = this.tabs.map((tab) => {
      if (tab.id === this.activeId) return { ...tab, snapshot: parked };
      if (tab.id === id) return { ...tab, snapshot: null };
      return tab;
    });
    this.activeId = id;
    void generation.saveSettings();
    void this.persist();
  }

  /** Step through tabs, wrapping at either end. */
  cycle(direction: 1 | -1): void {
    if (this.tabs.length < 2) return;
    const next = (this.activeIndex + direction + this.tabs.length) % this.tabs.length;
    this.switchTo(this.tabs[next].id);
  }

  close(id: string): void {
    if (this.tabs.length < 2) return;
    const index = this.tabs.findIndex((tab) => tab.id === id);
    if (index < 0) return;
    if (id === this.activeId) {
      const neighbour = this.tabs[index + 1] ?? this.tabs[index - 1];
      this.switchTo(neighbour.id);
    }
    this.tabs = this.tabs.filter((tab) => tab.id !== id);
    void this.persist();
  }
}

export const generationTabs = new GenerationTabsStore();
