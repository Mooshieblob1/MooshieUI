/**
 * Shared save/update flow after the character extraction review modal closes.
 * Used by both CharactersPanel (from prompt or image) and InterrogateModal.
 */
import { architectureLabel, savedCharacters, type NewCharacter } from "../stores/savedCharacters.svelte.js";
import { gallery } from "../stores/gallery.svelte.js";
import { locale } from "../stores/locale.svelte.js";
import { promptAssistant } from "../stores/promptAssistant.svelte.js";
import { mapLlmError } from "./llmError.js";
import type { CharacterImages } from "./characterThumbnail.js";
import type {
  DuplicateDecision,
  NewCharacterDecision,
} from "../components/generation/CharacterExtractReviewModal.svelte";

export interface ApplyExtractOptions {
  arch: string;
  decisions: DuplicateDecision[];
  named: NewCharacterDecision[];
  hadMatches: boolean;
  sourceImages?: CharacterImages | null;
  setBusy?: (busy: boolean) => void;
}

export async function applyCharacterExtractDecisions({
  arch,
  decisions,
  named,
  hadMatches,
  sourceImages = null,
  setBusy,
}: ApplyExtractOptions): Promise<{ added: number; addedVariants: number; updated: number }> {
  const withImage = (item: NewCharacter, attach: boolean | undefined): NewCharacter =>
    attach && sourceImages
      ? { ...item, thumbnail: sourceImages.thumbnail, reference: sourceImages.reference }
      : item;

  const fresh: NewCharacter[] = [
    ...named
      .filter((n) => n.keep && n.item.prompt.trim())
      .map((n) =>
        withImage(
          {
            name: n.name.trim() || n.item.name,
            prompt: n.item.prompt,
            negative: n.item.negative,
          },
          n.useImage,
        ),
      ),
    ...decisions
      .filter((d) => d.action === "new" && d.newName.trim() && d.item.prompt.trim())
      .map((d) =>
        withImage(
          {
            name: d.newName.trim(),
            prompt: d.item.prompt,
            negative: d.item.negative,
          },
          d.useImage,
        ),
      ),
  ];

  const variants: NewCharacter[] = decisions
    .filter((d) => d.action === "variant" && d.variantName.trim() && d.item.prompt.trim())
    .map((d) =>
      withImage(
        {
          name: d.variantName.trim(),
          prompt: d.item.prompt,
          negative: d.item.negative,
          parentId: d.saved.parentId ?? d.saved.id,
        },
        d.useImage,
      ),
    );

  const added = savedCharacters.saveExtracted(fresh, arch);
  const addedVariants = savedCharacters.saveExtracted(variants, arch);
  const previous: {
    id: string;
    prompt: string;
    negative: string;
    thumbnail: string | null;
    reference: string | null;
    textChanged: boolean;
    replaceThumbnail: boolean;
  }[] = [];

  setBusy?.(true);
  try {
    for (const d of decisions.filter((x) => x.action === "update" && x.item.prompt.trim())) {
      let next: { prompt: string; negative: string };
      try {
        const merged = await promptAssistant.updateCharacter(d.saved, d.item.prompt, d.item.negative);
        next = merged === "missing" ? { prompt: d.item.prompt, negative: d.item.negative } : merged;
      } catch (e) {
        console.error("Character update failed:", e);
        gallery.showToast(mapLlmError(String(e)), "error");
        continue;
      }
      const textChanged = next.prompt !== d.saved.prompt || next.negative !== d.saved.negative;
      const replaceThumbnail = !!(d.useImage && sourceImages);
      if (!textChanged && !replaceThumbnail) continue;
      previous.push({
        id: d.saved.id,
        prompt: d.saved.prompt,
        negative: d.saved.negative,
        thumbnail: d.saved.thumbnail,
        reference: d.saved.reference,
        textChanged,
        replaceThumbnail,
      });
      if (textChanged) savedCharacters.update(d.saved.id, next);
      if (replaceThumbnail && sourceImages) {
        savedCharacters.setThumbnail(d.saved.id, sourceImages.thumbnail, sourceImages.reference);
      }
    }
  } finally {
    setBusy?.(false);
  }

  if (added + addedVariants + previous.length === 0) {
    return { added: 0, addedVariants: 0, updated: 0 };
  }

  if (!hadMatches) {
    gallery.showToast(
      locale.t("characters.toast.saved", {
        added: String(added),
        updated: "0",
        arch: architectureLabel(arch),
      }),
      "success",
    );
  } else {
    gallery.showToast(
      locale.t("characters.toast.dupes_saved", {
        added: String(added),
        variants: String(addedVariants),
        updated: String(previous.length),
      }),
      "success",
      previous.length > 0
        ? {
            actionLabel: locale.t("characters.undo"),
            onAction: () => {
              for (const p of previous) {
                if (p.textChanged) {
                  savedCharacters.update(p.id, { prompt: p.prompt, negative: p.negative });
                }
                if (p.replaceThumbnail) {
                  savedCharacters.setThumbnail(p.id, p.thumbnail, p.reference);
                }
              }
            },
            durationMs: 8000,
          }
        : false,
    );
  }

  return { added, addedVariants, updated: previous.length };
}
