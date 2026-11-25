/**
 * Attaches saved characters' Precise Reference images to the generation store.
 */
import { generation } from "../stores/generation.svelte.js";
import type { SavedCharacter } from "../stores/savedCharacters.svelte.js";
import {
  clampCharacterReferenceFidelity,
  clampCharacterReferenceStrength,
  effectiveCharacterPreciseReferences,
  normalizeCharacterReferenceDescription,
} from "./characterPreciseRefLogic.js";
import { characterReferenceBase64 } from "./characterThumbnail.js";
import { NOVELAI_MAX_DIRECTOR_REFERENCES } from "./novelaiModels.js";

export interface AttachCharacterReferencesResult {
  addedBase64s: string[];
  attachedNames: string[];
  skippedVibes: boolean;
  skippedFull: boolean;
  duplicateCount: number;
  failedCount: number;
}

/**
 * Encode and attach `characters`' effective Precise Reference images to the
 * active NovelAI generation session.
 *
 * By default (`requireEnabled !== false`) only characters with
 * `usePreciseReference === true` are attached, and Vibe Transfer is left alone
 * (`skippedVibes: true`) rather than silently wiped. Explicit manual triggers
 * pass `requireEnabled: false, forceReplaceVibes: true`.
 */
export async function attachCharacterPreciseReferences(
  characters: readonly SavedCharacter[],
  opts?: { requireEnabled?: boolean; forceReplaceVibes?: boolean },
): Promise<AttachCharacterReferencesResult> {
  const empty: AttachCharacterReferencesResult = {
    addedBase64s: [],
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

  const model = generation.novelAiModel;
  const supportsOutfits = model?.outfitReferences ?? false;
  const strengthMax = model?.preciseReferenceStrengthMax ?? 1;

  const addedBase64s: string[] = [];
  const attachedNames: string[] = [];
  const addedNameSet = new Set<string>();
  let skippedFull = false;
  let duplicateCount = 0;
  let failedCount = 0;

  for (const card of withRefs) {
    const refs = effectiveCharacterPreciseReferences(card);
    for (const ref of refs) {
      if (generation.novelaiSettings.director_references.length >= NOVELAI_MAX_DIRECTOR_REFERENCES) {
        skippedFull = true;
        break;
      }
      const base64 = await characterReferenceBase64({
        thumbnail: ref.thumbnail,
        reference: ref.reference,
      });
      if (!base64) {
        failedCount += 1;
        continue;
      }
      if (generation.novelaiSettings.director_references.some((r) => r.image === base64)) {
        duplicateCount += 1;
        continue;
      }
      const finalDescription = normalizeCharacterReferenceDescription(ref.description, {
        supportsOutfits,
        supportsSameOutfit: supportsOutfits,
      });

      generation.addNovelAiDirectorReference(base64, finalDescription, {
        strength: clampCharacterReferenceStrength(ref.strength, strengthMax),
        fidelity: clampCharacterReferenceFidelity(ref.fidelity),
        enabled: true,
      });
      addedBase64s.push(base64);
      if (!addedNameSet.has(card.name)) {
        addedNameSet.add(card.name);
        attachedNames.push(card.name);
      }
    }
    if (skippedFull) break;
  }

  return {
    addedBase64s,
    attachedNames,
    skippedVibes: false,
    skippedFull,
    duplicateCount,
    failedCount,
  };
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
