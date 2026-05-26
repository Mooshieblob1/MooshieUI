import { ANIMADEX_ORIGIN, animadexFetch } from "./fetch.js";
import type {
  AnimadexCharacter,
  CharacterFilterFacetName,
  CharacterFacetsResponse,
  CharacterSearchParams,
  CharacterSearchResponse,
  FacetSearchResponse,
} from "./types.js";
import { CHARACTER_FILTER_FACETS } from "./types.js";

function apiUrl(path: string): string {
  return `${ANIMADEX_ORIGIN}api/characters${path}`;
}

const SEARCH_MIN_POST_COUNT = 50;
const RESPONSE_CACHE_TTL_MS = 45_000;
const responseCache = new Map<string, { fetchedAt: number; data: unknown }>();

class AnimadexHttpError extends Error {
  status: number;
  constructor(status: number, url: string) {
    super(`animadex: ${url} returned ${status}`);
    this.status = status;
  }
}

function cacheKey(url: string): string {
  return url;
}

function getCached<T>(url: string): T | null {
  const entry = responseCache.get(cacheKey(url));
  if (!entry) return null;
  if (Date.now() - entry.fetchedAt > RESPONSE_CACHE_TTL_MS) {
    responseCache.delete(cacheKey(url));
    return null;
  }
  return entry.data as T;
}

function setCached<T>(url: string, data: T): void {
  responseCache.set(cacheKey(url), { fetchedAt: Date.now(), data });
}

async function sleepWithAbort(ms: number, signal?: AbortSignal): Promise<void> {
  if (!signal) {
    await new Promise((resolve) => globalThis.setTimeout(resolve, ms));
    return;
  }
  if (signal.aborted) {
    throw new DOMException("Aborted", "AbortError");
  }
  await new Promise<void>((resolve, reject) => {
    const timer = globalThis.setTimeout(() => {
      signal.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    const onAbort = () => {
      globalThis.clearTimeout(timer);
      reject(new DOMException("Aborted", "AbortError"));
    };
    signal.addEventListener("abort", onAbort, { once: true });
  });
}

async function fetchJson<T>(url: string, signal?: AbortSignal): Promise<T> {
  const cached = getCached<T>(url);
  if (cached) return cached;

  const fetchFn = animadexFetch ?? globalThis.fetch.bind(globalThis);
  let res = await fetchFn(url, { credentials: "omit", signal });
  if (res.status === 429) {
    const stale = getCached<T>(url);
    if (stale) return stale;
    // Single delayed retry helps smooth over brief upstream rate spikes.
    await sleepWithAbort(700, signal);
    res = await fetchFn(url, { credentials: "omit", signal });
  }
  if (!res.ok) {
    throw new AnimadexHttpError(res.status, url);
  }
  const data = (await res.json()) as T;
  setCached(url, data);
  return data;
}

function normalizeRomanizedName(input: string): string {
  return input
    .toLowerCase()
    .replace(/[()'".,!?-]/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    // Common romaji long-vowel variants (e.g. mutou -> muto).
    .replace(/ou/g, "o")
    .replace(/aa/g, "a")
    .replace(/ii/g, "i")
    .replace(/uu/g, "u")
    .replace(/ee/g, "e")
    .replace(/\s/g, "");
}

function normalizedCharacterKey(c: AnimadexCharacter): string {
  const primary = c.trigger.split(",")[0]?.trim() || c.name;
  return `${normalizeRomanizedName(primary)}::${normalizeRomanizedName(c.copyright)}`;
}

function shouldReplaceCharacter(next: AnimadexCharacter, prev: AnimadexCharacter): boolean {
  if (next.count !== prev.count) return next.count > prev.count;
  if (next.loras.length !== prev.loras.length) return next.loras.length > prev.loras.length;
  if (next.has_image !== prev.has_image) return next.has_image;
  return next.slug.length < prev.slug.length;
}

function normalizeSearchResults(results: AnimadexCharacter[]): AnimadexCharacter[] {
  const deduped = new Map<string, AnimadexCharacter>();
  for (const row of results) {
    if ((row.count ?? 0) < SEARCH_MIN_POST_COUNT) continue;
    const key = normalizedCharacterKey(row);
    const existing = deduped.get(key);
    if (!existing || shouldReplaceCharacter(row, existing)) {
      deduped.set(key, row);
    }
  }
  return [...deduped.values()];
}

function buildSearchParams(params: CharacterSearchParams): string {
  const p = new URLSearchParams();
  if (params.q?.trim()) p.set("q", params.q.trim());
  p.set("sort", params.sort ?? "count");
  if (params.sort === "random" && params.seed) {
    p.set("seed", String(params.seed));
  }
  p.set("page", String(Math.max(1, params.page ?? 1)));
  for (const facet of CHARACTER_FILTER_FACETS) {
    for (const v of params.filters?.[facet] ?? []) {
      p.append(facet, v);
    }
  }
  if (params.lorasOnly) p.set("loras", "1");
  return p.toString();
}

export async function searchCharacters(
  params: CharacterSearchParams,
  signal?: AbortSignal,
): Promise<CharacterSearchResponse> {
  const qs = buildSearchParams(params);
  const data = await fetchJson<CharacterSearchResponse>(apiUrl(`/search?${qs}`), signal);
  const normalizedResults = normalizeSearchResults(data.results);
  return {
    ...data,
    results: normalizedResults,
  };
}

export async function loadCharacterFacets(): Promise<CharacterFacetsResponse> {
  return fetchJson<CharacterFacetsResponse>(apiUrl("/facets"));
}

export async function searchCharacterFacet(
  facet: CharacterFilterFacetName,
  query: string,
  signal?: AbortSignal,
): Promise<FacetSearchResponse> {
  const q = encodeURIComponent(query.trim());
  return fetchJson<FacetSearchResponse>(apiUrl(`/facet/${facet}?q=${q}`), signal);
}
