/**
 * Frontend orchestrator for collecting and applying Single-ZIP backup bundles.
 */

import { comic } from "../stores/comic.svelte.js";
import { generationTabs } from "../stores/generationTabs.svelte.js";
import { prefsSync, type PrefsCategory } from "../stores/prefsSync.svelte.js";
import { wildcards } from "../stores/wildcards.svelte.js";
import type { BackupBundleInput, BackupInspection } from "./api.js";
import { mergeLocalExtras, type LocalExtrasData } from "./backupMerge.js";
import { ipcStore, userScopedKey } from "./ipc.js";
import type { UserPrefsData } from "./serverPrefs.js";

const COMIC_BOOKS_KEY = "mooshieui.comic.books.v1";
const GENERATION_TABS_KEY = "generation_tabs";
const CHARACTER_PORTRAIT_KEY = "mooshieui.characterPortrait.v1";
const WILDCARDS_ORDER_KEY = "mooshieui.wildcards.order.v1";
const SHORTCUTS_KEY = "mooshieui.shortcuts.v1";

export type BackupCategoryId =
  | "characters"
  | "prompts"
  | "styles"
  | "lora_presets"
  | "generation"
  | "gallery_boards"
  | "comics"
  | "ui_notes";

export interface BackupCategorySpec {
  id: BackupCategoryId;
  labelKey: string;
  prefsCategories: readonly PrefsCategory[];
  localExtrasKeys: readonly (keyof LocalExtrasData)[];
}

export const BACKUP_CATEGORY_SPECS: readonly BackupCategorySpec[] = [
  {
    id: "characters",
    labelKey: "settings.backup.cat.characters",
    prefsCategories: ["characters"],
    localExtrasKeys: ["characterPortrait"],
  },
  {
    id: "prompts",
    labelKey: "settings.backup.cat.prompts",
    prefsCategories: ["prompt_presets", "prompt_history", "enhancer_sessions", "enhance_recipes"],
    localExtrasKeys: [],
  },
  {
    id: "styles",
    labelKey: "settings.backup.cat.styles",
    prefsCategories: ["styles", "artist_favourites", "style_creator", "artist_local_previews"],
    localExtrasKeys: [],
  },
  {
    id: "lora_presets",
    labelKey: "settings.backup.cat.lora_presets",
    prefsCategories: ["lora_presets"],
    localExtrasKeys: [],
  },
  {
    id: "generation",
    labelKey: "settings.backup.cat.generation",
    prefsCategories: ["generation", "profiles", "video_timeline", "anime_scene", "autocomplete"],
    localExtrasKeys: ["generationTabs"],
  },
  {
    id: "gallery_boards",
    labelKey: "settings.backup.cat.gallery_boards",
    prefsCategories: ["gallery_boards"],
    localExtrasKeys: [],
  },
  {
    id: "comics",
    labelKey: "settings.backup.cat.comics",
    prefsCategories: [],
    localExtrasKeys: ["comic"],
  },
  {
    id: "ui_notes",
    labelKey: "settings.backup.cat.ui_notes",
    prefsCategories: ["locale", "accessibility", "notes"],
    localExtrasKeys: ["wildcards_order", "shortcuts"],
  },
];

function readLocalJson<T>(key: string): T | null {
  try {
    const raw = localStorage.getItem(userScopedKey(key));
    if (!raw) return null;
    return JSON.parse(raw) as T;
  } catch {
    return null;
  }
}

function writeLocalJson(key: string, value: unknown): void {
  try {
    if (value === null || value === undefined) {
      localStorage.removeItem(userScopedKey(key));
    } else {
      localStorage.setItem(userScopedKey(key), JSON.stringify(value));
    }
  } catch {
    // Ignore quota/storage errors.
  }
}

export async function collectLocalExtras(): Promise<LocalExtrasData> {
  // Flush any debounced comic edits before snapshotting localStorage.
  comic.flushPendingSave();
  const comicData = readLocalJson<LocalExtrasData["comic"]>(COMIC_BOOKS_KEY);
  const generationTabsData = await ipcStore
    .get<LocalExtrasData["generationTabs"]>(GENERATION_TABS_KEY)
    .catch(() => null);
  const portraitData = readLocalJson<LocalExtrasData["characterPortrait"]>(CHARACTER_PORTRAIT_KEY);
  const wildcardsOrder = readLocalJson<LocalExtrasData["wildcards_order"]>(WILDCARDS_ORDER_KEY);
  const shortcutsData = readLocalJson<LocalExtrasData["shortcuts"]>(SHORTCUTS_KEY);

  const out: LocalExtrasData = {};
  if (comicData) out.comic = comicData;
  if (generationTabsData) out.generationTabs = generationTabsData;
  if (portraitData) out.characterPortrait = portraitData;
  if (wildcardsOrder) out.wildcards_order = wildcardsOrder;
  if (shortcutsData) out.shortcuts = shortcutsData;
  return out;
}

export async function collectBackupBundle(): Promise<BackupBundleInput> {
  const user_prefs = prefsSync.collectAll() as Record<string, unknown>;
  const local_extras = (await collectLocalExtras()) as Record<string, unknown>;
  return { user_prefs, local_extras };
}

function hasObjectData(val: unknown): boolean {
  if (val === null || val === undefined) return false;
  if (Array.isArray(val)) return val.length > 0;
  if (typeof val === "object") return Object.keys(val as Record<string, unknown>).length > 0;
  return true;
}

export function categoryPresentInInspection(
  inspection: BackupInspection,
  spec: BackupCategorySpec,
): boolean {
  const prefs = inspection.user_prefs ?? {};
  const extras = inspection.local_extras ?? {};
  return (
    spec.prefsCategories.some((cat) => hasObjectData(prefs[cat])) ||
    spec.localExtrasKeys.some((key) => hasObjectData(extras[key]))
  );
}

export function categorySummaryCount(
  inspection: BackupInspection,
  id: BackupCategoryId,
): number | null {
  const c = inspection.manifest.counts;
  switch (id) {
    case "characters":
      return c.characters;
    case "prompts":
      return c.prompt_presets + c.prompt_history + (c.enhance_recipes ?? 0);
    case "styles":
      return c.styles + c.artist_favourites;
    case "lora_presets":
      return c.lora_presets + c.lora_triggers;
    case "generation":
      return c.profiles + c.generation_tabs;
    case "comics":
      return c.comic_books;
    default:
      return null;
  }
}

export async function applyBackupUserAndExtras(
  inspection: BackupInspection,
  selectedCategories: ReadonlySet<BackupCategoryId>,
  mode: "merge" | "replace",
): Promise<void> {
  const prefsCats: PrefsCategory[] = [];
  const extrasKeys: (keyof LocalExtrasData)[] = [];

  for (const spec of BACKUP_CATEGORY_SPECS) {
    if (!selectedCategories.has(spec.id)) continue;
    prefsCats.push(...spec.prefsCategories);
    extrasKeys.push(...spec.localExtrasKeys);
  }

  if (prefsCats.length > 0) {
    await prefsSync.applyBackup(
      (inspection.user_prefs ?? {}) as UserPrefsData,
      mode,
      prefsCats,
    );
  }

  if (extrasKeys.length > 0) {
    const incomingExtras = (inspection.local_extras ?? {}) as LocalExtrasData;
    const currentExtras = await collectLocalExtras();
    const nextExtras: LocalExtrasData =
      mode === "merge"
        ? mergeLocalExtras(currentExtras, incomingExtras, extrasKeys)
        : { ...currentExtras };

    if (mode === "replace") {
      for (const key of extrasKeys) {
        if (incomingExtras[key] !== undefined) {
          (nextExtras as Record<string, unknown>)[key] = incomingExtras[key];
        }
      }
    }

    if (extrasKeys.includes("comic") && nextExtras.comic) {
      writeLocalJson(COMIC_BOOKS_KEY, nextExtras.comic);
      comic.reload();
    }
    if (extrasKeys.includes("generationTabs") && nextExtras.generationTabs !== undefined) {
      await ipcStore.set(GENERATION_TABS_KEY, nextExtras.generationTabs);
      await generationTabs.load();
    }
    if (extrasKeys.includes("characterPortrait") && nextExtras.characterPortrait !== undefined) {
      writeLocalJson(CHARACTER_PORTRAIT_KEY, nextExtras.characterPortrait);
    }
    if (extrasKeys.includes("wildcards_order") && nextExtras.wildcards_order !== undefined) {
      writeLocalJson(WILDCARDS_ORDER_KEY, nextExtras.wildcards_order);
      wildcards.reloadOrder();
    }
    if (extrasKeys.includes("shortcuts") && nextExtras.shortcuts !== undefined) {
      writeLocalJson(SHORTCUTS_KEY, nextExtras.shortcuts);
    }
  }
}
