/**
 * Attaches and keeps saved characters' Precise Reference images in sync with
 * the generation store.
 */
import { generation } from "../stores/generation.svelte.js";
import type { SavedCharacter } from "../stores/savedCharacters.svelte.js";
import type { NovelAiDirectorReference } from "../types/index.js";
import {
  clampCharacterReferenceFidelity,
  clampCharacterReferenceStrength,
  computeDHashFromRgba,
  effectiveCharacterPreciseReferences,
  matchDirectorRefCandidate,
  normalizeCharacterReferenceDescription,
  patchCharacterDirectorReference,
  pruneAutoAttachedCharacterReferences,
  reconcileCharacterDirectorReferences,
  removeCharacterDirectorReferences,
  type DirectorRefMatchCandidate,
  type ImageFingerprint,
} from "./characterPreciseRefLogic.js";
import { characterReferenceBase64 } from "./characterThumbnail.js";
import { novelAiBase64ToSrc } from "./novelaiImage.js";
import {
  NAI_REF_STYLE,
  NOVELAI_MAX_DIRECTOR_REFERENCES,
  novelAiReferenceBaseType,
} from "./novelaiModels.js";

export interface AttachCharacterReferencesResult {
  addedBase64s: string[];
  replacedCount: number;
  attachedNames: string[];
  skippedVibes: boolean;
  skippedFull: boolean;
  duplicateCount: number;
  failedCount: number;
}

export interface LegacySourceLike {
  thumbnail: string | null;
  reference: string | null;
}

/** Encode a unique set of stored character thumbnails/references into NovelAI base64 strings. */
export async function encodeCharacterLegacyImages(
  items: ReadonlyArray<LegacySourceLike>,
): Promise<Set<string>> {
  const seenSrc = new Set<string>();
  const images = new Set<string>();
  for (const item of items) {
    const src = item.reference ?? item.thumbnail;
    if (!src || seenSrc.has(src)) continue;
    seenSrc.add(src);
    const encoded = await characterReferenceBase64({
      thumbnail: item.thumbnail,
      reference: item.reference,
    });
    if (encoded) images.add(encoded);
  }
  return images;
}

async function encodeCardReferences(
  card: SavedCharacter,
  autoAttached: boolean,
): Promise<{ entries: NovelAiDirectorReference[]; failedCount: number }> {
  const model = generation.novelAiModel;
  const supportsOutfits = model?.outfitReferences ?? false;
  const strengthMax = model?.preciseReferenceStrengthMax ?? 1;
  const refs = effectiveCharacterPreciseReferences(card);
  const entries: NovelAiDirectorReference[] = [];
  let failedCount = 0;

  for (const ref of refs) {
    const image = await characterReferenceBase64({
      thumbnail: ref.thumbnail,
      reference: ref.reference,
    });
    if (!image) {
      failedCount += 1;
      continue;
    }
    const description = normalizeCharacterReferenceDescription(ref.description, {
      supportsOutfits,
      supportsSameOutfit: supportsOutfits,
    });
    entries.push({
      image,
      description,
      strength: clampCharacterReferenceStrength(ref.strength, strengthMax),
      fidelity: clampCharacterReferenceFidelity(ref.fidelity),
      enabled: true,
      source_character_id: card.id,
      source_ref_id: ref.id,
      auto_attached: autoAttached,
    });
  }
  return { entries, failedCount };
}

/**
 * Encode and attach `characters`' effective Precise Reference images to the
 * active NovelAI generation session, replacing any prior entries owned by the
 * same character (including legacy untagged card pictures).
 *
 * By default (`requireEnabled !== false`) only characters with
 * `usePreciseReference === true` are attached, and Vibe Transfer is left alone
 * (`skippedVibes: true`). Explicit manual triggers pass `requireEnabled: false,
 * autoAttached: false`.
 */
export async function attachCharacterPreciseReferences(
  characters: readonly SavedCharacter[],
  opts?: {
    requireEnabled?: boolean;
    forceReplaceVibes?: boolean;
    autoAttached?: boolean;
    preserveOverrides?: boolean;
  },
): Promise<AttachCharacterReferencesResult> {
  const empty: AttachCharacterReferencesResult = {
    addedBase64s: [],
    replacedCount: 0,
    attachedNames: [],
    skippedVibes: false,
    skippedFull: false,
    duplicateCount: 0,
    failedCount: 0,
  };
  if (!generation.isNovelAi || !generation.supportsNovelAiPreciseReference) {
    return empty;
  }

  const candidates =
    opts?.requireEnabled === false
      ? characters
      : characters.filter((c) => Boolean(c.usePreciseReference));
  const withRefs = candidates.filter((c) => effectiveCharacterPreciseReferences(c).length > 0);
  if (withRefs.length === 0) return empty;

  if (!opts?.forceReplaceVibes && generation.novelaiSettings.vibes.length > 0) {
    return { ...empty, skippedVibes: true };
  }

  const autoAttached = opts?.autoAttached ?? true;
  const initialImages = new Set(generation.novelaiSettings.director_references.map((r) => r.image));
  let working = [...generation.novelaiSettings.director_references];

  const addedBase64s: string[] = [];
  const attachedNames: string[] = [];
  const addedNameSet = new Set<string>();
  let replacedCount = 0;
  let skippedFull = false;
  let duplicateCount = 0;
  let failedCount = 0;
  let changed = false;

  for (const card of withRefs) {
    const legacyImages = working.some((r) => !r.source_character_id)
      ? await encodeCharacterLegacyImages([card, ...(card.preciseReferences ?? [])])
      : new Set<string>();
    const prevOwnedCount = working.filter(
      (r) =>
        r.source_character_id === card.id ||
        (!r.source_character_id && legacyImages.has(r.image)),
    ).length;

    const encoded = await encodeCardReferences(card, autoAttached);
    failedCount += encoded.failedCount;
    if (encoded.entries.length === 0) continue;

    const res = reconcileCharacterDirectorReferences(working, card.id, encoded.entries, {
      max: NOVELAI_MAX_DIRECTOR_REFERENCES,
      legacyImages,
      preserveOverrides: opts?.preserveOverrides ?? true,
    });
    working = res.next;
    changed = true;
    replacedCount += Math.min(prevOwnedCount, res.added.length);
    if (res.skippedFull) skippedFull = true;
    duplicateCount += res.duplicateCount;

    for (const entry of res.added) {
      if (!initialImages.has(entry.image)) {
        addedBase64s.push(entry.image);
      }
      if (!addedNameSet.has(card.name)) {
        addedNameSet.add(card.name);
        attachedNames.push(card.name);
      }
    }
    if (skippedFull) break;
  }

  if (changed) {
    generation.updateNovelAiSettings({
      director_references: working,
      ...(working.length > 0 ? { vibes: [] } : {}),
    });
  }

  return {
    addedBase64s,
    replacedCount,
    attachedNames,
    skippedVibes: false,
    skippedFull,
    duplicateCount,
    failedCount,
  };
}

/**
 * Remove attached Precise References belonging to `characterId` (or a specific
 * `refId`), immediately clearing tagged entries and sweeping any matching
 * legacy untagged entries async.
 */
export async function removeAttachedCharacterReferences(
  characterId: string,
  opts?: { refId?: string; legacySources?: ReadonlyArray<LegacySourceLike> },
): Promise<number> {
  let removedTotal = 0;
  const syncRes = removeCharacterDirectorReferences(
    generation.novelaiSettings.director_references,
    characterId,
    { refId: opts?.refId },
  );
  if (syncRes.removed.length > 0) {
    removedTotal += syncRes.removed.length;
    generation.updateNovelAiSettings({ director_references: syncRes.next });
  }

  const remaining = generation.novelaiSettings.director_references;
  if (opts?.legacySources?.length && remaining.some((r) => !r.source_character_id)) {
    const legacyImages = await encodeCharacterLegacyImages(opts.legacySources);
    if (legacyImages.size > 0) {
      const legacyRes = removeCharacterDirectorReferences(
        generation.novelaiSettings.director_references,
        characterId,
        { refId: opts.refId, legacyImages },
      );
      if (legacyRes.removed.length > 0) {
        removedTotal += legacyRes.removed.length;
        generation.updateNovelAiSettings({ director_references: legacyRes.next });
      }
    }
  }
  return removedTotal;
}

/**
 * Update description, strength, or fidelity in place on attached entries
 * matching `(characterId, refId)`.
 */
export function patchAttachedCharacterReference(
  characterId: string,
  refId: string,
  patch: { description?: string; strength?: number; fidelity?: number },
): void {
  const model = generation.novelAiModel;
  const supportsOutfits = model?.outfitReferences ?? false;
  const strengthMax = model?.preciseReferenceStrengthMax ?? 1;
  const normalizedPatch: Partial<Pick<NovelAiDirectorReference, "description" | "strength" | "fidelity">> = {};
  if (patch.description !== undefined) {
    normalizedPatch.description = normalizeCharacterReferenceDescription(patch.description, {
      supportsOutfits,
      supportsSameOutfit: supportsOutfits,
    });
  }
  if (patch.strength !== undefined) {
    normalizedPatch.strength = clampCharacterReferenceStrength(patch.strength, strengthMax);
  }
  if (patch.fidelity !== undefined) {
    normalizedPatch.fidelity = clampCharacterReferenceFidelity(patch.fidelity);
  }
  if (Object.keys(normalizedPatch).length === 0) return;

  const res = patchCharacterDirectorReference(
    generation.novelaiSettings.director_references,
    characterId,
    refId,
    normalizedPatch,
  );
  if (res.changed) {
    generation.updateNovelAiSettings({ director_references: res.next });
  }
}

/**
 * Synchronize an already-attached character's Precise References when the card
 * is edited in `SavedCharactersStore`. No-op when the character is not currently
 * attached in `director_references`.
 */
export async function syncAttachedCharacterReferences(
  card: SavedCharacter,
  opts?: { previousSources?: ReadonlyArray<LegacySourceLike> },
): Promise<void> {
  const sources: LegacySourceLike[] = opts?.previousSources?.length
    ? [...opts.previousSources, card, ...(card.preciseReferences ?? [])]
    : [card, ...(card.preciseReferences ?? [])];

  const current = generation.novelaiSettings.director_references;
  if (current.length === 0) return;

  const legacyImages = current.some((r) => !r.source_character_id)
    ? await encodeCharacterLegacyImages(sources)
    : new Set<string>();

  const existingOwned = generation.novelaiSettings.director_references.filter(
    (r) =>
      r.source_character_id === card.id ||
      (!r.source_character_id && legacyImages.has(r.image)),
  );
  if (existingOwned.length === 0) return;

  if (!card.usePreciseReference) {
    const res = removeCharacterDirectorReferences(
      generation.novelaiSettings.director_references,
      card.id,
      { legacyImages },
    );
    if (res.removed.length > 0) {
      generation.updateNovelAiSettings({ director_references: res.next });
    }
    return;
  }

  const autoAttached = existingOwned[0]?.auto_attached ?? true;
  const encoded = await encodeCardReferences(card, autoAttached);
  const res = reconcileCharacterDirectorReferences(
    generation.novelaiSettings.director_references,
    card.id,
    encoded.entries,
    { max: NOVELAI_MAX_DIRECTOR_REFERENCES, legacyImages },
  );
  generation.updateNovelAiSettings({ director_references: res.next });
}

/**
 * Remove `auto_attached` character entries not present in `keepCharacterIds`,
 * returning what was removed so Enhance Undo can restore them.
 */
export function pruneUnselectedAutoCharacterReferences(
  keepCharacterIds: ReadonlySet<string>,
): NovelAiDirectorReference[] {
  const res = pruneAutoAttachedCharacterReferences(
    generation.novelaiSettings.director_references,
    keepCharacterIds,
  );
  if (res.removed.length > 0) {
    generation.updateNovelAiSettings({ director_references: res.next });
  }
  return res.removed;
}

/** Re-insert previously removed `NovelAiDirectorReference` entries (for Undo). */
export function restoreDirectorReferences(entries: readonly NovelAiDirectorReference[]): void {
  if (entries.length === 0) return;
  const current = generation.novelaiSettings.director_references;
  const seen = new Set(current.map((r) => r.image));
  const restored = [...current];
  for (const entry of entries) {
    if (restored.length >= NOVELAI_MAX_DIRECTOR_REFERENCES) break;
    if (seen.has(entry.image)) continue;
    seen.add(entry.image);
    restored.push(entry);
  }
  if (restored.length !== current.length) {
    generation.updateNovelAiSettings({
      director_references: restored,
      vibes: [],
    });
  }
}

/**
 * Remove only the Precise Reference entries matching `images` (used by Enhance
 * Undo so manually attached references are preserved).
 */
export function removeDirectorReferencesByImages(images: readonly string[]): void {
  if (images.length === 0) return;
  const targets = new Set(images);
  const current = generation.novelaiSettings.director_references;
  if (!current.some((r) => targets.has(r.image))) return;
  generation.updateNovelAiSettings({
    director_references: current.filter((r) => !targets.has(r.image)),
  });
}

const MAX_FINGERPRINT_CACHE = 256;
const fingerprintCache = new Map<string, ImageFingerprint | null>();
const base64Cache = new Map<string, string | null>();

function setBoundedCache<T>(cache: Map<string, T>, key: string, value: T): void {
  if (cache.size >= MAX_FINGERPRINT_CACHE) {
    const oldest = cache.keys().next().value;
    if (oldest !== undefined) cache.delete(oldest);
  }
  cache.set(key, value);
}

async function cachedReferenceBase64(source: LegacySourceLike): Promise<string | null> {
  const key = source.reference ?? source.thumbnail ?? "";
  if (!key) return null;
  if (base64Cache.has(key)) return base64Cache.get(key) ?? null;
  const encoded = await characterReferenceBase64(source);
  if (encoded) setBoundedCache(base64Cache, key, encoded);
  return encoded;
}

/**
 * Decode an image data/blob URL into a 9x8 offscreen canvas and compute its
 * 64-bit dHash + source aspect ratio for visual matching across resolutions.
 */
export async function computeImageFingerprint(src: string): Promise<ImageFingerprint | null> {
  if (!src || typeof document === "undefined" || typeof Image === "undefined") return null;
  if (fingerprintCache.has(src)) return fingerprintCache.get(src) ?? null;

  const fp = await new Promise<ImageFingerprint | null>((resolve) => {
    const img = new Image();
    img.onload = () => {
      try {
        const origW = img.naturalWidth || img.width;
        const origH = img.naturalHeight || img.height;
        const canvas = document.createElement("canvas");
        canvas.width = 9;
        canvas.height = 8;
        const ctx = canvas.getContext("2d", { willReadFrequently: true });
        if (!ctx) {
          resolve(null);
          return;
        }
        ctx.drawImage(img, 0, 0, 9, 8);
        const rgba = ctx.getImageData(0, 0, 9, 8).data;
        resolve(computeDHashFromRgba(rgba, 9, 8, origW, origH));
      } catch {
        resolve(null);
      }
    };
    img.onerror = () => resolve(null);
    img.src = src;
  });

  if (fp) setBoundedCache(fingerprintCache, src, fp);
  return fp;
}

/**
 * Match a bare base64 PNG Precise Reference against saved characters via exact
 * base64 equality first, then visual 64-bit dHash similarity across resolutions.
 */
export async function matchImageToSavedCharacter(
  base64: string,
  characters: readonly SavedCharacter[],
  preferredArch?: string | null,
): Promise<{ characterId: string; refId: string } | null> {
  if (!base64 || characters.length === 0) return null;

  interface CandidateSource {
    characterId: string;
    refId: string;
    preferred: boolean;
    source: LegacySourceLike;
    fpSrc: string | null;
  }

  const sources: CandidateSource[] = [];
  for (const card of characters) {
    const preferred = Boolean(preferredArch && card.architecture === preferredArch);
    for (const ref of card.preciseReferences ?? []) {
      sources.push({
        characterId: card.id,
        refId: ref.id,
        preferred,
        source: { thumbnail: ref.thumbnail, reference: ref.reference },
        fpSrc: ref.thumbnail ?? ref.reference,
      });
    }
    if (card.reference ?? card.thumbnail) {
      sources.push({
        characterId: card.id,
        refId: "__card__",
        preferred,
        source: { thumbnail: card.thumbnail, reference: card.reference },
        fpSrc: card.thumbnail ?? card.reference,
      });
    }
  }
  if (sources.length === 0) return null;

  const encodedList = await Promise.all(sources.map((s) => cachedReferenceBase64(s.source)));
  const exactCandidates: DirectorRefMatchCandidate[] = sources.map((s, i) => ({
    characterId: s.characterId,
    refId: s.refId,
    preferred: s.preferred,
    base64: encodedList[i],
  }));
  const exactMatch = matchDirectorRefCandidate({ base64 }, exactCandidates);
  if (exactMatch) return exactMatch;

  const targetFp = await computeImageFingerprint(novelAiBase64ToSrc(base64));
  if (!targetFp) return null;

  const fps = await Promise.all(
    sources.map((s) => (s.fpSrc ? computeImageFingerprint(s.fpSrc) : Promise.resolve(null))),
  );
  const visualCandidates: DirectorRefMatchCandidate[] = sources.map((s, i) => ({
    characterId: s.characterId,
    refId: s.refId,
    preferred: s.preferred,
    base64: encodedList[i],
    fingerprint: fps[i],
  }));
  return matchDirectorRefCandidate({ base64, fingerprint: targetFp }, visualCandidates);
}

/**
 * Automatically backfill `source_character_id` / `source_ref_id` on active
 * non-Style Precise References that lack valid provenance (for example refs
 * persisted from older builds or added manually matching a saved character).
 */
export async function reconcileUntaggedDirectorReferences(
  characters: readonly SavedCharacter[],
  preferredArch?: string | null,
): Promise<number> {
  if (characters.length === 0) return 0;
  const current = generation.novelaiSettings.director_references;
  if (current.length === 0) return 0;

  const validIds = new Set(characters.map((c) => c.id));
  const uniqueImages = new Set<string>();
  for (const r of current) {
    if (!r.image) continue;
    if (novelAiReferenceBaseType(r.description) === NAI_REF_STYLE) continue;
    if (r.source_character_id && validIds.has(r.source_character_id)) continue;
    uniqueImages.add(r.image);
  }
  if (uniqueImages.size === 0) return 0;

  const resolvedByImage = new Map<string, { characterId: string; refId: string }>();
  await Promise.all(
    [...uniqueImages].map(async (image) => {
      const match = await matchImageToSavedCharacter(image, characters, preferredArch);
      if (match) resolvedByImage.set(image, match);
    }),
  );
  if (resolvedByImage.size === 0) return 0;

  const latest = generation.novelaiSettings.director_references;
  const latestValidIds = new Set(characters.map((c) => c.id));
  let updated = 0;
  const next = latest.map((r) => {
    if (
      !r.image ||
      novelAiReferenceBaseType(r.description) === NAI_REF_STYLE ||
      (r.source_character_id && latestValidIds.has(r.source_character_id))
    ) {
      return r;
    }
    const match = resolvedByImage.get(r.image);
    if (!match) return r;
    updated += 1;
    return {
      ...r,
      source_character_id: match.characterId,
      source_ref_id: match.refId,
    };
  });
  if (updated > 0) {
    generation.updateNovelAiSettings({ director_references: next });
  }
  return updated;
}
