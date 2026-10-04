import type { ArtistManifest, ArtistSearchHit } from "./types.js";

/** Variants the CDN ships for this artist. Cheap; safe to call over a whole index. */
export function cdnVariantCountOf(hit: ArtistSearchHit): number {
  return Math.max(1, hit.variantCount ?? hit.images?.length ?? 1);
}

/** Image id of one (1-based) variant of an artist's CDN preview. */
export function imageIdForVariant(hit: ArtistSearchHit, variant: number): string {
  // Prefer an explicit per-variant entry when the shard data is present.
  const img = hit.images?.[variant - 1];
  if (img?.imageId) return img.imageId;
  // search.json hits carry no `images[]`, so derive the variant id by
  // swapping the `-p<n>` suffix (all v2 imageIds end in `-p1`/`-p2`).
  return hit.imageId.replace(/-p\d+$/, `-p${variant}`);
}

/** Image extension for a dataset: AVIF in index v2+, WebP in v1. */
export function imageExtOf(manifest: ArtistManifest | null): string {
  return (manifest?.version ?? 1) >= 2 ? "avif" : "webp";
}
