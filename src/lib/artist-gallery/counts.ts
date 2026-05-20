import { locale } from "../stores/locale.svelte.js";
import type { ArtistEntry, ArtistSearchHit } from "./types.js";

type ArtistCountSource = Pick<ArtistEntry | ArtistSearchHit, "postCount" | "belowThreshold" | "b">;

export function isBelowThresholdCount(entry: ArtistCountSource): boolean {
  return entry.belowThreshold === true || entry.b === 1 || entry.b === true || entry.postCount <= 50;
}

export function rankingPostCount(entry: ArtistCountSource): number {
  return isBelowThresholdCount(entry) ? 50 : entry.postCount;
}

export function formatPostCount(entry: ArtistCountSource, compact = true): string {
  if (isBelowThresholdCount(entry)) return "≤50";

  const count = Math.max(0, Math.floor(entry.postCount));
  if (!compact) return locale.formatInteger(count);
  if (count >= 1_000_000) return `${locale.formatDecimalTrimmed(count / 1_000_000, 1)}M`;
  if (count >= 1_000) return `${locale.formatDecimalTrimmed(count / 1_000, 0)}k`;
  return locale.formatInteger(count);
}