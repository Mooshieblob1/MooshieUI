/**
 * Pure helpers for per-character Precise Reference selection, normalization,
 * and prompt-to-character matching.
 *
 * Leaf util with zero store imports so `scripts/tests/characterPreciseRef.test.ts`
 * can bundle and run it in Node.
 */
import { namesCharacter } from "./contentSafety.js";
import { detectCharacters, prepareCards, type DetectCard } from "./galleryCharacters.js";
import { PRECISE_REFERENCE_COST } from "./novelaiCost.js";
import {
  NAI_REF_CHARACTER,
  NAI_REF_CHARACTER_MAIN_OUTFIT,
  NAI_REF_CHARACTER_STYLE,
  NAI_REF_CHARACTER_STYLE_ANY_OUTFIT,
  NAI_REF_OUTFIT,
  NAI_REF_STYLE,
  NOVELAI_MAX_DIRECTOR_REFERENCES,
  type NovelAiModelInfo,
  novelAiReferenceBaseType,
  novelAiReferencePreferSameOutfit,
} from "./novelaiModels.js";

/** Structural slice of an active Precise Reference entry on the generation panel. */
export interface DirectorRefEntry {
  image: string;
  description: string;
  strength: number;
  fidelity: number;
  enabled?: boolean;
  source_character_id?: string;
  source_ref_id?: string;
  auto_attached?: boolean;
  target_character_index?: number | null;
  sheet_x?: number | null;
  sheet_y?: number | null;
  sheet_scale?: number | null;
}

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

/**
 * Remove Precise Reference entries belonging to `characterId` (and optionally
 * only a specific `refId` on that character), also matching untagged legacy
 * entries whose base64 `image` sits in `legacyImages`.
 */
export function removeCharacterDirectorReferences<T extends DirectorRefEntry>(
  refs: readonly T[],
  characterId: string,
  opts?: { refId?: string; legacyImages?: ReadonlySet<string> },
): { next: T[]; removed: T[] } {
  const next: T[] = [];
  const removed: T[] = [];
  for (const r of refs) {
    const taggedMatch =
      r.source_character_id === characterId &&
      (opts?.refId === undefined || r.source_ref_id === opts.refId);
    const legacyMatch = !r.source_character_id && Boolean(opts?.legacyImages?.has(r.image));
    if (taggedMatch || legacyMatch) removed.push(r);
    else next.push(r);
  }
  return { next, removed };
}

/**
 * Replace any existing entries owned by `characterId` (or matching untagged
 * `legacyImages`) in place with `replacements`, skipping duplicates owned by
 * other entries and stopping at `max`.
 */
export function reconcileCharacterDirectorReferences<T extends DirectorRefEntry>(
  refs: readonly T[],
  characterId: string,
  replacements: readonly T[],
  opts?: { max?: number; legacyImages?: ReadonlySet<string>; preserveOverrides?: boolean },
): { next: T[]; added: T[]; skippedFull: boolean; duplicateCount: number } {
  const max = opts?.max ?? NOVELAI_MAX_DIRECTOR_REFERENCES;
  const remaining: T[] = [];
  const existingByImage = new Map<string, T>();
  let insertAt = -1;
  for (const r of refs) {
    const taggedMatch = r.source_character_id === characterId;
    const legacyMatch = !r.source_character_id && Boolean(opts?.legacyImages?.has(r.image));
    if (taggedMatch || legacyMatch) {
      if (insertAt === -1) insertAt = remaining.length;
      if (!existingByImage.has(r.image)) existingByImage.set(r.image, r);
    } else {
      remaining.push(r);
    }
  }
  if (insertAt === -1) insertAt = remaining.length;

  const existingImages = new Set(remaining.map((r) => r.image));
  const added: T[] = [];
  let skippedFull = false;
  let duplicateCount = 0;

  for (const item of replacements) {
    if (existingImages.has(item.image)) {
      duplicateCount += 1;
      continue;
    }
    if (remaining.length + added.length >= max) {
      skippedFull = true;
      break;
    }
    const prev = opts?.preserveOverrides ? existingByImage.get(item.image) : undefined;
    const entry: T = prev
      ? {
          ...item,
          description: prev.description,
          strength: prev.strength,
          fidelity: prev.fidelity,
          enabled: prev.enabled !== undefined ? prev.enabled : item.enabled,
          ...(prev.auto_attached === false ? { auto_attached: false } : {}),
          ...(prev.target_character_index !== undefined
            ? { target_character_index: prev.target_character_index }
            : {}),
          ...(prev.sheet_x !== undefined ? { sheet_x: prev.sheet_x } : {}),
          ...(prev.sheet_y !== undefined ? { sheet_y: prev.sheet_y } : {}),
          ...(prev.sheet_scale !== undefined ? { sheet_scale: prev.sheet_scale } : {}),
        }
      : item;
    existingImages.add(entry.image);
    added.push(entry);
  }

  const next = [...remaining];
  next.splice(insertAt, 0, ...added);
  return { next, added, skippedFull, duplicateCount };
}

/** Update description, strength, or fidelity on attached entries matching `(characterId, refId)`. */
export function patchCharacterDirectorReference<T extends DirectorRefEntry>(
  refs: readonly T[],
  characterId: string,
  refId: string,
  patch: Partial<Pick<DirectorRefEntry, "description" | "strength" | "fidelity">>,
): { next: T[]; changed: boolean } {
  let changed = false;
  const next = refs.map((r) => {
    if (r.source_character_id !== characterId || r.source_ref_id !== refId) return r;
    const updated = { ...r, ...patch };
    if (
      updated.description !== r.description ||
      updated.strength !== r.strength ||
      updated.fidelity !== r.fidelity
    ) {
      changed = true;
      return updated;
    }
    return r;
  });
  return { next, changed };
}

/**
 * Remove stale `auto_attached` character entries not present in `keepCharacterIds`.
 * Manual uploads (`!source_character_id`) and manual `auto_attached === false` entries stay.
 */
export function pruneAutoAttachedCharacterReferences<T extends DirectorRefEntry>(
  refs: readonly T[],
  keepCharacterIds: ReadonlySet<string>,
): { next: T[]; removed: T[] } {
  const next: T[] = [];
  const removed: T[] = [];
  for (const r of refs) {
    if (
      r.source_character_id !== undefined &&
      r.auto_attached === true &&
      !keepCharacterIds.has(r.source_character_id)
    ) {
      removed.push(r);
    } else {
      next.push(r);
    }
  }
  return { next, removed };
}

/** An active Precise Reference slot enriched with optional saved-character target info. */
export interface ActivePreciseRefItem {
  /** Index inside `director_references`, preserved so UI controls can remove the exact slot. */
  index: number;
  image: string;
  description: string;
  baseType: string;
  preferSameOutfit: boolean;
  strength: number;
  characterId: string | null;
  characterName: string | null;
}

/** Active Precise References summary with resolved character names and total Anlas cost. */
export interface ActivePreciseRefSummary {
  items: ActivePreciseRefItem[];
  count: number;
  anlas: number;
  characterIds: string[];
  characterNames: string[];
}

/**
 * Describe active Precise References that will be sent for `model` and `isInfill`,
 * resolving `source_character_id` against saved `characters` in slot order.
 */
export function describeActiveNovelAiDirectorReferences(
  model: NovelAiModelInfo | null | undefined,
  references: readonly DirectorRefEntry[],
  isInfill: boolean,
  characters: readonly { id: string; name: string }[],
): ActivePreciseRefSummary {
  if (!model?.preciseReference || (isInfill && !model.preciseReferenceInpainting)) {
    return { items: [], count: 0, anlas: 0, characterIds: [], characterNames: [] };
  }
  const charNameById = new Map<string, string>();
  for (const ch of characters) {
    const cleanName = ch.name.trim();
    if (cleanName) charNameById.set(ch.id, cleanName);
  }

  const items: ActivePreciseRefItem[] = [];
  const characterIds: string[] = [];
  const characterNames: string[] = [];
  const seenCharIds = new Set<string>();
  const seenCharNames = new Set<string>();

  references.forEach((r, index) => {
    if (r.enabled === false) return;
    if (!r.image) return;
    if (!model.outfitReferences && r.description === NAI_REF_OUTFIT) return;
    const baseType = novelAiReferenceBaseType(r.description);
    const preferSameOutfit = novelAiReferencePreferSameOutfit(r.description);
    const characterId = r.source_character_id ?? null;
    const characterName = characterId ? (charNameById.get(characterId) || null) : null;
    items.push({
      index,
      image: r.image,
      description: r.description,
      baseType,
      preferSameOutfit,
      strength: r.strength,
      characterId,
      characterName,
    });
    if (characterId && !seenCharIds.has(characterId)) {
      seenCharIds.add(characterId);
      characterIds.push(characterId);
    }
    if (characterName && !seenCharNames.has(characterName)) {
      seenCharNames.add(characterName);
      characterNames.push(characterName);
    }
  });

  return {
    items,
    count: items.length,
    anlas: items.length * PRECISE_REFERENCE_COST,
    characterIds,
    characterNames,
  };
}

/**
 * Group active Precise References into compact (`short`) and mode-annotated
 * (`detailed`) target labels preserving slot appearance order.
 */
export function formatActivePreciseRefTargets(
  items: readonly Pick<ActivePreciseRefItem, "characterId" | "characterName" | "baseType">[],
  labelForMode: (baseType: string) => string,
): { short: string; detailed: string } {
  const groups: { name: string; isCharacter: boolean; count: number; modes: string[] }[] = [];
  const groupByKey = new Map<string, number>();

  for (const item of items) {
    const isCharacter = Boolean(item.characterId && item.characterName);
    const key = isCharacter ? `char:${item.characterId}` : `mode:${item.baseType}`;
    const modeLabel = labelForMode(item.baseType);
    let idx = groupByKey.get(key);
    if (idx === undefined) {
      idx = groups.length;
      groupByKey.set(key, idx);
      groups.push({
        name: isCharacter ? (item.characterName as string) : modeLabel,
        isCharacter,
        count: 0,
        modes: [],
      });
    }
    const group = groups[idx];
    group.count += 1;
    if (modeLabel && !group.modes.includes(modeLabel)) {
      group.modes.push(modeLabel);
    }
  }

  const short = groups
    .map((g) => (g.count > 1 ? `${g.name} ×${g.count}` : g.name))
    .join(", ");
  const detailed = groups
    .map((g) => {
      if (g.isCharacter) return `${g.name} (${g.modes.join(", ")})`;
      return g.count > 1 ? `${g.name} ×${g.count}` : g.name;
    })
    .join(", ");

  return { short, detailed };
}

/** 64-bit perceptual difference hash (9x8 grid) + source aspect ratio. */
export interface ImageFingerprint {
  hi: number;
  lo: number;
  aspect: number;
}

/**
 * Compute a 64-bit horizontal gradient hash (dHash) from a 9x8 RGBA sample
 * alongside the original image's aspect ratio (`origWidth / origHeight`).
 */
export function computeDHashFromRgba(
  rgba: Uint8ClampedArray | readonly number[],
  width: number,
  height: number,
  origWidth: number,
  origHeight: number,
): ImageFingerprint | null {
  if (width !== 9 || height !== 8 || origWidth <= 0 || origHeight <= 0) return null;
  if (rgba.length < 9 * 8 * 4) return null;

  let hi = 0;
  let lo = 0;
  for (let y = 0; y < 8; y++) {
    const row = y * 9;
    for (let x = 0; x < 8; x++) {
      const i0 = (row + x) * 4;
      const i1 = (row + x + 1) * 4;
      const l0 = 0.299 * rgba[i0] + 0.587 * rgba[i0 + 1] + 0.114 * rgba[i0 + 2];
      const l1 = 0.299 * rgba[i1] + 0.587 * rgba[i1 + 1] + 0.114 * rgba[i1 + 2];
      if (l0 > l1) {
        const bit = y * 8 + x;
        if (bit < 32) hi = (hi | (1 << bit)) >>> 0;
        else lo = (lo | (1 << (bit - 32))) >>> 0;
      }
    }
  }
  return {
    hi: hi >>> 0,
    lo: lo >>> 0,
    aspect: origWidth / origHeight,
  };
}

function popcount32(x: number): number {
  let v = x >>> 0;
  v = v - ((v >>> 1) & 0x55555555);
  v = (v & 0x33333333) + ((v >>> 2) & 0x33333333);
  return (((v + (v >>> 4)) & 0x0f0f0f0f) * 0x01010101) >>> 24;
}

/** Hamming distance (0..64) between two `ImageFingerprint` hashes. */
export function dHashHammingDistance(a: ImageFingerprint, b: ImageFingerprint): number {
  return popcount32((a.hi ^ b.hi) >>> 0) + popcount32((a.lo ^ b.lo) >>> 0);
}

/** Saved character / ref candidate tested against an untagged Precise Reference image. */
export interface DirectorRefMatchCandidate {
  characterId: string;
  refId: string;
  base64?: string | null;
  fingerprint?: ImageFingerprint | null;
  preferred?: boolean;
}

/**
 * Match a Precise Reference image against saved character candidates using exact
 * base64 equality first, then bounded 64-bit dHash similarity + aspect guard.
 */
export function matchDirectorRefCandidate(
  target: { base64: string; fingerprint?: ImageFingerprint | null },
  candidates: readonly DirectorRefMatchCandidate[],
  opts?: { maxDistance?: number; maxAspectDiff?: number },
): { characterId: string; refId: string } | null {
  let exactFallback: DirectorRefMatchCandidate | null = null;
  for (const cand of candidates) {
    if (cand.base64 && cand.base64 === target.base64) {
      if (cand.preferred) {
        return { characterId: cand.characterId, refId: cand.refId };
      }
      if (!exactFallback) exactFallback = cand;
    }
  }
  if (exactFallback) {
    return { characterId: exactFallback.characterId, refId: exactFallback.refId };
  }

  const targetFp = target.fingerprint;
  if (!targetFp) return null;

  const maxDistance = opts?.maxDistance ?? 6;
  const maxAspectDiff = opts?.maxAspectDiff ?? 0.08;
  let best: DirectorRefMatchCandidate | null = null;
  let bestDist = Infinity;

  for (const cand of candidates) {
    const fp = cand.fingerprint;
    if (!fp) continue;
    if (Math.abs(targetFp.aspect - fp.aspect) > maxAspectDiff) continue;
    const dist = dHashHammingDistance(targetFp, fp);
    if (dist > maxDistance) continue;
    if (
      dist < bestDist ||
      (dist === bestDist && Boolean(cand.preferred) && !best?.preferred)
    ) {
      best = cand;
      bestDist = dist;
    }
  }

  return best ? { characterId: best.characterId, refId: best.refId } : null;
}

/**
 * Identify an unambiguous single saved character from an image's metadata
 * (NovelAI comment/description or ComfyUI prompt metadata). Returns `null` when
 * ambiguous (0 or multiple distinct character families matched).
 */
export function detectSingleCharacterFromMetadata<T extends DetectCard>(
  metadata: Record<string, string> | null | undefined,
  cards: readonly T[],
): T | null {
  if (!metadata || cards.length === 0) return null;

  const hits = detectCharacters(metadata, prepareCards(cards));
  if (hits.length === 1) {
    return cards.find((c) => c.id === hits[0].cardId) ?? null;
  }
  if (hits.length > 1) return null;

  const base = metadata.positive_prompt ?? metadata.Description ?? "";
  let characters: string[] = [];
  const rawChars = metadata.mooshie_novelai_characters;
  if (typeof rawChars === "string" && rawChars.trim()) {
    try {
      const parsed = JSON.parse(rawChars) as unknown;
      if (Array.isArray(parsed)) {
        characters = parsed
          .map((item) =>
            item && typeof item === "object" && typeof (item as { prompt?: unknown }).prompt === "string"
              ? (item as { prompt: string }).prompt
              : "",
          )
          .filter((p) => p.trim() !== "");
      }
    } catch {
      characters = [];
    }
  }

  if (!base.trim() && characters.length === 0) return null;
  const matched = matchCharacterCardsInText(cards, [], { base, characters });
  return matched.length === 1 ? matched[0] : null;
}

/** Minimal slice of a NovelAI character box used to resolve PR isolation targets. */
export interface CharacterSlotLike {
  prompt: string;
  enabled?: boolean;
  center?: { x: number; y: number };
}

/**
 * Resolve each Precise Reference's target 0-based character slot index, or
 * `null` when it stays global on the base pass.
 *
 * - Style references are always global (`null`).
 * - `target_character_index === -1` forces global (`null`).
 * - `target_character_index >= 0` selects that slot if enabled and non-empty.
 * - `undefined`/`null` (Auto) matches `source_character_id` (or its card
 *   family) against enabled non-empty character box prompts.
 */
export function resolveDirectorReferenceCharacterTargets(
  references: readonly DirectorRefEntry[],
  characters: readonly CharacterSlotLike[],
  savedCards: readonly DetectCard[],
  maxCharacters?: number,
): (number | null)[] {
  const limit = maxCharacters !== undefined ? Math.min(characters.length, maxCharacters) : characters.length;
  const activeIndices: number[] = [];
  for (let i = 0; i < limit; i++) {
    const ch = characters[i];
    if (ch && ch.enabled !== false && ch.prompt.trim() !== "") {
      activeIndices.push(i);
    }
  }
  const activeSet = new Set(activeIndices);

  const cardById = new Map(savedCards.map((c) => [c.id, c]));
  const familyOfId = (id: string): string => {
    const c = cardById.get(id);
    return c?.parentId && cardById.has(c.parentId) ? c.parentId : id;
  };

  const slotCardIds = new Map<number, string[]>();
  const slotFamilies = new Map<number, Set<string>>();
  if (savedCards.length > 0 && references.some((r) => Boolean(r.source_character_id))) {
    for (const idx of activeIndices) {
      const hits = matchCharacterCardsInText(savedCards, [], {
        base: "",
        characters: [characters[idx].prompt],
      });
      slotCardIds.set(idx, hits.map((h) => h.id));
      slotFamilies.set(idx, new Set(hits.map((h) => familyOfId(h.id))));
    }
  }

  return references.map((ref) => {
    if (novelAiReferenceBaseType(ref.description) === NAI_REF_STYLE) {
      return null;
    }
    const raw = ref.target_character_index;
    if (raw === -1) return null;
    if (typeof raw === "number" && Number.isInteger(raw) && raw >= 0) {
      return activeSet.has(raw) ? raw : null;
    }
    const sourceId = ref.source_character_id;
    if (!sourceId) return null;
    for (const idx of activeIndices) {
      if (slotCardIds.get(idx)?.includes(sourceId)) return idx;
    }
    const sourceFam = familyOfId(sourceId);
    for (const idx of activeIndices) {
      if (slotFamilies.get(idx)?.has(sourceFam)) return idx;
    }
    return null;
  });
}

export type MultiCharPrStrategy = "sheet" | "passes" | "hybrid";

export interface MultiCharPrGroupSummary {
  characterIndex: number;
  refCount: number;
}

export interface MultiCharPrPlanSummary {
  active: boolean;
  strategy: MultiCharPrStrategy;
  usesSheet: boolean;
  usesPasses: boolean;
  enabledCharacterCount: number;
  baseRefCount: number;
  groups: MultiCharPrGroupSummary[];
  resolvedTargets: (number | null)[];
}

export function normalizeMultiCharPrStrategy(raw?: string | null): MultiCharPrStrategy {
  if (raw === "passes" || raw === "hybrid") return raw;
  return "sheet";
}

/**
 * Summarize how active Precise References split between the base pass and
 * isolated per-character passes so the UI, cost badge, and submission agree.
 */
export function summarizeMultiCharPrPlan(
  model: NovelAiModelInfo | null | undefined,
  references: readonly DirectorRefEntry[],
  characters: readonly CharacterSlotLike[],
  savedCards: readonly DetectCard[],
  isInfill: boolean,
  enabled: boolean,
  strategyRaw: MultiCharPrStrategy | string = "sheet",
): MultiCharPrPlanSummary {
  const strategy = normalizeMultiCharPrStrategy(strategyRaw);
  const maxChars = model?.maxCharacters;
  const resolvedTargets = resolveDirectorReferenceCharacterTargets(
    references,
    characters,
    savedCards,
    maxChars,
  );

  let enabledCharacterCount = 0;
  const limit = maxChars !== undefined ? Math.min(characters.length, maxChars) : characters.length;
  for (let i = 0; i < limit; i++) {
    const ch = characters[i];
    if (ch && ch.enabled !== false && ch.prompt.trim() !== "") {
      enabledCharacterCount += 1;
    }
  }

  if (!model?.preciseReference || (isInfill && !model.preciseReferenceInpainting)) {
    return {
      active: false,
      strategy,
      usesSheet: false,
      usesPasses: false,
      enabledCharacterCount,
      baseRefCount: 0,
      groups: [],
      resolvedTargets,
    };
  }

  const isRefActive = (r: DirectorRefEntry) =>
    r.enabled !== false &&
    r.image.length > 0 &&
    (model.outfitReferences || r.description !== NAI_REF_OUTFIT);

  if (!enabled || isInfill || enabledCharacterCount < 2) {
    const baseRefCount = references.filter(isRefActive).length;
    return {
      active: false,
      strategy,
      usesSheet: false,
      usesPasses: false,
      enabledCharacterCount,
      baseRefCount,
      groups: [],
      resolvedTargets,
    };
  }

  let baseRefCount = 0;
  const groupMap = new Map<number, number>();
  references.forEach((r, idx) => {
    if (!isRefActive(r)) return;
    const target = resolvedTargets[idx];
    if (target !== null) {
      groupMap.set(target, (groupMap.get(target) ?? 0) + 1);
    } else {
      baseRefCount += 1;
    }
  });

  const groups = [...groupMap.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([characterIndex, refCount]) => ({ characterIndex, refCount }));
  const active = groups.length > 0;

  return {
    active,
    strategy,
    usesSheet: active && strategy !== "passes",
    usesPasses: active && strategy !== "sheet",
    enabledCharacterCount,
    baseRefCount,
    groups,
    resolvedTargets,
  };
}

/** 97 buckets accepted by V5's Precise Reference encoder; mirrors `reference_canvas::V5_REFERENCE_BUCKETS`. */
export const V5_REFERENCE_BUCKETS: readonly (readonly [number, number])[] = [
  [256, 4096], [256, 4032], [256, 3968], [256, 3904], [256, 3840], [256, 3776],
  [256, 3712], [256, 3648], [256, 3584], [256, 3520], [256, 3456], [256, 3392],
  [256, 3328], [320, 3264], [320, 3200], [320, 3136], [320, 3072], [320, 3008],
  [320, 2944], [320, 2880], [320, 2816], [320, 2752], [384, 2688], [384, 2624],
  [384, 2560], [384, 2496], [384, 2432], [384, 2368], [448, 2304], [448, 2240],
  [448, 2176], [448, 2112], [512, 2048], [512, 1984], [512, 1920], [512, 1856],
  [576, 1792], [576, 1728], [576, 1664], [640, 1600], [640, 1536], [704, 1472],
  [704, 1408], [768, 1344], [768, 1280], [832, 1216], [896, 1152], [960, 1088],
  [1024, 1024], [1088, 960], [1152, 896], [1216, 832], [1280, 768], [1344, 768],
  [1408, 704], [1472, 704], [1536, 640], [1600, 640], [1664, 576], [1728, 576],
  [1792, 576], [1856, 512], [1920, 512], [1984, 512], [2048, 512], [2112, 448],
  [2176, 448], [2240, 448], [2304, 448], [2368, 384], [2432, 384], [2496, 384],
  [2560, 384], [2624, 384], [2688, 384], [2752, 320], [2816, 320], [2880, 320],
  [2944, 320], [3008, 320], [3072, 320], [3136, 320], [3200, 320], [3264, 320],
  [3328, 256], [3392, 256], [3456, 256], [3520, 256], [3584, 256], [3648, 256],
  [3712, 256], [3776, 256], [3840, 256], [3904, 256], [3968, 256], [4032, 256],
  [4096, 256],
] as const;

const ACCEPTED_V45_CANVASES: readonly (readonly [number, number])[] = [
  [1024, 1536],
  [1536, 1024],
  [1472, 1472],
];

/** Pick the V5 bucket or V4.5 canvas closest in aspect ratio to `genW` x `genH`. */
export function chooseNovelAiSheetCanvas(
  genW: number,
  genH: number,
  isV5: boolean,
): [number, number] {
  const w = Math.max(1, genW);
  const h = Math.max(1, genH);
  if (isV5) {
    const target = Math.log(w / h);
    let best: readonly [number, number] = V5_REFERENCE_BUCKETS[0];
    let bestDiff = Infinity;
    for (const pair of V5_REFERENCE_BUCKETS) {
      const diff = Math.abs(target - Math.log(pair[0] / pair[1]));
      if (diff < bestDiff) {
        bestDiff = diff;
        best = pair;
      }
    }
    return [best[0], best[1]];
  }
  const aspect = w / h;
  let best: readonly [number, number] = ACCEPTED_V45_CANVASES[0];
  let bestDiff = Infinity;
  for (const pair of ACCEPTED_V45_CANVASES) {
    const diff = Math.abs(pair[0] / pair[1] - aspect);
    if (diff < bestDiff) {
      bestDiff = diff;
      best = pair;
    }
  }
  return [best[0], best[1]];
}

/** Automatic relative scale for items on the combined multi-character sheet. */
export function autoSheetScale(canvasW: number, canvasH: number, itemCount: number): number {
  const ratio = (canvasW / Math.max(1, canvasH)) / (Math.max(1, itemCount) * 0.55);
  return Math.min(0.85, Math.max(0.45, ratio));
}

function clampRange(val: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, val));
}

/** Pixel rectangle on `canvasW` x `canvasH` mirroring Rust `compute_sheet_item_rect`. */
export function computeSheetItemRect(
  canvasW: number,
  canvasH: number,
  imgW: number,
  imgH: number,
  x: number,
  y: number,
  scale: number,
): { left: number; top: number; width: number; height: number } {
  const cw = Math.max(1, canvasW);
  const ch = Math.max(1, canvasH);
  const iw = Math.max(1, imgW);
  const ih = Math.max(1, imgH);
  const s = clampRange(Number.isFinite(scale) ? scale : 0.65, 0.2, 1.35);
  const cx = clampRange(Number.isFinite(x) ? x : 0.5, 0.05, 0.95);
  const cy = clampRange(Number.isFinite(y) ? y : 0.5, 0.05, 0.95);

  const factor = Math.min((cw * s) / iw, (ch * s) / ih);
  const width = Math.max(1, Math.round(iw * factor));
  const height = Math.max(1, Math.round(ih * factor));
  const left = Math.round(cx * cw - width / 2);
  const top = Math.round(cy * ch - height / 2);
  return { left, top, width, height };
}

export interface SheetLayoutItem<T extends DirectorRefEntry = DirectorRefEntry> {
  refIndex: number;
  characterIndex: number;
  ref: T;
  x: number;
  y: number;
  scale: number;
}

/** Resolve sheet item coordinates and scales, mirroring `resolve_sheet_placements`. */
export function resolveMultiCharSheetLayout<T extends DirectorRefEntry>(
  references: readonly T[],
  resolvedTargets: readonly (number | null)[],
  characters: readonly CharacterSlotLike[],
  useCoords: boolean,
  canvasW: number,
  canvasH: number,
  isRefActive: (r: T) => boolean = (r) => r.enabled !== false && r.image.length > 0,
): SheetLayoutItem<T>[] {
  const groupMap = new Map<number, { refIndex: number; ref: T }[]>();
  references.forEach((ref, refIndex) => {
    if (!isRefActive(ref)) return;
    const target = resolvedTargets[refIndex];
    if (target === null || target === undefined) return;
    const list = groupMap.get(target);
    if (list) list.push({ refIndex, ref });
    else groupMap.set(target, [{ refIndex, ref }]);
  });

  const sortedGroups = [...groupMap.entries()].sort((a, b) => a[0] - b[0]);
  const groupCount = Math.max(1, sortedGroups.length);
  const totalItems = sortedGroups.reduce((acc, [, list]) => acc + list.length, 0);
  if (totalItems === 0) return [];

  const defaultScale = autoSheetScale(canvasW, canvasH, totalItems);
  const out: SheetLayoutItem<T>[] = [];

  sortedGroups.forEach(([characterIndex, items], groupOrd) => {
    const ch = characters[characterIndex];
    const baseX = useCoords
      ? clampRange(ch?.center?.x ?? 0.5, 0.05, 0.95)
      : (groupOrd + 0.5) / groupCount;
    const baseY = useCoords ? clampRange(ch?.center?.y ?? 0.5, 0.05, 0.95) : 0.5;
    const nInGroup = items.length;
    items.forEach(({ refIndex, ref }, subIdx) => {
      const spread = nInGroup > 1 ? (subIdx - (nInGroup - 1) / 2) * 0.06 : 0;
      const autoX = clampRange(baseX + spread, 0.05, 0.95);
      const x =
        typeof ref.sheet_x === "number" && Number.isFinite(ref.sheet_x)
          ? clampRange(ref.sheet_x, 0.05, 0.95)
          : autoX;
      const y =
        typeof ref.sheet_y === "number" && Number.isFinite(ref.sheet_y)
          ? clampRange(ref.sheet_y, 0.05, 0.95)
          : baseY;
      const scale =
        typeof ref.sheet_scale === "number" && Number.isFinite(ref.sheet_scale)
          ? clampRange(ref.sheet_scale, 0.2, 1.35)
          : defaultScale;
      out.push({ refIndex, characterIndex, ref, x, y, scale });
    });
  });

  return out;
}
