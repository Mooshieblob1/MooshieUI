/**
 * Pure merge helpers for `local_extras.json` backup restore (`comic`,
 * `generationTabs`, `characterPortrait`, `wildcards_order`, `shortcuts`).
 * Store-free so `node:test` can verify them directly.
 */

import { mergeList, mergeRecord } from "./prefsMerge.js";

type Obj = Record<string, unknown>;

export interface ComicBooksExtras {
  books?: unknown[];
  activeBookId?: string;
}

export interface GenerationTabsExtras {
  activeId?: string;
  tabs?: unknown[];
}

export interface CharacterPortraitExtras {
  artists?: string;
  chunkIds?: string[];
  styleIds?: string[];
}

export interface WildcardsOrderExtras {
  ordered?: string[];
  counters?: Record<string, number>;
}

export interface LocalExtrasData {
  comic?: ComicBooksExtras | null;
  generationTabs?: GenerationTabsExtras | null;
  characterPortrait?: CharacterPortraitExtras | null;
  wildcards_order?: WildcardsOrderExtras | null;
  shortcuts?: Record<string, string> | null;
}

function isObj(v: unknown): v is Obj {
  return !!v && typeof v === "object" && !Array.isArray(v);
}

/** Merge two comic-book collections by book `id`, keeping `primary`'s order and active book when valid. */
export function mergeComicBooks(primary: unknown, secondary: unknown): ComicBooksExtras {
  const p = isObj(primary) ? primary : {};
  const s = isObj(secondary) ? secondary : {};
  const books = mergeList(p.books, s.books, (b) => (typeof b.id === "string" && b.id ? `id:${b.id}` : null));
  const ids = new Set(
    books.filter((b): b is Obj => isObj(b) && typeof b.id === "string").map((b) => b.id as string),
  );
  const pickActive = (candidate: unknown): string | undefined =>
    typeof candidate === "string" && ids.has(candidate) ? candidate : undefined;
  const firstId =
    books.find((b): b is Obj => isObj(b) && typeof b.id === "string")?.id as string | undefined;
  const activeBookId = pickActive(p.activeBookId) ?? pickActive(s.activeBookId) ?? firstId;
  return activeBookId ? { books, activeBookId } : { books };
}

/** Merge two generation-tab stores by tab `id`, ensuring non-active tabs keep snapshots. */
export function mergeGenerationTabs(primary: unknown, secondary: unknown): GenerationTabsExtras | null {
  if (!isObj(primary) && !isObj(secondary)) return null;
  const p = isObj(primary) ? primary : {};
  const s = isObj(secondary) ? secondary : {};
  const rawTabs = mergeList(p.tabs, s.tabs, (t) => (typeof t.id === "string" && t.id ? `id:${t.id}` : null));
  const validIds = new Set(
    rawTabs.filter((t): t is Obj => isObj(t) && typeof t.id === "string").map((t) => t.id as string),
  );
  const pickActive = (candidate: unknown): string | undefined =>
    typeof candidate === "string" && validIds.has(candidate) ? candidate : undefined;
  const firstId =
    rawTabs.find((t): t is Obj => isObj(t) && typeof t.id === "string")?.id as string | undefined;
  const activeId = pickActive(p.activeId) ?? pickActive(s.activeId) ?? firstId;
  const tabs = rawTabs.filter(
    (t): t is Obj => isObj(t) && typeof t.id === "string" && (t.id === activeId || isObj(t.snapshot)),
  );
  return { ...(activeId ? { activeId } : {}), tabs };
}

/** Merge character portrait extras, unioning chunkIds and styleIds. */
export function mergeCharacterPortrait(primary: unknown, secondary: unknown): CharacterPortraitExtras | null {
  if (!isObj(primary) && !isObj(secondary)) return null;
  const p = isObj(primary) ? primary : {};
  const s = isObj(secondary) ? secondary : {};
  const pArtists = typeof p.artists === "string" ? p.artists : "";
  const sArtists = typeof s.artists === "string" ? s.artists : "";
  const strList = (v: unknown) => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);
  return {
    artists: pArtists.trim() ? pArtists : sArtists,
    chunkIds: [...new Set([...strList(p.chunkIds), ...strList(s.chunkIds)])],
    styleIds: [...new Set([...strList(p.styleIds), ...strList(s.styleIds)])],
  };
}

/** Merge wildcard order lists and per-wildcard sequential counters (`primary` wins counter clashes). */
export function mergeWildcardsOrder(primary: unknown, secondary: unknown): WildcardsOrderExtras | null {
  if (!isObj(primary) && !isObj(secondary)) return null;
  const p = isObj(primary) ? primary : {};
  const s = isObj(secondary) ? secondary : {};
  const strList = (v: unknown) => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);
  const sanitizeCounters = (v: unknown): Record<string, number> => {
    if (!isObj(v)) return {};
    const out: Record<string, number> = {};
    for (const [k, n] of Object.entries(v)) {
      if (typeof n === "number" && Number.isFinite(n)) out[k] = n;
    }
    return out;
  };
  return {
    ordered: [...new Set([...strList(p.ordered), ...strList(s.ordered)])],
    counters: { ...sanitizeCounters(s.counters), ...sanitizeCounters(p.counters) },
  };
}

/** Merge custom keyboard shortcut bindings (`primary` wins per-action clashes). */
export function mergeShortcuts(primary: unknown, secondary: unknown): Record<string, string> {
  const raw = mergeRecord(primary, secondary);
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(raw)) {
    if (typeof v === "string" && k) out[k] = v;
  }
  return out;
}

/** Merge selected `local_extras` keys between `primary` (existing) and `secondary` (backup). */
export function mergeLocalExtras(
  primary: LocalExtrasData,
  secondary: LocalExtrasData,
  keys: readonly (keyof LocalExtrasData)[],
): LocalExtrasData {
  const out: LocalExtrasData = { ...primary };
  for (const key of keys) {
    const pVal = primary[key];
    const sVal = secondary[key];
    if (sVal === undefined || sVal === null) continue;
    if (pVal === undefined || pVal === null) {
      (out as Record<string, unknown>)[key] = sVal;
      continue;
    }
    switch (key) {
      case "comic":
        out.comic = mergeComicBooks(pVal, sVal);
        break;
      case "generationTabs":
        out.generationTabs = mergeGenerationTabs(pVal, sVal);
        break;
      case "characterPortrait":
        out.characterPortrait = mergeCharacterPortrait(pVal, sVal);
        break;
      case "wildcards_order":
        out.wildcards_order = mergeWildcardsOrder(pVal, sVal);
        break;
      case "shortcuts":
        out.shortcuts = mergeShortcuts(pVal, sVal);
        break;
    }
  }
  return out;
}
