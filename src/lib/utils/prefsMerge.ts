/**
 * One-time merge of two copies of a user's saved data.
 *
 * Desktop and browser mode used to keep separate copies (each in its own
 * webview storage), so a user who used both has data on both sides. This
 * combines them without dropping anything a user saved:
 *
 * - Lists (characters, presets, styles, artist favourites, history...) are
 *   combined and de-duplicated by id, or by name/slug where there is no id. On
 *   a clash the more recently edited copy wins, ties going to `primary`.
 * - Single settings (generation settings, locale, toggles) come from
 *   `primary`, with anything it lacks filled in from `secondary`.
 *
 * `primary` is the desktop app's copy (the caller decides which side that is).
 * Pure and store-free so it can be reasoned about on its own.
 */

import type { UserPrefsData } from "./serverPrefs.js";

type Obj = Record<string, unknown>;

/** Mirrors `MAX_PROMPT_HISTORY` in the generation store. */
export const MERGED_PROMPT_HISTORY_MAX = 100;
/** Mirrors `MAX_SESSION_TURNS` in `enhancerSession.ts`. */
const MERGED_SESSION_TURNS_MAX = 50;
/** Mirrors `MAX_HISTORY` in the Style Creator store. */
const MERGED_STYLE_HISTORY_MAX = 5000;

function isObj(v: unknown): v is Obj {
  return !!v && typeof v === "object" && !Array.isArray(v);
}

function num(v: unknown): number {
  return typeof v === "number" && Number.isFinite(v) ? v : 0;
}

/** When an item was last edited, for picking a side on a clash. */
function editedAt(item: Obj): number {
  return Math.max(num(item.updatedAt), num(item.createdAt), num(item.addedAt), num(item.at));
}

function defaultKey(item: Obj): string | null {
  if (typeof item.id === "string" && item.id) return `id:${item.id}`;
  if (typeof item.slug === "string" && item.slug) return `slug:${item.slug}`;
  if (typeof item.name === "string" && item.name) return `name:${item.name.trim().toLowerCase()}`;
  return null;
}

/**
 * Union of two lists, keyed by `keyOf`. `primary`'s order comes first, then
 * the items only `secondary` has. Items without a key are all kept.
 */
export function mergeList(
  primary: unknown,
  secondary: unknown,
  keyOf: (item: Obj) => string | null = defaultKey,
): unknown[] {
  const a = Array.isArray(primary) ? primary : [];
  const b = Array.isArray(secondary) ? secondary : [];
  const out: unknown[] = [];
  const index = new Map<string, number>();
  // Keyless primitives (board names, keywords) de-duplicate by value.
  const values = new Set<unknown>();
  const add = (item: unknown, fromPrimary: boolean) => {
    if (!isObj(item)) {
      if (values.has(item)) return;
      values.add(item);
      out.push(item);
      return;
    }
    const key = keyOf(item);
    if (key === null) {
      out.push(item);
      return;
    }
    const at = index.get(key);
    if (at === undefined) {
      index.set(key, out.length);
      out.push(item);
    } else if (!fromPrimary && editedAt(item) > editedAt(out[at] as Obj)) {
      out[at] = item;
    }
  };
  for (const item of a) add(item, true);
  for (const item of b) add(item, false);
  return out;
}

/** Union of two keyed records; `primary` wins where both have a key. */
export function mergeRecord(primary: unknown, secondary: unknown): Obj {
  return { ...(isObj(secondary) ? secondary : {}), ...(isObj(primary) ? primary : {}) };
}

/** `primary`'s single settings, with keys it lacks taken from `secondary`. */
function fill(primary: unknown, secondary: unknown): unknown {
  if (isObj(primary) && isObj(secondary)) return { ...secondary, ...primary };
  return primary ?? secondary;
}

function mergeNotes(primary: unknown, secondary: unknown): unknown {
  const a = isObj(primary) && typeof primary.text === "string" ? primary.text : "";
  const b = isObj(secondary) && typeof secondary.text === "string" ? secondary.text : "";
  const base = isObj(primary) ? primary : isObj(secondary) ? secondary : {};
  if (!b.trim() || a.includes(b)) return primary;
  if (!a.trim() || b.includes(a)) return { ...base, text: b };
  // Both sides wrote different notes: keep both.
  return { ...base, text: `${a.trimEnd()}\n\n${b.trimStart()}` };
}

/** Same prompt pair, mode and style: the generation store's own dedupe rule. */
function historyKey(item: Obj): string {
  return JSON.stringify([item.positivePrompt, item.negativePrompt, item.mode, item.stylePreset]);
}

function mergePromptHistory(primary: unknown, secondary: unknown): unknown[] {
  const byKey = new Map<string, Obj>();
  const all = [
    ...(Array.isArray(primary) ? primary : []),
    ...(Array.isArray(secondary) ? secondary : []),
  ].filter((e): e is Obj => isObj(e) && typeof e.id === "string");
  for (const entry of all) {
    const key = historyKey(entry);
    const seen = byKey.get(key);
    if (!seen) {
      byKey.set(key, entry);
      continue;
    }
    const newer = num(entry.createdAt) > num(seen.createdAt) ? entry : seen;
    byKey.set(key, { ...newer, favorite: !!seen.favorite || !!entry.favorite });
  }
  const merged = [...byKey.values()].sort((x, y) => num(y.createdAt) - num(x.createdAt));
  if (merged.length <= MERGED_PROMPT_HISTORY_MAX) return merged;
  // Over the cap: favourites are kept first, then the newest of the rest.
  const favourites = merged.filter((e) => e.favorite);
  const rest = merged.filter((e) => !e.favorite);
  const keep = new Set([
    ...favourites,
    ...rest.slice(0, Math.max(0, MERGED_PROMPT_HISTORY_MAX - favourites.length)),
  ]);
  return merged.filter((e) => keep.has(e));
}

/** `{ <listKey>: [...] , ...singles }` where the list merges and singles fill. */
function mergeWithList(primary: unknown, secondary: unknown, listKey: string): unknown {
  if (!isObj(primary)) return secondary ?? primary;
  if (!isObj(secondary)) return primary;
  return { ...secondary, ...primary, [listKey]: mergeList(primary[listKey], secondary[listKey]) };
}

function mergeArtistFavourites(primary: unknown, secondary: unknown): unknown {
  if (!isObj(primary)) return secondary ?? primary;
  if (!isObj(secondary)) return primary;
  const categories = mergeList(primary.categories, [], (c) => (typeof c.id === "string" ? c.id : null)) as Obj[];
  // A category made on both sides has two ids; match it by name instead and
  // point the other side's favourites at the surviving id.
  const remap = new Map<string, string>();
  const byName = new Map(categories.map((c) => [String(c.name ?? "").trim().toLowerCase(), c]));
  const byId = new Set(categories.map((c) => c.id));
  for (const cat of Array.isArray(secondary.categories) ? secondary.categories : []) {
    if (!isObj(cat) || typeof cat.id !== "string") continue;
    if (byId.has(cat.id)) continue;
    const same = byName.get(String(cat.name ?? "").trim().toLowerCase());
    if (same) {
      remap.set(cat.id, same.id as string);
      continue;
    }
    categories.push(cat);
    byId.add(cat.id);
  }
  const theirs = (Array.isArray(secondary.favourites) ? secondary.favourites : []).map((f) =>
    isObj(f) && typeof f.categoryId === "string" && remap.has(f.categoryId)
      ? { ...f, categoryId: remap.get(f.categoryId) }
      : f,
  );
  // Same artist favourited on both sides: `mergeList` keeps the newer entry;
  // if only the other one has a category, the category carries over.
  const favourites = mergeList(primary.favourites, theirs, (f) => (typeof f.slug === "string" ? f.slug : null)).map(
    (f) => {
      if (!isObj(f) || f.categoryId) return f;
      const other = [...(Array.isArray(primary.favourites) ? primary.favourites : []), ...theirs].find(
        (o) => isObj(o) && o.slug === f.slug && o.categoryId,
      ) as Obj | undefined;
      return other ? { ...f, categoryId: other.categoryId } : f;
    },
  );
  return { ...secondary, ...primary, categories, favourites };
}

function mergeGallery(primary: unknown, secondary: unknown): unknown {
  if (!isObj(primary)) return secondary ?? primary;
  if (!isObj(secondary)) return primary;
  const pn = isObj(primary.nsfw) ? primary.nsfw : undefined;
  const sn = isObj(secondary.nsfw) ? secondary.nsfw : undefined;
  return {
    ...secondary,
    ...primary,
    boardAssignments: mergeRecord(primary.boardAssignments, secondary.boardAssignments),
    customBoards: mergeList(primary.customBoards, secondary.customBoards),
    favourites: mergeRecord(primary.favourites, secondary.favourites),
    ratings: mergeRecord(primary.ratings, secondary.ratings),
    nsfw:
      pn && sn
        ? {
            ...sn,
            ...pn,
            marks: mergeRecord(pn.marks, sn.marks),
            keywords: mergeList(pn.keywords, sn.keywords),
          }
        : (pn ?? sn),
  };
}

function mergeEnhancerSessions(primary: unknown, secondary: unknown): unknown {
  if (!isObj(primary)) return secondary ?? primary;
  if (!isObj(secondary)) return primary;
  const out: Obj = { ...secondary, ...primary };
  for (const flow of new Set([...Object.keys(primary), ...Object.keys(secondary)])) {
    const turns = mergeList(primary[flow], secondary[flow], (t) => `${num(t.at)}:${String(t.user ?? "")}`) as Obj[];
    out[flow] = turns.sort((x, y) => num(x.at) - num(y.at)).slice(-MERGED_SESSION_TURNS_MAX);
  }
  return out;
}

function mergeArtistLocalPreviews(primary: unknown, secondary: unknown): unknown {
  if (!isObj(primary)) return secondary ?? primary;
  if (!isObj(secondary)) return primary;
  const out: Obj = { ...secondary };
  for (const [slug, slot] of Object.entries(primary)) out[slug] = fill(slot, secondary[slug]);
  return out;
}

function mergeStyleCreator(primary: unknown, secondary: unknown): unknown {
  if (!isObj(primary)) return secondary ?? primary;
  if (!isObj(secondary)) return primary;
  const history = mergeList(primary.history, secondary.history);
  return { ...secondary, ...primary, history: history.slice(-MERGED_STYLE_HISTORY_MAX) };
}

/**
 * Merge two snapshots of the same user's data (see the module docs). Either
 * side may be missing categories; the result has every category either had.
 */
export function mergeUserPrefs(primary: UserPrefsData, secondary: UserPrefsData): UserPrefsData {
  const p = primary as Obj;
  const s = secondary as Obj;
  const pick = <K extends keyof UserPrefsData>(key: K, merge: (a: unknown, b: unknown) => unknown) => {
    const a = p[key];
    const b = s[key];
    if (a === undefined || a === null) return b;
    if (b === undefined || b === null) return a;
    return merge(a, b);
  };
  const merged: Obj = {
    ...s,
    ...p,
    generation: pick("generation", fill),
    prompt_history: pick("prompt_history", mergePromptHistory),
    prompt_presets: pick("prompt_presets", (a, b) => mergeWithList(a, b, "presets")),
    styles: pick("styles", (a, b) => mergeWithList(a, b, "styles")),
    lora_presets: pick("lora_presets", (a, b) => mergeWithList(a, b, "presets")),
    artist_favourites: pick("artist_favourites", mergeArtistFavourites),
    gallery_boards: pick("gallery_boards", mergeGallery),
    autocomplete: pick("autocomplete", fill),
    accessibility: pick("accessibility", fill),
    notes: pick("notes", mergeNotes),
    characters: pick("characters", (a, b) => mergeWithList(a, b, "characters")),
    video_timeline: pick("video_timeline", fill),
    locale: pick("locale", (a) => a),
    artist_local_previews: pick("artist_local_previews", mergeArtistLocalPreviews),
    enhancer_sessions: pick("enhancer_sessions", mergeEnhancerSessions),
    style_creator: pick("style_creator", mergeStyleCreator),
    anime_scene: pick("anime_scene", fill),
  };
  for (const key of Object.keys(merged)) if (merged[key] === undefined) delete merged[key];
  return merged as UserPrefsData;
}
