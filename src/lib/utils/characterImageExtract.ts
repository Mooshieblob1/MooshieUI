/**
 * Pure helpers for preparing character extraction inputs from image metadata,
 * interrogator tags, and VLM descriptions, plus duplicate partitioning.
 */
import type { InterrogationResult } from "../types/index.js";
import type { ExtractedCharacter } from "./characterExtract.js";
import { findDuplicate, sameContent, type DuplicateCandidate } from "./characterDuplicates.js";

export type ExtractSourceKind =
  | "prompt"
  | "combined"
  | "metadata"
  | "interrogator"
  | "describe";

export interface ImageExtractSource {
  prompt: string;
  negative: string;
  kind: ExtractSourceKind;
  boxes?: { prompt: string; negative: string }[];
}

export interface CheckedInterrogatorTags {
  character: Record<string, boolean>;
  general: Record<string, boolean>;
  copyright?: Record<string, boolean>;
}

/**
 * Convert Interrogator results into an extraction prompt without artist or
 * rating tags. When `checked` is omitted, uses all character and general tags.
 */
export function tagsToExtractSource(
  result: InterrogationResult,
  checked?: CheckedInterrogatorTags,
): ImageExtractSource | null {
  const seen = new Set<string>();
  const tags: string[] = [];
  const pushTags = (list: readonly { name: string }[], filter?: Record<string, boolean>) => {
    for (const tag of list) {
      if (filter && !filter[tag.name]) continue;
      const formatted = tag.name.replace(/_/g, " ").trim();
      if (!formatted) continue;
      const key = formatted.toLowerCase();
      if (seen.has(key)) continue;
      seen.add(key);
      tags.push(formatted);
    }
  };

  pushTags(result.character_tags, checked?.character);
  if (checked?.copyright) pushTags(result.copyright_tags, checked.copyright);
  pushTags(result.general_tags, checked?.general);

  if (tags.length === 0) return null;
  return {
    prompt: tags.join(", "),
    negative: "",
    kind: "interrogator",
  };
}

function parseMetaCharacterBoxes(
  raw: string | undefined,
): { prompt: string; negative: string }[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    const boxes: { prompt: string; negative: string }[] = [];
    for (const entry of parsed) {
      if (!entry || typeof entry !== "object") continue;
      const item = entry as {
        prompt?: unknown;
        negative_prompt?: unknown;
        enabled?: unknown;
      };
      if (item.enabled === false) continue;
      const prompt = typeof item.prompt === "string" ? item.prompt.trim() : "";
      if (!prompt) continue;
      const negative =
        typeof item.negative_prompt === "string" ? item.negative_prompt.trim() : "";
      boxes.push({ prompt, negative });
    }
    return boxes;
  } catch {
    return [];
  }
}

/**
 * Build a character extraction source from embedded image metadata (`readImageMetadata*`
 * / `readPngMetadataClientSide`). Returns null when neither positive prompt nor active
 * NovelAI character boxes are present.
 */
export function metadataToExtractSource(
  meta: Record<string, string> | null | undefined,
): ImageExtractSource | null {
  if (!meta) return null;
  const basePositive = (meta.positive_prompt ?? "").trim();
  const baseNegative = (meta.negative_prompt ?? "").trim();
  const boxes = parseMetaCharacterBoxes(meta.mooshie_novelai_characters);

  if (!basePositive && boxes.length === 0) return null;

  const posParts = [basePositive];
  const negParts = [baseNegative];
  boxes.forEach((box, i) => {
    posParts.push(`Character ${i + 1}: ${box.prompt}`);
    if (box.negative) negParts.push(`Character ${i + 1}: ${box.negative}`);
  });

  return {
    prompt: posParts.filter(Boolean).join("\n"),
    negative: negParts.filter(Boolean).join("\n"),
    kind: "metadata",
    ...(boxes.length > 0 ? { boxes } : {}),
  };
}

/**
 * Sort extracted characters into new cards (`fresh`) and cards that match an
 * already saved character with differing content (`matches`), omitting exact duplicates.
 */
export function partitionExtracted<T extends DuplicateCandidate>(
  extracted: readonly ExtractedCharacter[],
  saved: readonly T[],
): { matches: { item: ExtractedCharacter; saved: T }[]; fresh: ExtractedCharacter[] } {
  const savedList = [...saved];
  const fresh: ExtractedCharacter[] = [];
  const matches: { item: ExtractedCharacter; saved: T }[] = [];
  for (const item of extracted) {
    const dup = findDuplicate(item, savedList);
    if (!dup) {
      fresh.push(item);
    } else if (!sameContent(item, dup) && !matches.some((m) => m.saved.id === dup.id)) {
      matches.push({ item, saved: dup });
    }
  }
  return { matches, fresh };
}
