/**
 * Import converters for favourites exported by the Anima Style Explorer
 * website (the "anima style gallery" site). Two formats are supported:
 *
 * 1. JSON export (`anima-style-favorites-*.json`): favourites are stored as
 *    numeric artist ids plus optional folder assignments. The site's id→name
 *    table (`app/data.js`) is fetched to resolve the ids, then names are
 *    matched against MooshieUI's artist search index to obtain slugs.
 * 2. TXT export (`anima-favorites-*.txt`): one artist name per line.
 *
 * Both converters produce a native `FavouritesExport` payload that can be fed
 * straight into `artistFavourites.importJSON()`.
 */

import type { ArtistGalleryClient } from "./types.js";
import {
  CATEGORY_COLOR_PALETTE,
  EXPORT_KIND,
  EXPORT_VERSION,
  type FavouriteCategory,
  type FavouriteEntry,
  type FavouritesExport,
} from "./favourites.svelte.js";

/** Known hosts of the explorer's id→name table, tried in order. */
const EXPLORER_DATA_URLS = [
  "https://thetacursed.github.io/Anima-Style-Explorer/app/data.js",
  "https://raw.githubusercontent.com/Mooshieblob1/Blob-Anima-Style-Explorer/main/app/data.js",
];

interface ExplorerFavorite {
  id: string | number;
  timestamp?: number;
}

interface ExplorerFolder {
  id: string;
  name: string;
}

export interface AnimaExplorerExport {
  metadata?: { appName?: string };
  favorites?: ExplorerFavorite[];
  folderData?: {
    folders?: ExplorerFolder[];
    folderArtists?: Record<string, Array<{ id: string | number; added?: number }>>;
  } | null;
}

export interface ExternalImportResult {
  payload: FavouritesExport;
  matched: number;
  /** Artist names (or raw ids) that could not be matched to a known slug. */
  unmatched: string[];
}

/** Detect the Anima Style Explorer JSON export. Returns null for anything else. */
export function parseAnimaExplorerExport(raw: string): AnimaExplorerExport | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
  const obj = parsed as AnimaExplorerExport & { kind?: unknown };
  // Native MooshieUI exports carry a `kind` discriminator.
  if (typeof obj.kind === "string") return null;
  if (!Array.isArray(obj.favorites)) return null;
  const appName = obj.metadata?.appName ?? "";
  const idBasedFavourites = obj.favorites.every(
    (f) => !!f && (typeof f.id === "string" || typeof f.id === "number"),
  );
  if (!/anima style explorer/i.test(appName) && !idBasedFavourites) return null;
  return obj;
}

/** Detect a plain-text artist name list (the site's TXT export). */
export function parseArtistNameList(raw: string): string[] | null {
  if (!raw.trim()) return null;
  try {
    JSON.parse(raw);
    return null; // valid JSON is handled elsewhere
  } catch {
    // Not JSON: treat as a newline-separated name list.
  }
  const lines = raw
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean);
  return lines.length > 0 ? lines : null;
}

/**
 * Normalize an artist name so site names ("ruu \(tksymkw\)") and MooshieUI
 * tags ("@ruu_(tksymkw)") compare equal: strip escapes, spaces → underscores.
 */
function normalizeArtistName(name: string): string {
  return name
    .trim()
    .toLowerCase()
    .replace(/\\/g, "")
    .replace(/\s+/g, "_");
}

async function buildNameToSlugMap(client: ArtistGalleryClient): Promise<Map<string, string>> {
  const hits = await client.loadSearchIndex();
  const map = new Map<string, string>();
  for (const hit of hits) {
    const tag = hit.tag.startsWith("@") ? hit.tag.slice(1) : hit.tag;
    map.set(normalizeArtistName(tag), hit.slug);
    // Accept slugs directly as well (slugs strip punctuation differently).
    if (!map.has(hit.slug)) map.set(hit.slug, hit.slug);
  }
  return map;
}

/** Fetch the explorer site's `data.js` and build an id → artist-name map. */
async function fetchExplorerIdMap(fetchImpl: typeof fetch = fetch): Promise<Map<string, string>> {
  let lastError: unknown = null;
  for (const url of EXPLORER_DATA_URLS) {
    try {
      const res = await fetchImpl(url);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const text = await res.text();
      // data.js is `const galleryData = [ ... ];` — extract the array literal.
      const start = text.indexOf("[");
      const end = text.lastIndexOf("]");
      if (start === -1 || end === -1 || end <= start) throw new Error("Unexpected data.js shape");
      const items = JSON.parse(text.slice(start, end + 1)) as Array<{
        id?: string | number;
        name?: string;
      }>;
      const map = new Map<string, string>();
      for (const item of items) {
        if (item && item.id != null && typeof item.name === "string") {
          map.set(String(item.id), item.name);
        }
      }
      if (map.size > 0) return map;
    } catch (e) {
      lastError = e;
    }
  }
  throw new Error(
    `Could not download the Anima Style Explorer artist table: ${
      lastError instanceof Error ? lastError.message : String(lastError)
    }`,
  );
}

/** Convert the site's JSON export (id-based, with folders) to a native payload. */
export async function convertAnimaExplorerExport(
  parsed: AnimaExplorerExport,
  client: ArtistGalleryClient,
  fetchImpl: typeof fetch = fetch,
): Promise<ExternalImportResult> {
  const [idToName, nameToSlug] = await Promise.all([
    fetchExplorerIdMap(fetchImpl),
    buildNameToSlugMap(client),
  ]);

  // Site folders become favourite categories.
  const categories: FavouriteCategory[] = [];
  const folders = parsed.folderData?.folders ?? [];
  folders.forEach((folder, i) => {
    if (!folder || typeof folder.id !== "string" || typeof folder.name !== "string") return;
    categories.push({
      id: folder.id,
      name: folder.name,
      color: CATEGORY_COLOR_PALETTE[i % CATEGORY_COLOR_PALETTE.length],
    });
  });
  const catIds = new Set(categories.map((c) => c.id));

  // Explorer artist id → folder/category id.
  const idCategory = new Map<string, string>();
  const folderArtists = parsed.folderData?.folderArtists ?? {};
  for (const [folderId, entries] of Object.entries(folderArtists)) {
    if (!catIds.has(folderId) || !Array.isArray(entries)) continue;
    for (const entry of entries) {
      if (entry && entry.id != null) idCategory.set(String(entry.id), folderId);
    }
  }

  const favourites: FavouriteEntry[] = [];
  const unmatched: string[] = [];
  for (const fav of parsed.favorites ?? []) {
    if (!fav || fav.id == null) continue;
    const id = String(fav.id);
    const name = idToName.get(id);
    const slug = name ? nameToSlug.get(normalizeArtistName(name)) : undefined;
    if (!slug) {
      unmatched.push(name ?? id);
      continue;
    }
    favourites.push({
      slug,
      categoryId: idCategory.get(id) ?? null,
      addedAt: typeof fav.timestamp === "number" && fav.timestamp > 0 ? fav.timestamp : Date.now(),
    });
  }

  return {
    payload: {
      kind: EXPORT_KIND,
      version: EXPORT_VERSION,
      exportedAt: new Date().toISOString(),
      favourites,
      categories,
    },
    matched: favourites.length,
    unmatched,
  };
}

/** Convert a plain artist-name list (TXT export) to a native payload. */
export async function convertArtistNameList(
  names: string[],
  client: ArtistGalleryClient,
): Promise<ExternalImportResult> {
  const nameToSlug = await buildNameToSlugMap(client);
  const favourites: FavouriteEntry[] = [];
  const unmatched: string[] = [];
  const seen = new Set<string>();
  for (const name of names) {
    const slug = nameToSlug.get(normalizeArtistName(name));
    if (!slug) {
      unmatched.push(name);
      continue;
    }
    if (seen.has(slug)) continue;
    seen.add(slug);
    favourites.push({ slug, categoryId: null, addedAt: Date.now() });
  }

  return {
    payload: {
      kind: EXPORT_KIND,
      version: EXPORT_VERSION,
      exportedAt: new Date().toISOString(),
      favourites,
      categories: [],
    },
    matched: favourites.length,
    unmatched,
  };
}
