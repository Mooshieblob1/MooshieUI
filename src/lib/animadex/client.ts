import { ANIMADEX_ORIGIN, animadexFetch } from "./fetch.js";
import type {
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

async function fetchJson<T>(url: string): Promise<T> {
  const fetchFn = animadexFetch ?? globalThis.fetch.bind(globalThis);
  const res = await fetchFn(url, { credentials: "omit" });
  if (!res.ok) {
    throw new Error(`animadex: ${url} returned ${res.status}`);
  }
  return (await res.json()) as T;
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
): Promise<CharacterSearchResponse> {
  const qs = buildSearchParams(params);
  return fetchJson<CharacterSearchResponse>(apiUrl(`/search?${qs}`));
}

export async function loadCharacterFacets(): Promise<CharacterFacetsResponse> {
  return fetchJson<CharacterFacetsResponse>(apiUrl("/facets"));
}

export async function searchCharacterFacet(
  facet: CharacterFilterFacetName,
  query: string,
): Promise<FacetSearchResponse> {
  const q = encodeURIComponent(query.trim());
  return fetchJson<FacetSearchResponse>(apiUrl(`/facet/${facet}?q=${q}`));
}
