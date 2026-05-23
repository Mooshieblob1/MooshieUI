/** Animadex characters API types (https://animadex.net/?mode=characters). */

/** Facets exposed in the characters filter sidebar (search already covers character names). */
export type CharacterFilterFacetName =
  | "copyright"
  | "hair_color"
  | "hair_length"
  | "eye_color"
  | "gender";

export const CHARACTER_FILTER_FACETS: CharacterFilterFacetName[] = [
  "copyright",
  "hair_color",
  "hair_length",
  "eye_color",
  "gender",
];

/** All facet keys returned by the Animadex API (includes character, unused in UI). */
export type CharacterFacetName = "character" | CharacterFilterFacetName;

export type CharacterSort = "count" | "az" | "random";

export interface AnimadexLora {
  id: number;
  name: string;
  url: string;
}

export interface AnimadexCharacter {
  slug: string;
  name: string;
  copyright: string;
  copyright_name: string;
  trigger: string;
  tags: string[];
  count: number;
  url: string;
  thumb_url: string;
  img_url: string;
  has_image: boolean;
  loras: AnimadexLora[];
}

export interface CharacterSearchResponse {
  total: number;
  page: number;
  page_size: number;
  pages: number;
  results: AnimadexCharacter[];
}

export interface FacetValue {
  value: string;
  label: string;
  count: number;
}

export interface FacetGroup {
  label: string;
  total: number;
  values: FacetValue[];
}

export interface CharacterFacetsResponse {
  total: number;
  facets: Partial<Record<CharacterFacetName, FacetGroup>>;
}

export interface FacetSearchResponse {
  label: string;
  total: number;
  values: FacetValue[];
}

export interface CharacterSearchParams {
  q?: string;
  sort?: CharacterSort;
  seed?: number | null;
  page?: number;
  filters?: Partial<Record<CharacterFilterFacetName, string[]>>;
  lorasOnly?: boolean;
}
