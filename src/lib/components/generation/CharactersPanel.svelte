<script lang="ts">
  /**
   * Bottom panel Characters tab: characters the prompt assistant pulled out of
   * a prompt, saved per model architecture so only the ones written for the
   * selected model show up.
   */
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery, isVideoImage } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import {
    CARD_SIZE_MAX,
    CARD_SIZE_MIN,
    NOVELAI_ARCHITECTURE,
    architectureLabel,
    architecturePromptStyle,
    characterArchitectures,
    savedCharacters,
    type SavedCharacter,
  } from "../../stores/savedCharacters.svelte.js";
  import {
    attachCharacterPreciseReferences,
    removeAttachedCharacterReferences,
  } from "../../utils/characterPreciseRef.js";
  import { effectiveCharacterPreciseReferences } from "../../utils/characterPreciseRefLogic.js";
  import { PRECISE_REFERENCE_COST } from "../../utils/novelaiCost.js";
  import { NOVELAI_MAX_DIRECTOR_REFERENCES } from "../../utils/novelaiModels.js";
  import {
    interrogateGalleryImage,
    interrogateImage,
    interrogateImagePath,
    readClipboardImageSafe,
    readImageMetadata,
    readImageMetadataBytes,
    readImageMetadataPath,
    saveTextFile,
  } from "../../utils/api.js";
  import { readPngMetadataClientSide } from "../../utils/pngMetadata.js";
  import { imageBytesToBase64, loadOutputImageForGenerationInput } from "../../utils/galleryActions.js";
  import { portal } from "../../utils/portal.js";
  import { mapLlmError } from "../../utils/llmError.js";
  import type { CharacterMergeResult } from "../../utils/characterMerge.js";
  import CharacterMergeModal from "./CharacterMergeModal.svelte";
  import CharacterPortraitModal from "./CharacterPortraitModal.svelte";
  import CharacterEditModal from "./CharacterEditModal.svelte";
  import { memoryContext, type MemoryChange } from "../../utils/characterMemory.js";
  import CharacterExtractReviewModal, {
    type DuplicateDecision,
    type DuplicateMatch,
    type NewCharacterDecision,
  } from "./CharacterExtractReviewModal.svelte";
  import { boxCharacters, type ExtractedCharacter } from "../../utils/characterExtract.js";
  import {
    metadataToExtractSource,
    partitionExtracted,
    tagsToExtractSource,
    type ExtractSourceKind,
  } from "../../utils/characterImageExtract.js";
  import { applyCharacterExtractDecisions } from "../../utils/characterExtractFlow.js";
  import GalleryPickerModal from "../gallery/GalleryPickerModal.svelte";
  import { progress } from "../../stores/progress.svelte.js";
  import type { InterrogationResult, OutputImage } from "../../types/index.js";
  import {
    type CharacterImages,
    blobToCharacterThumbnail,
    currentImageToCharacterThumbnail,
    outputImageToCharacterThumbnail,
  } from "../../utils/characterThumbnail.js";

  type CapturedImageSource =
    | {
        kind: "gallery";
        filename: string;
        images: CharacterImages | null;
        meta: Record<string, string> | null;
      }
    | {
        kind: "bytes";
        base64: string;
        path?: string;
        images: CharacterImages | null;
        meta: Record<string, string> | null;
      };

  let search = $state("");
  let extracting = $state(false);
  let extractPhase = $state<"idle" | "interrogating" | "extracting">("idle");
  let switchingToInterrogator = $state(false);
  let switchingToPromptOnly = $state(false);
  let imagePickerOpen = $state(false);
  let imageGalleryPickerOpen = $state(false);
  let imagePickerDragOver = $state(false);
  let imageFileInput = $state<HTMLInputElement | null>(null);
  /** Card open in the editor, with the tab to start on and memory changes to review. */
  let editor = $state<{
    character: SavedCharacter;
    tab: "details" | "references" | "memory";
    suggestions: MemoryChange[];
  } | null>(null);
  /** Character the prompt assistant is working into the prompt right now. */
  let mergingId = $state<string | null>(null);
  /** Card whose thumbnail options are showing. */
  let thumbMenuId = $state<string | null>(null);
  /** Card a file upload or gallery pick is for. */
  let thumbTargetId = $state<string | null>(null);
  /** Card whose thumbnail is being made right now. */
  let thumbBusyId = $state<string | null>(null);
  let thumbPickerOpen = $state(false);
  /** Card and mode the portrait/reference dialog is open for. */
  let portraitFor = $state<{ character: SavedCharacter; target: "portrait" | "reference" } | null>(null);
  let thumbFileInput = $state<HTMLInputElement | null>(null);

  /** What Extract found, awaiting the user's names and calls before saving. */
  let extractReview = $state<{
    arch: string;
    fresh: ExtractedCharacter[];
    matches: DuplicateMatch[];
    sourceKind?: ExtractSourceKind;
    sourceImages?: CharacterImages | null;
    captured?: CapturedImageSource;
    canPromptOnly?: boolean;
  } | null>(null);

  /** Top-level cards whose variants are showing. */
  let expanded = $state<Set<string>>(new Set());
  /** Card whose extra options (variants, copy) are showing. */
  let moreMenuId = $state<string | null>(null);
  /** Card being copied to another architecture right now. */
  let copyingId = $state<string | null>(null);
  let importInput = $state<HTMLInputElement | null>(null);

  /** Character being refreshed from the prompt right now. */
  let updatingId = $state<string | null>(null);
  let review = $state<{
    character: SavedCharacter;
    before: { prompt: string; negative: string };
    result: CharacterMergeResult;
    boxMode: boolean;
    boxNegative: boolean;
  } | null>(null);

  const architecture = $derived(savedCharacters.currentArchitecture);
  const archLabel = $derived(architecture ? architectureLabel(architecture) : "");

  /**
   * Top-level cards with their variants. While searching, a group shows when
   * its card or any variant matches, with the matching variants open.
   */
  const groups = $derived.by(() => {
    const q = search.toLowerCase().trim();
    const list = savedCharacters.currentCharacters;
    const ids = new Set(list.map((c) => c.id));
    const matches = (c: SavedCharacter) =>
      !q || c.name.toLowerCase().includes(q) || c.prompt.toLowerCase().includes(q);
    const out: { top: SavedCharacter; variants: SavedCharacter[]; open: boolean }[] = [];
    for (const top of list) {
      if (top.parentId && ids.has(top.parentId)) continue;
      const variants = list
        .filter((c) => c.parentId === top.id)
        .sort((a, b) => a.createdAt - b.createdAt);
      if (!q) {
        out.push({ top, variants, open: expanded.has(top.id) });
        continue;
      }
      const hits = variants.filter(matches);
      if (!matches(top) && hits.length === 0) continue;
      out.push({
        top,
        variants: hits.length > 0 ? hits : variants,
        open: hits.length > 0 || expanded.has(top.id),
      });
    }
    return out;
  });

  /** Top-level cards a card could be filed under as a variant. */
  function parentChoices(character: SavedCharacter): SavedCharacter[] {
    return savedCharacters.currentCharacters.filter((c) => !c.parentId && c.id !== character.id);
  }

  function toggleExpanded(id: string) {
    const next = new Set(expanded);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    expanded = next;
  }

  /** More of the prompt fits beside a taller picture. */
  const promptClamp = $derived(
    savedCharacters.cardSize >= 144 ? "line-clamp-6" : savedCharacters.cardSize >= 96 ? "line-clamp-4" : "line-clamp-2",
  );
  const negativeClamp = $derived(savedCharacters.cardSize >= 96 ? "line-clamp-2" : "line-clamp-1");

  /** Whether Use can attach a card's picture as a Precise Reference. */
  const canReference = $derived(
    generation.isNovelAi && generation.supportsNovelAiPreciseReference,
  );

  /**
   * After Use, attach the character's Precise Reference image(s) when opted in
   * on models that take one. Vibe Transfer and Precise Reference cannot go
   * together, so active vibes stay intact.
   */
  async function attachReference(character: SavedCharacter) {
    if (!canReference) return;
    if (!character.usePreciseReference) {
      void removeAttachedCharacterReferences(character.id, {
        legacySources: [character, ...(character.preciseReferences ?? [])],
      });
      return;
    }
    if (effectiveCharacterPreciseReferences(character).length === 0) return;
    const res = await attachCharacterPreciseReferences([character], {
      requireEnabled: true,
      forceReplaceVibes: false,
      autoAttached: true,
    });
    if (res.skippedVibes) {
      gallery.showToast(locale.t("characters.toast.reference_vibes", { name: character.name }), "info");
      return;
    }
    if (res.skippedFull && res.addedBase64s.length === 0 && res.replacedCount === 0) {
      gallery.showToast(locale.t("characters.toast.reference_full", { name: character.name }), "info");
      return;
    }
    if (res.addedBase64s.length > 0) {
      gallery.showToast(
        res.addedBase64s.length > 1
          ? locale.t("characters.references.toast_attached_use", {
              name: character.name,
              count: String(res.addedBase64s.length),
            })
          : locale.t("characters.toast.reference_added", { name: character.name }),
        "success",
      );
    }
  }

  /** Attach the card's Precise Reference image(s) directly without altering the prompt. */
  async function attachReferenceOnly(character: SavedCharacter) {
    moreMenuId = null;
    if (!canReference || effectiveCharacterPreciseReferences(character).length === 0) return;
    const res = await attachCharacterPreciseReferences([character], {
      requireEnabled: false,
      forceReplaceVibes: false,
      autoAttached: false,
    });
    if (res.skippedVibes) {
      gallery.showToast(locale.t("characters.toast.reference_only_vibes", { name: character.name }), "info");
      return;
    }
    if (res.addedBase64s.length > 0) {
      gallery.showToast(
        res.addedBase64s.length > 1
          ? locale.t("characters.references.toast_added_count", {
              name: character.name,
              count: String(res.addedBase64s.length),
            })
          : locale.t("characters.toast.reference_only_added", { name: character.name }),
        "success",
      );
      return;
    }
    if (res.skippedFull && res.replacedCount === 0) {
      gallery.showToast(
        locale.t("characters.toast.reference_only_full", {
          max: String(NOVELAI_MAX_DIRECTOR_REFERENCES),
        }),
        "info",
      );
    } else if (res.duplicateCount > 0 && res.replacedCount === 0) {
      gallery.showToast(locale.t("generation.novelai.reference.toast_duplicate"), "info");
    } else if (res.failedCount > 0 && res.replacedCount === 0) {
      gallery.showToast(locale.t("generation.novelai.reference.toast_failed"), "error");
    }
  }

  /** Start a variant as a copy of the card and open it for editing. */
  function addVariant(character: SavedCharacter) {
    moreMenuId = null;
    const id = savedCharacters.addVariant(character, locale.t("characters.dupes.variant_default"));
    if (!id) return;
    expanded = new Set([...expanded, character.parentId ?? character.id]);
    const created = savedCharacters.characters.find((c) => c.id === id);
    if (created) openEditor(created);
  }

  /**
   * Copy a card to another architecture. Between tag and natural-language
   * models the prompt assistant rewrites it; between two tag models (or with
   * no assistant set up) it goes over as is.
   */
  async function copyTo(character: SavedCharacter, target: string) {
    if (!target || copyingId) return;
    moreMenuId = null;
    const arch = architectureLabel(target);
    const style = architecturePromptStyle(target);
    const rewrite = style !== architecturePromptStyle(character.architecture);
    let text = { prompt: character.prompt, negative: character.negative };
    if (rewrite && promptAssistant.isAvailable) {
      copyingId = character.id;
      try {
        text = await promptAssistant.convertCharacter(
          character,
          architectureLabel(character.architecture),
          arch,
          style,
        );
      } catch (e) {
        console.error("Character copy failed:", e);
        const msg = String(e);
        gallery.showToast(
          msg.includes("invalid_character_convert")
            ? locale.t("characters.toast.copy_failed", { name: character.name, arch })
            : mapLlmError(msg),
          "error",
        );
        return;
      } finally {
        copyingId = null;
      }
    }
    savedCharacters.saveExtracted(
      [
        {
          name: character.name,
          ...text,
          thumbnail: character.thumbnail,
          reference: character.reference,
          memory: savedCharacters.memoryOf(character),
        },
      ],
      target,
    );
    gallery.showToast(
      locale.t(
        rewrite && !promptAssistant.isAvailable ? "characters.toast.copied_as_is" : "characters.toast.copied",
        { name: character.name, arch },
      ),
      "success",
    );
  }

  /** Every saved card, for every architecture, as a JSON file. */
  async function exportCharacters() {
    const count = savedCharacters.characters.length;
    if (count === 0) return;
    const content = savedCharacters.exportJson();
    const filename = "mooshieui-characters.json";
    try {
      if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const path = await save({ defaultPath: filename, filters: [{ name: "JSON", extensions: ["json"] }] });
        if (!path) return;
        await saveTextFile(content, path);
      } else {
        const url = URL.createObjectURL(new Blob([content], { type: "application/json" }));
        const a = document.createElement("a");
        a.href = url;
        a.download = filename;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
      }
      gallery.showToast(locale.t("characters.toast.exported", { count: String(count) }), "success");
    } catch (e) {
      console.error("Character export failed:", e);
      gallery.showToast(String(e), "error");
    }
  }

  async function importCharacters(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    const result = savedCharacters.importJson(await file.text());
    if (!result) {
      gallery.showToast(locale.t("characters.toast.import_invalid"), "error");
      return;
    }
    gallery.showToast(
      locale.t("characters.toast.imported", { added: String(result.added), skipped: String(result.skipped) }),
      result.added > 0 ? "success" : "info",
    );
  }

  /** The positive prompt plus, on NovelAI, every character box that will be sent. */
  function extractionSource(): string {
    const parts = [generation.positivePrompt.trim()];
    if (generation.isNovelAi) {
      generation.activeNovelAiCharacters.forEach((c, i) => {
        parts.push(`Character ${i + 1}: ${c.prompt.trim()}`);
      });
    }
    return parts.filter(Boolean).join("\n");
  }

  /** The negative prompt (UC) plus, on NovelAI, each sent character box's own UC. */
  function extractionNegative(): string {
    const parts = [generation.negativePrompt.trim()];
    if (generation.isNovelAi) {
      generation.activeNovelAiCharacters.forEach((c, i) => {
        const uc = c.negative_prompt.trim();
        if (uc) parts.push(`Character ${i + 1}: ${uc}`);
      });
    }
    return parts.filter(Boolean).join("\n");
  }

  /**
   * On NovelAI with character boxes in use, each box is one character: read
   * them as they are, and only ask the prompt assistant for names. Without an
   * assistant, or if naming fails, they are numbered for the user to name in
   * the review. Null when there are no boxes to read.
   */
  async function charactersFromBoxes(
    saved: SavedCharacter[],
    rawBoxes: readonly { prompt: string; negative: string }[],
  ): Promise<ExtractedCharacter[] | null> {
    const boxes = boxCharacters(
      rawBoxes.map((c) => ({ prompt: c.prompt, negative: c.negative })),
    );
    if (boxes.length === 0) return null;
    let named = boxes;
    if (promptAssistant.isAvailable) {
      try {
        named = await promptAssistant.nameCharacters(boxes, saved);
      } catch (e) {
        // The characters are already right; only their names are missing.
        console.error("Naming extracted characters failed:", e);
      }
    }
    return named.map((c, i) => ({
      ...c,
      name: c.name.trim() || locale.t("characters.extract_box_name", { n: String(i + 1) }),
    }));
  }

  const hasCurrentImage = $derived(
    Boolean(progress.lastOutputImage || gallery.sessionImages.some((image) => !isVideoImage(image))),
  );
  const hasCurrentPrompt = $derived(Boolean(extractionSource()));

  function activePromptBoxes(): { prompt: string; negative: string }[] {
    return generation.isNovelAi
      ? generation.activeNovelAiCharacters.map((c) => ({
          prompt: c.prompt,
          negative: c.negative_prompt,
        }))
      : [];
  }

  function handleExtractError(e: unknown) {
    console.error("Character extraction failed:", e);
    const msg = String(e);
    gallery.showToast(
      msg.includes("invalid_character_extract")
        ? locale.t("characters.toast.invalid_reply")
        : mapLlmError(msg),
      "error",
    );
  }

  function finishExtracted(
    found: ExtractedCharacter[],
    saved: SavedCharacter[],
    opts: {
      arch: string;
      sourceKind: ExtractSourceKind;
      sourceImages?: CharacterImages | null;
      captured?: CapturedImageSource;
      canPromptOnly?: boolean;
      clearOnEmpty?: boolean;
    },
  ) {
    if (found.length === 0) {
      if (opts.clearOnEmpty) extractReview = null;
      gallery.showToast(locale.t("characters.toast.none_found"), "info");
      return;
    }
    const { matches, fresh } = partitionExtracted(found, saved);
    if (fresh.length === 0 && matches.length === 0) {
      if (opts.clearOnEmpty) extractReview = null;
      gallery.showToast(locale.t("characters.toast.already_saved"), "info");
      return;
    }
    extractReview = {
      arch: opts.arch,
      fresh,
      matches,
      sourceKind: opts.sourceKind,
      sourceImages: opts.sourceImages ?? null,
      captured: opts.captured,
      canPromptOnly: opts.canPromptOnly ?? false,
    };
  }

  async function performPromptExtraction(
    arch: string,
    opts?: {
      sourceImages?: CharacterImages | null;
      captured?: CapturedImageSource;
      clearOnEmpty?: boolean;
    },
  ) {
    const activeBoxes = activePromptBoxes();
    const hasBoxes = activeBoxes.some((c) => c.prompt.trim());
    const source = extractionSource();
    if (!source) {
      gallery.showToast(locale.t("characters.toast.empty_prompt"), "error");
      return;
    }
    const negative = extractionNegative();
    const saved = savedCharacters.currentCharacters;
    const targetStyle = architecturePromptStyle(arch);
    const found =
      (hasBoxes ? await charactersFromBoxes(saved, activeBoxes) : null) ??
      (await promptAssistant.extractCharacters(source, negative, saved, targetStyle));
    finishExtracted(found, saved, {
      arch,
      sourceKind: "prompt",
      sourceImages: opts?.sourceImages,
      captured: opts?.captured,
      clearOnEmpty: opts?.clearOnEmpty,
    });
  }

  async function extractPromptOnly(opts?: {
    asSwitch?: boolean;
    sourceImages?: CharacterImages | null;
    captured?: CapturedImageSource;
  }) {
    const arch = architecture;
    if (!arch) return;
    imagePickerOpen = false;
    const hasBoxes = activePromptBoxes().some((c) => c.prompt.trim());
    if (!hasBoxes && !promptAssistant.isAvailable) {
      promptAssistant.setupModalOpen = true;
      return;
    }
    if (!extractionSource()) {
      gallery.showToast(locale.t("characters.toast.empty_prompt"), "error");
      return;
    }
    if (opts?.asSwitch) {
      if (switchingToPromptOnly || switchingToInterrogator) return;
      switchingToPromptOnly = true;
    } else {
      if (extracting) return;
      extracting = true;
      extractPhase = "extracting";
    }
    try {
      await performPromptExtraction(arch, {
        sourceImages: opts?.sourceImages,
        captured: opts?.captured,
        clearOnEmpty: opts?.asSwitch,
      });
    } catch (e) {
      handleExtractError(e);
    } finally {
      if (opts?.asSwitch) {
        switchingToPromptOnly = false;
      } else {
        extracting = false;
        extractPhase = "idle";
      }
    }
  }

  async function extract() {
    const arch = architecture;
    if (!arch || extracting) return;
    const hasPrompt = Boolean(extractionSource());
    const hasCurrent = hasCurrentImage;

    if (!hasPrompt && !hasCurrent) {
      if (!promptAssistant.isAvailable) {
        promptAssistant.setupModalOpen = true;
      } else {
        imagePickerOpen = true;
      }
      return;
    }
    if (!hasPrompt && hasCurrent) {
      await extractFromCurrentImage();
      return;
    }
    if (hasPrompt && !hasCurrent) {
      await extractPromptOnly();
      return;
    }

    const activeBoxes = activePromptBoxes();
    const hasBoxes = activeBoxes.some((c) => c.prompt.trim());
    if (!hasBoxes && !promptAssistant.isAvailable) {
      promptAssistant.setupModalOpen = true;
      return;
    }

    extracting = true;
    extractPhase = hasBoxes ? "extracting" : "interrogating";
    try {
      const captured = await captureCurrentImage({ quietNoCurrent: true }).catch(() => null);
      if (!hasBoxes && captured) {
        const interrogated = await interrogateCaptured(captured).catch(() => null);
        const tagSrc = interrogated ? tagsToExtractSource(interrogated) : null;
        if (tagSrc) {
          extractPhase = "extracting";
          const source = extractionSource();
          const negative = extractionNegative();
          const saved = savedCharacters.currentCharacters;
          const targetStyle = architecturePromptStyle(arch);
          const found = await promptAssistant.extractCharacters(
            source,
            negative,
            saved,
            targetStyle,
            tagSrc.prompt,
          );
          finishExtracted(found, saved, {
            arch,
            sourceKind: "combined",
            sourceImages: captured.images,
            captured,
            canPromptOnly: true,
          });
          return;
        }
      }
      extractPhase = "extracting";
      await performPromptExtraction(arch, {
        sourceImages: captured?.images ?? null,
        captured: captured ?? undefined,
      });
    } catch (e) {
      handleExtractError(e);
    } finally {
      extracting = false;
      extractPhase = "idle";
    }
  }

  function openImagePicker() {
    if (!architecture || extracting) return;
    imagePickerOpen = true;
  }

  async function readMetaFromBytes(
    uint8: Uint8Array,
    rawBuffer?: ArrayBuffer,
  ): Promise<Record<string, string> | null> {
    const serverMeta = await readImageMetadataBytes(Array.from(uint8)).catch(() => null);
    if (serverMeta && Object.keys(serverMeta).length > 0) return serverMeta;
    const buf = rawBuffer ?? uint8.slice().buffer;
    const clientMeta = await readPngMetadataClientSide(buf).catch(() => null);
    return clientMeta && Object.keys(clientMeta).length > 0 ? clientMeta : null;
  }

  function interrogateCaptured(captured: CapturedImageSource): Promise<InterrogationResult> {
    if (captured.kind === "gallery") return interrogateGalleryImage(captured.filename);
    if (captured.path) return interrogateImagePath(captured.path);
    return interrogateImage(captured.base64);
  }

  async function captureCurrentImage(options?: {
    quietNoCurrent?: boolean;
  }): Promise<CapturedImageSource | null> {
    const latest = gallery.sessionImages.find((image) => !isVideoImage(image)) ?? null;
    const displayUrl = progress.lastOutputImage;
    if (!displayUrl && !latest) {
      if (!options?.quietNoCurrent) {
        gallery.showToast(locale.t("characters.thumbnail.no_current"), "info");
      }
      return null;
    }
    const images = await currentImageToCharacterThumbnail(displayUrl, latest);
    if (latest?.gallery_filename) {
      const meta = await readImageMetadata(latest.gallery_filename).catch(() => null);
      return { kind: "gallery", filename: latest.gallery_filename, images, meta };
    }
    let uint8: Uint8Array | null = null;
    if (latest) {
      const bytes = await gallery.resolveImagePngBytes(latest).catch(() => null);
      if (bytes && bytes.length > 0) {
        uint8 = new Uint8Array(bytes);
      } else {
        const loaded = await loadOutputImageForGenerationInput(latest).catch(() => null);
        if (loaded) uint8 = new Uint8Array(loaded.bytes);
      }
    }
    if (!uint8 && displayUrl) {
      const resp = await fetch(displayUrl).catch(() => null);
      if (resp?.ok) uint8 = new Uint8Array(await resp.arrayBuffer());
    }
    if (!uint8 || uint8.length === 0) return null;
    const meta = await readMetaFromBytes(uint8);
    return {
      kind: "bytes",
      base64: imageBytesToBase64(Array.from(uint8)),
      images,
      meta,
    };
  }

  async function runImageExtraction(captured: CapturedImageSource, forceInterrogator = false) {
    const arch = architecture;
    if (!arch) return;
    if (!forceInterrogator && extracting) return;
    if (forceInterrogator && (switchingToInterrogator || switchingToPromptOnly)) return;

    imagePickerOpen = false;
    imageGalleryPickerOpen = false;
    if (forceInterrogator) {
      switchingToInterrogator = true;
    } else {
      extracting = true;
      extractPhase = "interrogating";
    }
    try {
      const saved = savedCharacters.currentCharacters;
      const targetStyle = architecturePromptStyle(arch);
      let found: ExtractedCharacter[] = [];
      let sourceKind: ExtractSourceKind = "interrogator";

      const metaSrc = forceInterrogator ? null : metadataToExtractSource(captured.meta);
      const interrogated = await interrogateCaptured(captured).catch((err) => {
        if (metaSrc) return null;
        throw err;
      });
      const tagSrc = interrogated ? tagsToExtractSource(interrogated) : null;

      if (!forceInterrogator) extractPhase = "extracting";
      if (metaSrc && tagSrc) {
        sourceKind = "combined";
        found = await promptAssistant.extractCharacters(
          metaSrc.prompt,
          metaSrc.negative,
          saved,
          targetStyle,
          tagSrc.prompt,
        );
      } else if (metaSrc) {
        sourceKind = "metadata";
        if (metaSrc.boxes?.length && targetStyle === "tags") {
          found = (await charactersFromBoxes(saved, metaSrc.boxes)) ?? [];
        } else {
          found = await promptAssistant.extractCharacters(
            metaSrc.prompt,
            metaSrc.negative,
            saved,
            targetStyle,
          );
        }
      } else if (tagSrc) {
        sourceKind = "interrogator";
        found = await promptAssistant.extractCharacters(tagSrc.prompt, "", saved, targetStyle);
      } else if (interrogated?.image_token) {
        sourceKind = "describe";
        const prose = await promptAssistant.describe(interrogated.image_token, "detailed");
        if (prose.trim()) {
          found = await promptAssistant.extractCharacters(prose, "", saved, targetStyle);
        }
      }

      finishExtracted(found, saved, {
        arch,
        sourceKind,
        sourceImages: captured.images,
        captured,
        clearOnEmpty: forceInterrogator,
      });
    } catch (e) {
      handleExtractError(e);
    } finally {
      if (forceInterrogator) {
        switchingToInterrogator = false;
      } else {
        extracting = false;
        extractPhase = "idle";
      }
    }
  }

  async function extractFromCurrentImage() {
    if (!architecture || extracting) return;
    if (!promptAssistant.isAvailable) {
      promptAssistant.setupModalOpen = true;
      return;
    }
    imagePickerOpen = false;
    extracting = true;
    extractPhase = "interrogating";
    let captured: CapturedImageSource | null = null;
    try {
      captured = await captureCurrentImage();
      if (!captured) {
        if (hasCurrentImage) gallery.showToast(locale.t("characters.extract_image_failed"), "error");
        return;
      }
    } catch (e) {
      console.error("Failed to read current image:", e);
      gallery.showToast(locale.t("characters.extract_image_failed"), "error");
      return;
    } finally {
      extracting = false;
      extractPhase = "idle";
    }
    if (captured) await runImageExtraction(captured);
  }

  async function extractFromGalleryImage(picked: OutputImage[]) {
    imageGalleryPickerOpen = false;
    const image = picked[0];
    if (!image || !architecture || extracting) return;
    if (!promptAssistant.isAvailable) {
      promptAssistant.setupModalOpen = true;
      return;
    }
    extracting = true;
    extractPhase = "interrogating";
    let captured: CapturedImageSource | null = null;
    try {
      const images = await outputImageToCharacterThumbnail(image).catch(() => null);
      if (image.gallery_filename) {
        const meta = await readImageMetadata(image.gallery_filename).catch(() => null);
        captured = { kind: "gallery", filename: image.gallery_filename, images, meta };
      } else {
        const bytes = await gallery.resolveImagePngBytes(image);
        const uint8 = new Uint8Array(bytes);
        const meta = await readMetaFromBytes(uint8);
        captured = {
          kind: "bytes",
          base64: imageBytesToBase64(Array.from(uint8)),
          images,
          meta,
        };
      }
    } catch (e) {
      console.error("Failed to load gallery image:", e);
      gallery.showToast(locale.t("characters.extract_image_failed"), "error");
      return;
    } finally {
      extracting = false;
      extractPhase = "idle";
    }
    if (captured) await runImageExtraction(captured);
  }

  async function extractFromBlobBuffer(uint8: Uint8Array, rawBuffer?: ArrayBuffer, path?: string) {
    if (!architecture || extracting) return;
    if (!promptAssistant.isAvailable) {
      promptAssistant.setupModalOpen = true;
      return;
    }
    imagePickerOpen = false;
    extracting = true;
    extractPhase = "interrogating";
    let captured: CapturedImageSource | null = null;
    try {
      const images = await blobToCharacterThumbnail(new Blob([uint8 as unknown as BlobPart]));
      const meta = path
        ? await readImageMetadataPath(path).catch(() => readMetaFromBytes(uint8, rawBuffer))
        : await readMetaFromBytes(uint8, rawBuffer);
      captured = {
        kind: "bytes",
        base64: imageBytesToBase64(Array.from(uint8)),
        ...(path ? { path } : {}),
        images,
        meta,
      };
    } catch (e) {
      console.error("Failed to read image file:", e);
      gallery.showToast(locale.t("characters.extract_image_failed"), "error");
      return;
    } finally {
      extracting = false;
      extractPhase = "idle";
    }
    if (captured) await runImageExtraction(captured);
  }

  async function handleExtractFileList(files: FileList | null) {
    const file = files?.[0];
    if (imageFileInput) imageFileInput.value = "";
    if (!file) return;
    const buf = await file.arrayBuffer();
    await extractFromBlobBuffer(new Uint8Array(buf), buf);
  }

  async function handleExtractClipboard(clipboardItems?: DataTransferItemList) {
    if (clipboardItems) {
      for (const item of Array.from(clipboardItems)) {
        if (item.type.startsWith("image/")) {
          const file = item.getAsFile();
          if (file) {
            const buf = await file.arrayBuffer();
            await extractFromBlobBuffer(new Uint8Array(buf), buf);
            return;
          }
        }
      }
    }
    try {
      const bytes = await readClipboardImageSafe();
      if (!bytes || bytes.length === 0) {
        gallery.showToast(locale.t("common.no_clipboard_image"), "info");
        return;
      }
      await extractFromBlobBuffer(new Uint8Array(bytes));
    } catch {
      gallery.showToast(locale.t("common.no_clipboard_image"), "info");
    }
  }

  async function handleTauriImageDrop(paths: string[]) {
    const p = paths[0];
    if (!p) return;
    try {
      const { readFile } = await import("@tauri-apps/plugin-fs");
      const bytes = await readFile(p);
      await extractFromBlobBuffer(new Uint8Array(bytes), undefined, p);
    } catch (e) {
      console.error("Failed to read dropped image:", e);
      gallery.showToast(locale.t("characters.extract_image_failed"), "error");
    }
  }

  function tauriDropImagePicker(el: HTMLElement) {
    const handler = (event: Event) => {
      const detail = (event as CustomEvent<{ path?: string; paths?: string[] }>).detail;
      const paths = detail?.paths?.length ? detail.paths : detail?.path ? [detail.path] : [];
      if (paths.length > 0) void handleTauriImageDrop(paths);
    };
    el.addEventListener("tauri-file-drop", handler);
    return { destroy: () => el.removeEventListener("tauri-file-drop", handler) };
  }

  $effect(() => {
    if (!imagePickerOpen) return;
    const pasteHandler = async (event: ClipboardEvent) => {
      const el = event.target as HTMLElement | null;
      if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable)) {
        return;
      }
      event.preventDefault();
      event.stopImmediatePropagation();
      await handleExtractClipboard(event.clipboardData?.items);
    };
    const keyHandler = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        imagePickerOpen = false;
      }
    };
    window.addEventListener("paste", pasteHandler, { capture: true });
    window.addEventListener("keydown", keyHandler);
    return () => {
      window.removeEventListener("paste", pasteHandler, { capture: true });
      window.removeEventListener("keydown", keyHandler);
    };
  });

  /**
   * Carry out the extract review: new characters are saved under the names
   * the user gave them (an emptied name falls back to the suggested one),
   * variants become cards, a wrong match saved as new becomes a card of its
   * own, and each Update merges what the review shows (as the user left it)
   * into the saved card. One Undo covers every update.
   */
  async function applyExtractReview(decisions: DuplicateDecision[], named: NewCharacterDecision[]) {
    const pending = extractReview;
    extractReview = null;
    if (!pending) return;
    await applyCharacterExtractDecisions({
      arch: pending.arch,
      decisions,
      named,
      hadMatches: pending.matches.length > 0,
      sourceImages: pending.sourceImages ?? null,
      setBusy: (busy) => {
        extracting = busy;
      },
    });
  }

  /**
   * Have the prompt assistant work the character into the prompt, then review
   * it. Without an assistant, or with nothing in the prompt to merge into,
   * there is nothing to resolve, so it inserts the saved text as is.
   */
  async function use(character: SavedCharacter) {
    if (mergingId) return;
    if (savedCharacters.isInPrompt(character)) {
      gallery.showToast(locale.t("characters.toast.already_in_prompt", { name: character.name }), "info");
      return;
    }
    if (!promptAssistant.isAvailable || !generation.positivePrompt.trim()) {
      insertPlain(character);
      return;
    }
    const target = savedCharacters.boxTarget();
    const before = { prompt: generation.positivePrompt, negative: generation.negativePrompt };
    mergingId = character.id;
    try {
      const result = await promptAssistant.mergeCharacter({
        prompt: before.prompt,
        negative: before.negative,
        character,
        box: target
          ? {
              boxes: generation.activeNovelAiCharacters.map((c) => c.prompt),
              boxNegative: target.boxNegative,
            }
          : null,
        memory: memoryContext(savedCharacters.memoryOf(character)),
      });
      review = {
        character,
        before,
        result,
        boxMode: target !== null,
        boxNegative: target?.boxNegative ?? false,
      };
    } catch (e) {
      console.error("Character merge failed:", e);
      const msg = String(e);
      gallery.showToast(
        msg.includes("invalid_character_merge")
          ? locale.t("characters.toast.merge_invalid")
          : mapLlmError(msg),
        "error",
      );
    } finally {
      mergingId = null;
    }
  }

  function applyReview(fields: { prompt?: string; negative?: string; character?: string; characterNegative?: string }) {
    const r = review;
    if (!r) return;
    review = null;
    // The merge was written against the prompt as it was; applying it over
    // edits made since would silently undo them.
    if (generation.positivePrompt !== r.before.prompt || generation.negativePrompt !== r.before.negative) {
      gallery.showToast(locale.t("characters.toast.prompt_changed", { name: r.character.name }), "error");
      return;
    }
    savedCharacters.applyMerge(fields);
    gallery.showToast(
      locale.t(
        r.boxMode && fields.character ? "characters.toast.added_character_box" : "characters.toast.added_prompt",
        { name: r.character.name },
      ),
      "success",
    );
    void attachReference(r.character);
  }

  function insertPlain(character: SavedCharacter) {
    review = null;
    const result = savedCharacters.insert(character);
    const key =
      result === "duplicate"
        ? "characters.toast.already_in_prompt"
        : result === "novelai_character"
          ? "characters.toast.added_character_box"
          : "characters.toast.added_prompt";
    gallery.showToast(
      locale.t(key, { name: character.name }),
      result === "duplicate" ? "info" : "success",
    );
    if (result !== "duplicate") void attachReference(character);
  }

  /**
   * Refresh the card from what the current prompt says about this character,
   * leaving other characters' details out. Undo restores the old text.
   */
  async function refresh(character: SavedCharacter) {
    if (updatingId || mergingId) return;
    if (!promptAssistant.isAvailable) {
      promptAssistant.setupModalOpen = true;
      return;
    }
    const source = extractionSource();
    if (!source) {
      gallery.showToast(locale.t("characters.toast.empty_prompt"), "error");
      return;
    }
    updatingId = character.id;
    try {
      const update = await promptAssistant.updateCharacter(
        character,
        source,
        extractionNegative(),
        savedCharacters.memoryOf(character),
      );
      if (update === "missing") {
        gallery.showToast(locale.t("characters.toast.update_missing", { name: character.name }), "info");
        return;
      }
      const suggestions = update.memory ?? [];
      const changed = update.prompt !== character.prompt || update.negative !== character.negative;
      if (!changed && suggestions.length === 0) {
        gallery.showToast(locale.t("characters.toast.update_unchanged", { name: character.name }), "info");
        return;
      }
      if (changed) {
        const previous = { prompt: character.prompt, negative: character.negative };
        savedCharacters.update(character.id, { prompt: update.prompt, negative: update.negative });
        gallery.showToast(locale.t("characters.toast.updated", { name: character.name }), "success", {
          actionLabel: locale.t("characters.undo"),
          onAction: () => savedCharacters.update(character.id, previous),
          durationMs: 8000,
        });
      }
      // Lasting facts the prompt showed: reviewed in the editor before they are kept.
      const card = savedCharacters.characters.find((c) => c.id === character.id);
      if (suggestions.length > 0 && card) openEditor(card, "memory", suggestions);
    } catch (e) {
      console.error("Character update failed:", e);
      const msg = String(e);
      gallery.showToast(
        msg.includes("invalid_character_update")
          ? locale.t("characters.toast.invalid_reply")
          : mapLlmError(msg),
        "error",
      );
    } finally {
      updatingId = null;
    }
  }

  /** A portrait for this card is queued or rendering. */
  function portraitBusy(id: string): boolean {
    const ids = savedCharacters.portraitPromptIds(id);
    return ids.length > 0 && progress.pendingPrompts.some((p) => ids.includes(p.promptId));
  }

  /** Run one thumbnail source for a card and store what it produced. */
  async function setThumb(id: string, make: () => Promise<CharacterImages | null>, emptyKey: string) {
    thumbBusyId = id;
    thumbMenuId = null;
    try {
      const images = await make();
      if (images) savedCharacters.setThumbnail(id, images.thumbnail, images.reference);
      else gallery.showToast(locale.t(emptyKey), "error");
    } catch (e) {
      console.error("Character thumbnail failed:", e);
      gallery.showToast(locale.t("characters.thumbnail.failed"), "error");
    } finally {
      thumbBusyId = null;
    }
  }

  function thumbFromCurrent(character: SavedCharacter) {
    const latest = gallery.sessionImages.find((image) => !isVideoImage(image)) ?? null;
    if (!progress.lastOutputImage && !latest) {
      thumbMenuId = null;
      gallery.showToast(locale.t("characters.thumbnail.no_current"), "info");
      return;
    }
    void setThumb(
      character.id,
      () => currentImageToCharacterThumbnail(progress.lastOutputImage, latest),
      "characters.thumbnail.failed",
    );
  }

  function thumbFromFile(files: FileList | null) {
    const id = thumbTargetId;
    const file = files?.[0];
    if (thumbFileInput) thumbFileInput.value = "";
    if (!id || !file) return;
    void setThumb(id, () => blobToCharacterThumbnail(file), "characters.thumbnail.failed");
  }

  function thumbFromGallery(images: OutputImage[]) {
    thumbPickerOpen = false;
    const id = thumbTargetId;
    const image = images[0];
    if (!id || !image) return;
    void setThumb(id, () => outputImageToCharacterThumbnail(image), "characters.thumbnail.failed");
  }

  function openEditor(
    character: SavedCharacter,
    tab: "details" | "references" | "memory" = "details",
    suggestions: MemoryChange[] = [],
  ) {
    moreMenuId = null;
    thumbMenuId = null;
    editor = { character, tab, suggestions };
  }

  /** Whether a card shows its Memory button: the provider reads memory, or it has some. */
  function showsMemory(character: SavedCharacter): boolean {
    return promptAssistant.canUseMemory || savedCharacters.memoryOf(character).length > 0;
  }

  function remove(character: SavedCharacter) {
    if (!confirm(locale.t("characters.delete_confirm", { name: character.name }))) return;
    savedCharacters.remove(character.id);
  }
</script>

{#snippet card(character: SavedCharacter)}
  <div class="rounded-lg border border-neutral-800 bg-neutral-900/60 p-1.5 flex flex-col gap-1 min-w-0">
    <div class="flex gap-2 min-w-0">
    <button
      type="button"
      style="width: {savedCharacters.cardSize}px; height: {savedCharacters.cardSize}px;"
      class="shrink-0 rounded-md overflow-hidden border flex items-center justify-center transition-colors {thumbMenuId === character.id ? 'border-indigo-500' : 'border-neutral-700 hover:border-neutral-500'} bg-neutral-800 text-neutral-500"
      title={locale.t("characters.thumbnail.change")}
      aria-label={locale.t("characters.thumbnail.change")}
      aria-expanded={thumbMenuId === character.id}
      onclick={() => { thumbMenuId = thumbMenuId === character.id ? null : character.id; }}
    >
      {#if thumbBusyId === character.id || portraitBusy(character.id)}
        <svg class="w-4 h-4 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 12a9 9 0 1 1-6.22-8.56" stroke-linecap="round"/></svg>
      {:else if character.thumbnail}
        <img
          src={savedCharacters.cardSize > 96 ? (character.reference ?? character.thumbnail) : character.thumbnail}
          alt={character.name}
          class="w-full h-full object-cover"
        />
      {:else}
        <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>
      {/if}
    </button>
    <div class="flex-1 min-w-0 flex flex-col gap-0.5">
      <span class="truncate text-xs font-medium text-neutral-100" title={character.name}>{character.name}</span>
      <p class="text-[11px] leading-snug text-neutral-400 {promptClamp} break-words" title={character.prompt}>{character.prompt}</p>
      {#if character.negative}
        <p class="text-[11px] leading-snug text-red-300/70 {negativeClamp} break-words" title={character.negative}>
          <span class="text-red-400/80 font-medium">{locale.t("characters.negative_prefix")}</span> {character.negative}
        </p>
      {/if}
    </div>
    </div>
    <div class="flex items-center gap-1 min-w-0">
      <button
        type="button"
        class="shrink-0 h-5 px-2 text-[11px] rounded bg-indigo-600/80 hover:bg-indigo-500 text-white disabled:opacity-40 disabled:cursor-not-allowed flex items-center gap-1"
        title={locale.t(promptAssistant.isAvailable ? "characters.use_smart_tip" : "characters.use_tip")}
        disabled={mergingId !== null || updatingId !== null}
        onclick={() => use(character)}
      >
        {#if mergingId === character.id}
          <svg class="w-3 h-3 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 12a9 9 0 1 1-6.22-8.56" stroke-linecap="round"/></svg>
          {locale.t("characters.merging")}
        {:else}
          {locale.t("characters.use")}
        {/if}
      </button>
      <button
        type="button"
        class="shrink-0 w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800"
        title={locale.t("characters.insert_plain_tip")}
        aria-label={locale.t("characters.insert_plain")}
        onclick={() => insertPlain(character)}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
      </button>
      <button
        type="button"
        class="shrink-0 w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed"
        title={locale.t("characters.update_tip")}
        aria-label={locale.t("characters.update")}
        disabled={updatingId !== null || mergingId !== null}
        onclick={() => refresh(character)}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3 {updatingId === character.id ? 'animate-spin' : ''}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/></svg>
      </button>
      <button
        type="button"
        class="shrink-0 w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800"
        title={locale.t("characters.edit")}
        aria-label={locale.t("characters.edit")}
        onclick={() => openEditor(character)}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9"/><path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z"/></svg>
      </button>
      {#if canReference || character.architecture === NOVELAI_ARCHITECTURE}
        {@const refCount = effectiveCharacterPreciseReferences(character).length}
        {@const dedicatedCount = character.preciseReferences?.length ?? 0}
        {@const prOn = Boolean(character.usePreciseReference) && refCount > 0}
        <button
          type="button"
          class="shrink-0 h-5 px-1.5 flex items-center justify-center gap-0.5 rounded text-[10px] font-medium transition-colors {prOn
            ? 'border border-indigo-500/50 bg-indigo-500/20 text-indigo-200 hover:bg-indigo-500/30'
            : 'text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800'}"
          title={prOn
            ? locale.t("characters.references.pill_on_tip", {
                name: character.name,
                count: String(refCount),
                anlas: String(refCount * PRECISE_REFERENCE_COST),
              })
            : refCount > 0
              ? locale.t("characters.references.pill_off_tip", {
                  name: character.name,
                  count: String(refCount),
                })
              : locale.t("characters.references.pill_empty_tip")}
          aria-pressed={prOn}
          onclick={() => {
            if (refCount > 0) savedCharacters.setUsePreciseReference(character.id, !character.usePreciseReference);
            else openEditor(character, "references");
          }}
        >
          <span>{locale.t("characters.references.pill_label")}</span>
          {#if dedicatedCount > 1 || (prOn && refCount > 1)}
            <span class="tabular-nums opacity-85">{refCount}</span>
          {/if}
        </button>
      {/if}
      {#if showsMemory(character)}
        {@const memoryCount = savedCharacters.memoryOf(character).length}
        <button
          type="button"
          class="shrink-0 h-5 min-w-5 px-1 flex items-center justify-center gap-0.5 rounded hover:bg-neutral-800 {memoryCount > 0 ? 'text-indigo-300 hover:text-indigo-200' : 'text-neutral-500 hover:text-neutral-200'}"
          title={locale.t("characters.memory.tip")}
          aria-label={locale.t("characters.memory.open")}
          onclick={() => openEditor(character, "memory")}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="8" y1="13" x2="16" y2="13"/><line x1="8" y1="17" x2="13" y2="17"/></svg>
          {#if memoryCount > 0}<span class="text-[10px] leading-none tabular-nums">{memoryCount}</span>{/if}
        </button>
      {/if}
      <button
        type="button"
        class="shrink-0 w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800 {moreMenuId === character.id ? 'bg-neutral-800 text-neutral-200' : ''}"
        title={locale.t("characters.more")}
        aria-label={locale.t("characters.more")}
        aria-expanded={moreMenuId === character.id}
        onclick={() => { moreMenuId = moreMenuId === character.id ? null : character.id; }}
      >
        {#if copyingId === character.id}
          <svg class="w-3 h-3 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 12a9 9 0 1 1-6.22-8.56" stroke-linecap="round"/></svg>
        {:else}
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="currentColor"><circle cx="5" cy="12" r="1.8"/><circle cx="12" cy="12" r="1.8"/><circle cx="19" cy="12" r="1.8"/></svg>
        {/if}
      </button>
      <span class="flex-1"></span>
      <button
        type="button"
        class="shrink-0 w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-red-300 hover:bg-red-600/10"
        title={locale.t("characters.delete")}
        aria-label={locale.t("characters.delete")}
        onclick={() => remove(character)}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
      </button>
    </div>
    {#if moreMenuId === character.id}
      <div class="flex flex-wrap items-center gap-1.5 text-[11px]">
        <button
          type="button"
          class="px-2 py-0.5 rounded border border-neutral-700 text-neutral-300 hover:bg-neutral-800"
          onclick={() => addVariant(character)}
        >{locale.t("characters.variants.add")}</button>
        {#if canReference || character.architecture === NOVELAI_ARCHITECTURE}
          <button
            type="button"
            class="px-2 py-0.5 rounded border border-neutral-700 text-neutral-300 hover:bg-neutral-800"
            onclick={() => openEditor(character, "references")}
          >{locale.t("characters.editor.references")}</button>
          <button
            type="button"
            class="px-2 py-0.5 rounded border border-neutral-700 text-neutral-300 hover:bg-neutral-800"
            onclick={() => { moreMenuId = null; portraitFor = { character, target: "reference" }; }}
          >{locale.t("characters.references.generate_button")}</button>
        {/if}
        {#if canReference && effectiveCharacterPreciseReferences(character).length > 0}
          <button
            type="button"
            class="px-2 py-0.5 rounded border border-indigo-500/50 text-indigo-300 hover:bg-indigo-500/15"
            onclick={() => void attachReferenceOnly(character)}
          >{locale.t("characters.use_as_reference")}</button>
        {/if}
        <label class="flex items-center gap-1 text-neutral-400">
          {locale.t("characters.variants.of")}
          <select
            class="max-w-[9rem] bg-neutral-800 border border-neutral-700 rounded px-1 py-0.5 text-[11px] text-neutral-200 focus:outline-none focus:border-indigo-500"
            value={character.parentId ?? ""}
            onchange={(e) => {
              savedCharacters.setParent(character.id, (e.currentTarget as HTMLSelectElement).value || null);
              moreMenuId = null;
            }}
          >
            <option value="">{locale.t("characters.variants.none")}</option>
            {#each parentChoices(character) as parent (parent.id)}
              <option value={parent.id}>{parent.name}</option>
            {/each}
          </select>
        </label>
        <label class="flex items-center gap-1 text-neutral-400">
          {locale.t("characters.copy_to")}
          <select
            class="max-w-[9rem] bg-neutral-800 border border-neutral-700 rounded px-1 py-0.5 text-[11px] text-neutral-200 focus:outline-none focus:border-indigo-500 disabled:opacity-40"
            value=""
            disabled={copyingId !== null}
            onchange={(e) => copyTo(character, (e.currentTarget as HTMLSelectElement).value)}
          >
            <option value="">{locale.t("characters.copy_to_placeholder")}</option>
            {#each characterArchitectures().filter((a) => a !== character.architecture) as arch (arch)}
              <option value={arch}>{architectureLabel(arch)}</option>
            {/each}
          </select>
        </label>
      </div>
    {/if}
    {#if thumbMenuId === character.id}
      <div class="flex flex-wrap gap-1">
        <button
          type="button"
          class="px-2 py-0.5 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:bg-neutral-800"
          title={locale.t("characters.thumbnail.portrait_tip")}
          onclick={() => { thumbMenuId = null; portraitFor = { character, target: "portrait" }; }}
        >{locale.t("characters.thumbnail.portrait")}</button>
        {#if canReference || character.architecture === NOVELAI_ARCHITECTURE}
          <button
            type="button"
            class="px-2 py-0.5 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:bg-neutral-800"
            onclick={() => { thumbMenuId = null; portraitFor = { character, target: "reference" }; }}
          >{locale.t("characters.references.generate_button")}</button>
        {/if}
        <button
          type="button"
          class="px-2 py-0.5 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:bg-neutral-800"
          onclick={() => thumbFromCurrent(character)}
        >{locale.t("characters.thumbnail.current")}</button>
        <button
          type="button"
          class="px-2 py-0.5 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:bg-neutral-800"
          onclick={() => { thumbTargetId = character.id; thumbFileInput?.click(); }}
        >{locale.t("characters.thumbnail.upload")}</button>
        <button
          type="button"
          class="px-2 py-0.5 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:bg-neutral-800"
          onclick={() => { thumbTargetId = character.id; thumbPickerOpen = true; }}
        >{locale.t("characters.thumbnail.gallery")}</button>
        {#if character.thumbnail}
          <button
            type="button"
            class="px-2 py-0.5 text-[11px] rounded border border-neutral-700 text-neutral-400 hover:border-red-500 hover:text-red-300"
            onclick={() => { savedCharacters.setThumbnail(character.id, null); thumbMenuId = null; }}
          >{locale.t("characters.thumbnail.remove")}</button>
        {/if}
      </div>
    {/if}
  </div>
{/snippet}

<div class="flex flex-col h-full">
  <div class="px-2 pt-1.5 pb-1 shrink-0 flex items-center gap-2">
    <input
      type="text"
      name="saved-character-search"
      bind:value={search}
      placeholder={locale.t("characters.search_placeholder")}
      disabled={!architecture}
      class="flex-1 min-w-0 bg-neutral-800 border border-neutral-700 rounded px-2.5 py-1 text-xs text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors disabled:opacity-50"
    />
    {#if architecture}
      <span
        class="shrink-0 text-[10px] px-1.5 py-0.5 rounded bg-neutral-800 border border-neutral-700 text-neutral-400"
        title={locale.t("characters.arch_tip", { arch: archLabel })}
      >
        {archLabel}
      </span>
    {/if}
    <label class="shrink-0 flex items-center gap-1 text-neutral-500" title={locale.t("characters.card_size")}>
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/></svg>
      <input
        type="range"
        name="saved-character-card-size"
        min={CARD_SIZE_MIN}
        max={CARD_SIZE_MAX}
        step="8"
        value={savedCharacters.cardSize}
        oninput={(e) => savedCharacters.setCardSize(Number((e.currentTarget as HTMLInputElement).value))}
        aria-label={locale.t("characters.card_size")}
        disabled={!architecture}
        class="w-16 sm:w-20 accent-indigo-500 disabled:opacity-40"
      />
    </label>
    <button
      type="button"
      class="shrink-0 w-6 h-6 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800"
      title={locale.t("characters.import_tip")}
      aria-label={locale.t("characters.import")}
      onclick={() => importInput?.click()}
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
    </button>
    <button
      type="button"
      class="shrink-0 w-6 h-6 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed"
      title={locale.t("characters.export_tip")}
      aria-label={locale.t("characters.export")}
      disabled={savedCharacters.characters.length === 0}
      onclick={exportCharacters}
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
    </button>
    <div class="inline-flex items-stretch shrink-0">
      <button
        type="button"
        class="px-2.5 py-1 text-[11px] font-medium rounded-l bg-indigo-600 hover:bg-indigo-500 text-white transition-colors disabled:opacity-40 disabled:cursor-not-allowed flex items-center gap-1.5"
        onclick={extract}
        disabled={!architecture || extracting || (promptAssistant.isGenerating && !extracting)}
        title={locale.t("characters.extract_tip")}
      >
        {#if extracting}
          <svg class="w-3 h-3 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 12a9 9 0 1 1-6.22-8.56" stroke-linecap="round"/></svg>
          {promptAssistant.stage === "loading_model"
            ? locale.t("prompt_assistant.loading_model")
            : extractPhase === "interrogating"
              ? locale.t("characters.switching_to_interrogator")
              : locale.t("characters.extracting")}
        {:else}
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>
          {locale.t("characters.extract")}
        {/if}
      </button>
      <button
        type="button"
        class="px-1.5 py-1 rounded-r bg-indigo-600 hover:bg-indigo-500 border-l border-indigo-400/40 text-white transition-colors disabled:opacity-40 disabled:cursor-not-allowed flex items-center justify-center"
        onclick={openImagePicker}
        disabled={!architecture || extracting || (promptAssistant.isGenerating && !extracting)}
        title={locale.t("characters.extract_more")}
        aria-label={locale.t("characters.extract_more")}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
      </button>
    </div>
  </div>

  {#if !architecture}
    <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs px-4 text-center">
      <p>{locale.t("characters.no_architecture")}</p>
    </div>
  {:else if savedCharacters.currentCharacters.length === 0}
    <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs px-4 text-center">
      <p>{locale.t("characters.empty", { arch: archLabel })}</p>
    </div>
  {:else if groups.length === 0}
    <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs">
      <p>{locale.t("characters.no_results")}</p>
    </div>
  {:else}
    <div class="flex-1 min-h-0 overflow-y-auto [scrollbar-gutter:stable] px-2 py-2">
      <div class="grid gap-2" style="grid-template-columns: repeat(auto-fill, minmax(min({124 + savedCharacters.cardSize * 2}px, 100%), 1fr)); align-content: start;">
        {#each groups as group (group.top.id)}
          <div class="flex flex-col gap-1.5 min-w-0">
            {@render card(group.top)}
            {#if group.variants.length > 0}
              <button
                type="button"
                class="self-start flex items-center gap-1 px-1.5 py-0.5 text-[10px] rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800"
                aria-expanded={group.open}
                onclick={() => toggleExpanded(group.top.id)}
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="w-2.5 h-2.5 transition-transform {group.open ? 'rotate-90' : ''}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
                {locale.t("characters.variants.count", { count: String(group.variants.length) })}
              </button>
              {#if group.open}
                <div class="ml-3 pl-2 border-l border-neutral-800 flex flex-col gap-1.5">
                  {#each group.variants as variant (variant.id)}
                    {@render card(variant)}
                  {/each}
                </div>
              {/if}
            {/if}
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

{#if extractReview}
  <CharacterExtractReviewModal
    matches={extractReview.matches}
    fresh={extractReview.fresh}
    sourceKind={extractReview.sourceKind}
    sourcePreviewUrl={extractReview.sourceImages?.thumbnail ?? null}
    canSwitchToInterrogator={Boolean(extractReview.captured)}
    {switchingToInterrogator}
    onSwitchToInterrogator={extractReview.captured
      ? () => void runImageExtraction(extractReview!.captured!, true)
      : undefined}
    canSwitchToPromptOnly={Boolean(extractReview.canPromptOnly)}
    {switchingToPromptOnly}
    onSwitchToPromptOnly={extractReview.canPromptOnly
      ? () =>
          void extractPromptOnly({
            asSwitch: true,
            sourceImages: extractReview?.sourceImages ?? null,
            captured: extractReview?.captured,
          })
      : undefined}
    onsave={applyExtractReview}
    onclose={() => { extractReview = null; }}
  />
{/if}

{#if editor}
  {#key editor}
    <CharacterEditModal
      character={editor.character}
      tab={editor.tab}
      suggestions={editor.suggestions}
      ongenerateref={(c) => { portraitFor = { character: c, target: "reference" }; }}
      onclose={() => { editor = null; }}
    />
  {/key}
{/if}

{#if portraitFor}
  <CharacterPortraitModal
    character={portraitFor.character}
    target={portraitFor.target}
    onclose={() => { portraitFor = null; }}
  />
{/if}

{#if review}
  <CharacterMergeModal
    name={review.character.name}
    before={review.before}
    result={review.result}
    boxMode={review.boxMode}
    boxNegative={review.boxNegative}
    onapply={applyReview}
    oninsertplain={() => { if (review) insertPlain(review.character); }}
    onclose={() => { review = null; }}
  />
{/if}

<input
  bind:this={thumbFileInput}
  type="file"
  accept="image/*"
  class="hidden"
  aria-hidden="true"
  tabindex="-1"
  onchange={(e) => thumbFromFile((e.currentTarget as HTMLInputElement).files)}
/>

<GalleryPickerModal
  open={thumbPickerOpen}
  title={locale.t("characters.thumbnail.pick_title")}
  onselect={thumbFromGallery}
  onclose={() => { thumbPickerOpen = false; }}
/>

<input
  bind:this={importInput}
  type="file"
  accept="application/json,.json"
  class="hidden"
  onchange={importCharacters}
/>

<GalleryPickerModal
  open={imageGalleryPickerOpen}
  title={locale.t("characters.extract_from_image_gallery_title")}
  onselect={(images) => void extractFromGalleryImage(images)}
  onclose={() => { imageGalleryPickerOpen = false; }}
/>

{#if imagePickerOpen}
  <!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
  <div
    use:portal
    class="fixed inset-0 z-210 bg-black/75 backdrop-blur-sm flex items-center justify-center p-4"
    role="dialog"
    aria-modal="true"
    aria-label={locale.t("characters.extract_from_image_title")}
    onclick={(e) => { if (e.target === e.currentTarget) imagePickerOpen = false; }}
  >
    <div
      data-drop-zone="character-image-extract"
      use:tauriDropImagePicker
      class="w-full max-w-sm rounded-xl border bg-neutral-900 shadow-2xl transition-colors {imagePickerDragOver
        ? 'border-indigo-500 bg-indigo-500/10'
        : 'border-neutral-700'}"
      ondragenter={(e) => { e.preventDefault(); e.stopPropagation(); imagePickerDragOver = true; }}
      ondragover={(e) => { e.preventDefault(); e.stopPropagation(); imagePickerDragOver = true; }}
      ondragleave={() => { imagePickerDragOver = false; }}
      ondrop={(e) => {
        e.preventDefault();
        e.stopPropagation();
        imagePickerDragOver = false;
        void handleExtractFileList(e.dataTransfer?.files ?? null);
      }}
    >
      <div class="flex items-center justify-between gap-2 px-4 py-3 border-b border-neutral-800">
        <h3 class="text-sm font-semibold text-neutral-100">
          {locale.t("characters.extract_from_image_title")}
        </h3>
        <button
          type="button"
          class="text-neutral-500 hover:text-neutral-200 text-lg leading-none"
          aria-label={locale.t("common.cancel")}
          onclick={() => { imagePickerOpen = false; }}
        >✕</button>
      </div>
      <div class="p-4 space-y-2">
        {#if hasCurrentPrompt && hasCurrentImage}
          <button
            type="button"
            class="w-full flex items-center gap-3 px-3.5 py-2.5 rounded-lg border border-neutral-700 bg-neutral-800 hover:bg-neutral-700 text-xs font-medium text-neutral-100 transition-colors"
            onclick={() => void extractPromptOnly()}
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 shrink-0 text-amber-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7V4h16v3"/><path d="M9 20h6"/><path d="M12 4v16"/></svg>
            {locale.t("characters.extract_source.prompt_only")}
          </button>
        {/if}
        {#if hasCurrentImage}
          <button
            type="button"
            class="w-full flex items-center gap-3 px-3.5 py-2.5 rounded-lg border border-neutral-700 bg-neutral-800 hover:bg-neutral-700 text-xs font-medium text-neutral-100 transition-colors"
            onclick={() => void extractFromCurrentImage()}
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 shrink-0 text-indigo-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/></svg>
            {locale.t("characters.extract_source.current_only")}
          </button>
        {/if}
        <button
          type="button"
          class="w-full flex items-center gap-3 px-3.5 py-2.5 rounded-lg border border-neutral-700 bg-neutral-800 hover:bg-neutral-700 text-xs font-medium text-neutral-100 transition-colors"
          onclick={() => { imagePickerOpen = false; imageGalleryPickerOpen = true; }}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 shrink-0 text-sky-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/></svg>
          {locale.t("characters.extract_source.gallery")}
        </button>
        <button
          type="button"
          class="w-full flex items-center gap-3 px-3.5 py-2.5 rounded-lg border border-neutral-700 bg-neutral-800 hover:bg-neutral-700 text-xs font-medium text-neutral-100 transition-colors"
          onclick={() => void handleExtractClipboard()}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 shrink-0 text-emerald-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 5H7a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-2"/><rect x="9" y="3" width="6" height="4" rx="1"/></svg>
          {locale.t("generation.interrogate.paste_clipboard")}
        </button>
        <button
          type="button"
          class="w-full flex items-center gap-3 px-3.5 py-2.5 rounded-lg border border-neutral-700 bg-neutral-800 hover:bg-neutral-700 text-xs font-medium text-neutral-100 transition-colors"
          onclick={() => imageFileInput?.click()}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 shrink-0 text-neutral-300" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z"/><polyline points="14 2 14 8 20 8"/></svg>
          {locale.t("generation.interrogate.browse_file")}
        </button>
        <input
          bind:this={imageFileInput}
          type="file"
          accept="image/*"
          class="hidden"
          aria-hidden="true"
          tabindex="-1"
          onchange={(e) => void handleExtractFileList(e.currentTarget.files)}
        />
      </div>
      <p class="px-4 pb-3.5 text-[10px] text-neutral-500 text-center">
        {locale.t("generation.interrogate.drop_hint")}
      </p>
    </div>
  </div>
{/if}
