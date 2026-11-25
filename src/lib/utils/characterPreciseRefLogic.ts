/**
 * Pure helpers for per-character Precise Reference selection, normalization,
 * and prompt-to-character matching.
 *
 * Leaf util with zero store imports so `scripts/tests/characterPreciseRef.test.ts`
 * can bundle and run it in Node.
 */
import { namesCharacter } from "./contentSafety.js";
import { detectCharacters, prepareCards, type DetectCard } from "./galleryCharacters.js";
import {
  NAI_REF_CHARACTER,
  NAI_REF_CHARACTER_MAIN_OUTFIT,
  NAI_REF_CHARACTER_STYLE,
  NAI_REF_CHARACTER_STYLE_ANY_OUTFIT,
  NAI_REF_OUTFIT,
  NAI_REF_STYLE,
} from "./novelaiModels.js";

/** Structural slice of a saved Precise Reference entry on a character card. */
export interface CharacterPreciseReferenceData {
  id: string;
  thumbnail: string;
  reference: string;
  description?: string;
  strength?: number;
  fidelity?: number;
}

/** Structural slice of a character card read by Precise Reference helpers. */
export interface CharacterWithPreciseReferences {
  thumbnail: string | null;
  reference: string | null;
  preciseReferences?: readonly CharacterPreciseReferenceData[];
}

const VALID_DESCRIPTIONS = new Set<string>([
  NAI_REF_CHARACTER,
  NAI_REF_CHARACTER_MAIN_OUTFIT,
  NAI_REF_STYLE,
  NAI_REF_CHARACTER_STYLE,
  NAI_REF_CHARACTER_STYLE_ANY_OUTFIT,
  NAI_REF_OUTFIT,
]);

/**
 * Effective Precise Reference list for a character card: its explicit
 * `preciseReferences` when populated, or a single fallback entry from its
 * card picture (`reference ?? thumbnail`) when not.
 */
export function effectiveCharacterPreciseReferences(
  card: CharacterWithPreciseReferences,
): CharacterPreciseReferenceData[] {
  if (card.preciseReferences && card.preciseReferences.length > 0) {
    return card.preciseReferences.map((r) => ({ ...r }));
  }
  const image = card.reference ?? card.thumbnail;
  const thumbnail = card.thumbnail ?? card.reference;
  if (!image || !thumbnail) return [];
  return [
    {
      id: "__card__",
      thumbnail,
      reference: image,
      description: NAI_REF_CHARACTER,
      strength: 1.0,
      fidelity: 1.0,
    },
  ];
}

/** Map a stored Precise Reference description to one supported by the active model. */
export function normalizeCharacterReferenceDescription(
  description: string | undefined,
  opts: { supportsOutfits: boolean; supportsSameOutfit: boolean },
): string {
  const clean = typeof description === "string" && VALID_DESCRIPTIONS.has(description)
    ? description
    : NAI_REF_CHARACTER;
  if (clean === NAI_REF_OUTFIT && !opts.supportsOutfits) {
    return NAI_REF_CHARACTER;
  }
  if (!opts.supportsSameOutfit) {
    if (clean === NAI_REF_CHARACTER_MAIN_OUTFIT) return NAI_REF_CHARACTER;
    if (clean === NAI_REF_CHARACTER_STYLE_ANY_OUTFIT) return NAI_REF_CHARACTER_STYLE;
  }
  return clean;
}

/** Clamp stored strength onto `[0, strengthMax]`, falling back to `1.0`. */
export function clampCharacterReferenceStrength(
  strength: number | undefined,
  strengthMax: number,
): number {
  const ceiling = Number.isFinite(strengthMax) && strengthMax >= 0 ? strengthMax : 1;
  const raw = typeof strength === "number" && Number.isFinite(strength) ? strength : 1.0;
  return Math.max(0, Math.min(ceiling, raw));
}

/** Clamp stored fidelity onto `[0, 1]`, falling back to `1.0`. */
export function clampCharacterReferenceFidelity(fidelity: number | undefined): number {
  const raw = typeof fidelity === "number" && Number.isFinite(fidelity) ? fidelity : 1.0;
  return Math.max(0, Math.min(1, raw));
}

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function normalizeMatchText(text: string): string {
  return text.normalize("NFKC").toLowerCase().replace(/_/g, " ");
}

function stripVariantQualifier(name: string): { bare: string; qualifier: string } {
  const match = /^(.+?)\s*\(([^)]+)\)\s*$/.exec(name.trim());
  if (!match) return { bare: name.trim(), qualifier: "" };
  return { bare: match[1].trim(), qualifier: match[2].trim() };
}

function containsWholePhrase(text: string, phrase: string): boolean {
  const cleanPhrase = normalizeMatchText(phrase).trim();
  if (!cleanPhrase) return false;
  const cleanText = normalizeMatchText(text);
  // Non-Latin scripts (CJK, Kana, Hangul, etc.) often omit spaces between words.
  if (/[^\x00-\x7F]/u.test(cleanPhrase) && !/[a-z0-9]/i.test(cleanPhrase)) {
    return cleanText.includes(cleanPhrase);
  }
  if (cleanPhrase.length < 2) return false;
  const pattern = new RegExp(
    `(?<![\\p{L}\\p{N}])${escapeRegExp(cleanPhrase).replace(/\s+/g, "[\\s_-]+")}(?![\\p{L}\\p{N}])`,
    "iu",
  );
  return pattern.test(cleanText);
}

function explicitNameTier(
  card: DetectCard,
  allTexts: readonly string[],
): 0 | 1 | 2 | 3 {
  const { bare, qualifier } = stripVariantQualifier(card.name);
  const nameMatched = allTexts.some(
    (t) => namesCharacter(card.name, t) || (bare.length > 0 && containsWholePhrase(t, bare)),
  );
  if (!nameMatched) return 0;
  if (qualifier && allTexts.some((t) => containsWholePhrase(t, qualifier))) {
    return 3;
  }
  return card.parentId === null ? 2 : 1;
}

/**
 * Find which saved character cards appear in a prompt-rewrite exchange.
 *
 * Matches explicit character names in the user request or generated answer
 * (preferring a variant when its bracketed qualifier also appears, otherwise the
 * base character card), plus tag/identity matches in the generated prompt or
 * character boxes for follow-up turns that omit the name. Deduplicates per
 * character family.
 */
export function matchCharacterCardsInText<T extends DetectCard>(
  cards: readonly T[],
  requestTexts: readonly string[],
  output: { base: string; characters?: readonly string[] },
): T[] {
  if (cards.length === 0) return [];
  const cardById = new Map(cards.map((c) => [c.id, c]));
  const familyOf = (c: T): string =>
    c.parentId && cardById.has(c.parentId) ? c.parentId : c.id;

  const outputTexts = [output.base, ...(output.characters ?? [])].filter((t) => t.trim() !== "");
  const allTexts = [...requestTexts, ...outputTexts].filter((t) => t.trim() !== "");

  // 1. Explicit name matches across request and output text.
  const bestByFamily = new Map<string, { card: T; score: number; index: number }>();
  cards.forEach((card, index) => {
    const tier = explicitNameTier(card, allTexts);
    if (tier === 0) return;
    const score = 10 + tier;
    const fam = familyOf(card);
    const current = bestByFamily.get(fam);
    if (!current || score > current.score) {
      bestByFamily.set(fam, { card, score, index });
    }
  });

  // 2. Tag / identity fallback from the generated output (helps unnamed follow-up edits).
  if (outputTexts.length > 0) {
    const prepared = prepareCards(cards);
    const nonEmptyChars = (output.characters ?? []).filter((p) => p.trim() !== "");
    const hits = detectCharacters(
      {
        positive_prompt: output.base,
        ...(nonEmptyChars.length > 0
          ? { mooshie_novelai_characters: JSON.stringify(nonEmptyChars.map((prompt) => ({ prompt }))) }
          : {}),
      },
      prepared,
    );
    for (const hit of hits) {
      const card = cardById.get(hit.cardId);
      if (!card) continue;
      const fam = familyOf(card);
      if (bestByFamily.has(fam)) continue;
      const index = cards.indexOf(card);
      bestByFamily.set(fam, { card, score: hit.score, index });
    }
  }

  return [...bestByFamily.values()]
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map((entry) => entry.card);
}
