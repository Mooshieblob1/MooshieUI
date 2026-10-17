/**
 * Sorting and grouping for the favourite artists list (bottom panel Artists tab).
 *
 * Pure helpers: the component owns the persisted choice and the labels.
 */
import { rankingPostCount } from "./counts.js";
import type { FavouriteCategory, FavouriteEntry } from "./favourites.svelte.js";
import type { ArtistSearchHit } from "./types.js";

export type FavouriteSort =
  | "added_desc"
  | "added_asc"
  | "name_asc"
  | "name_desc"
  | "posts_desc"
  | "posts_asc";

export type FavouriteGroup = "none" | "category" | "letter";

export const FAVOURITE_SORTS: readonly FavouriteSort[] = [
  "added_desc",
  "added_asc",
  "name_asc",
  "name_desc",
  "posts_desc",
  "posts_asc",
];

export const FAVOURITE_GROUPS: readonly FavouriteGroup[] = ["none", "category", "letter"];

/** Group key used for favourites with no category. */
export const UNCATEGORISED_GROUP = "__uncat";
/** Letter group key for slugs that don't start with a letter. */
export const OTHER_LETTER_GROUP = "#";

export interface FavouriteHitGroup {
  /** Category id, `UNCATEGORISED_GROUP`, a capital letter, or `OTHER_LETTER_GROUP`. */
  key: string;
  /** Set for category groups only. */
  category: FavouriteCategory | null;
  hits: ArtistSearchHit[];
}

export function sortFavouriteHits(
  hits: readonly ArtistSearchHit[],
  favourites: Record<string, FavouriteEntry>,
  sort: FavouriteSort,
): ArtistSearchHit[] {
  const addedAt = (hit: ArtistSearchHit) => favourites[hit.slug]?.addedAt ?? 0;
  const byName = (a: ArtistSearchHit, b: ArtistSearchHit) => a.slug.localeCompare(b.slug);
  const compare = (a: ArtistSearchHit, b: ArtistSearchHit): number => {
    switch (sort) {
      case "added_asc":
        return addedAt(a) - addedAt(b);
      case "name_asc":
        return byName(a, b);
      case "name_desc":
        return byName(b, a);
      case "posts_desc":
        return rankingPostCount(b) - rankingPostCount(a);
      case "posts_asc":
        return rankingPostCount(a) - rankingPostCount(b);
      case "added_desc":
      default:
        return addedAt(b) - addedAt(a);
    }
  };
  // Ties (equal post counts, same-millisecond imports) fall back to name.
  return [...hits].sort((a, b) => compare(a, b) || byName(a, b));
}

export function letterGroupOf(slug: string): string {
  // NFD splits accents off, so "ämi" files under A rather than its own Ä.
  const first = slug.normalize("NFD").charAt(0).toLocaleUpperCase();
  return /\p{L}/u.test(first) ? first : OTHER_LETTER_GROUP;
}

/**
 * Split already-sorted hits into groups, keeping each group's order.
 * Empty groups are dropped. Category groups follow the user's category order
 * with uncategorised last; letter groups run alphabetically (reversed for a
 * Z-A name sort) with non-letters last.
 */
export function groupFavouriteHits(
  hits: readonly ArtistSearchHit[],
  mode: FavouriteGroup,
  favourites: Record<string, FavouriteEntry>,
  categories: readonly FavouriteCategory[],
  sort: FavouriteSort,
): FavouriteHitGroup[] {
  if (mode === "none") return [{ key: "all", category: null, hits: [...hits] }];

  const buckets = new Map<string, ArtistSearchHit[]>();
  const push = (key: string, hit: ArtistSearchHit) => {
    const bucket = buckets.get(key);
    if (bucket) bucket.push(hit);
    else buckets.set(key, [hit]);
  };

  if (mode === "category") {
    const known = new Set(categories.map((c) => c.id));
    for (const hit of hits) {
      const id = favourites[hit.slug]?.categoryId;
      push(id && known.has(id) ? id : UNCATEGORISED_GROUP, hit);
    }
    const groups: FavouriteHitGroup[] = [];
    for (const cat of categories) {
      const bucket = buckets.get(cat.id);
      if (bucket) groups.push({ key: cat.id, category: cat, hits: bucket });
    }
    const uncat = buckets.get(UNCATEGORISED_GROUP);
    if (uncat) groups.push({ key: UNCATEGORISED_GROUP, category: null, hits: uncat });
    return groups;
  }

  for (const hit of hits) push(letterGroupOf(hit.slug), hit);
  const dir = sort === "name_desc" ? -1 : 1;
  const keys = [...buckets.keys()].sort((a, b) => {
    if (a === OTHER_LETTER_GROUP) return 1;
    if (b === OTHER_LETTER_GROUP) return -1;
    return a.localeCompare(b) * dir;
  });
  return keys.map((key) => ({ key, category: null, hits: buckets.get(key)! }));
}
