/**
 * Shared save/update flow after the character extraction review modal closes.
 * Used by both CharactersPanel (from prompt or image) and InterrogateModal.
 */
import { architectureLabel, savedCharacters, type NewCharacter, type SavedCharacter } from "../stores/savedCharacters.svelte.js";
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
      .filter((n) => n.keep && (n.action ?? "new") === "new" && n.item.prompt.trim())
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

  const variants: NewCharacter[] = [
    ...decisions
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
      ),
    ...named
      .filter(
        (n) =>
          n.keep &&
          n.action === "variant" &&
          n.targetId &&
          n.name.trim() &&
          n.item.prompt.trim(),
      )
      .flatMap((n) => {
        const target = savedCharacters.characters.find((c) => c.id === n.targetId);
        if (!target) return [];
        return [
          withImage(
            {
              name: n.name.trim(),
              prompt: n.item.prompt,
              negative: n.item.negative,
              parentId: target.parentId ?? target.id,
            },
            n.useImage,
          ),
        ];
      }),
  ];

  const added = savedCharacters.saveExtracted(fresh, arch);
  const addedVariants = savedCharacters.saveExtracted(variants, arch);

  type PreviousSnapshot = {
    id: string;
    prompt: string;
    negative: string;
    thumbnail: string | null;
    reference: string | null;
    textChanged: boolean;
    replaceThumbnail: boolean;
  };
  const previousMap = new Map<string, PreviousSnapshot>();

  const existingUpdates: {
    targetId: string;
    fallbackSaved?: SavedCharacter;
    mode: "update" | "replace";
    prompt: string;
    negative: string;
    useImage?: boolean;
  }[] = [
    ...decisions
      .filter((x) => (x.action === "update" || x.action === "replace") && x.item.prompt.trim())
      .map((x) => ({
        targetId: x.saved.id,
        fallbackSaved: x.saved,
        mode: x.action as "update" | "replace",
        prompt: x.item.prompt,
        negative: x.item.negative,
        useImage: x.useImage,
      })),
    ...named
      .filter(
        (n) =>
          n.keep &&
          (n.action === "update" || n.action === "replace") &&
          Boolean(n.targetId) &&
          Boolean(n.item.prompt.trim()),
      )
      .map((n) => ({
        targetId: n.targetId!,
        mode: n.action as "update" | "replace",
        prompt: n.item.prompt,
        negative: n.item.negative,
        useImage: n.useImage,
      })),
  ];

  setBusy?.(true);
  try {
    for (const item of existingUpdates) {
      const live =
        savedCharacters.characters.find((c) => c.id === item.targetId) ?? item.fallbackSaved;
      if (!live) continue;

      let next: { prompt: string; negative: string };
      if (item.mode === "replace" || !promptAssistant.isAvailable) {
        next = { prompt: item.prompt.trim(), negative: item.negative.trim() };
      } else {
        try {
          const merged = await promptAssistant.updateCharacter(
            live,
            item.prompt,
            item.negative,
            null,
            true,
          );
          next =
            merged === "missing"
              ? { prompt: item.prompt.trim(), negative: item.negative.trim() }
              : merged;
        } catch (e) {
          console.error("Character update failed:", e);
          gallery.showToast(mapLlmError(String(e)), "error");
          continue;
        }
      }

      const textChanged = next.prompt !== live.prompt || next.negative !== live.negative;
      const replaceThumbnail = !!(item.useImage && sourceImages);
      if (!textChanged && !replaceThumbnail) continue;

      const existingSnap = previousMap.get(live.id);
      if (existingSnap) {
        existingSnap.textChanged ||= textChanged;
        existingSnap.replaceThumbnail ||= replaceThumbnail;
      } else {
        previousMap.set(live.id, {
          id: live.id,
          prompt: live.prompt,
          negative: live.negative,
          thumbnail: live.thumbnail,
          reference: live.reference,
          textChanged,
          replaceThumbnail,
        });
      }

      if (textChanged) savedCharacters.update(live.id, next);
      if (replaceThumbnail && sourceImages) {
        savedCharacters.setThumbnail(live.id, sourceImages.thumbnail, sourceImages.reference);
      }
    }
  } finally {
    setBusy?.(false);
  }

  const previous = [...previousMap.values()].filter((p) => p.textChanged || p.replaceThumbnail);

  if (added + addedVariants + previous.length === 0) {
    return { added: 0, addedVariants: 0, updated: 0 };
  }

  if (!hadMatches && previous.length === 0 && addedVariants === 0) {
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
