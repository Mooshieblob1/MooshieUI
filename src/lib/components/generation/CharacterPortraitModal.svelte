<script lang="ts">
  /**
   * Generate a front-on, neck-up portrait of a saved character as its card's
   * picture. The prompt starts from the card, without what a neck-up picture
   * cannot show (listed under it to put back), and can be edited. The
   * expression has its own box: the card's own expression tags, the one used
   * last time, or one the prompt assistant picks from memory or canon. Chunks,
   * Artist Styles and artist tags go on top and are remembered for the next
   * portrait. The UC of picked chunks and styles shows as red pills under the
   * negative prompt, which is where it goes. On NovelAI models with character
   * prompts, the card's look, the expression and the card's UC go in a
   * character prompt and character UC of their own, and the main prompt keeps
   * the count, framing and style, the way Use lays a card out.
   */
  import { untrack } from "svelte";
  import { portal } from "../../utils/portal.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery, isVideoImage } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { novelai } from "../../stores/novelai.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { promptPresets } from "../../stores/promptPresets.svelte.js";
  import { styles } from "../../stores/styles.svelte.js";
  import { savedCharacters, type SavedCharacter } from "../../stores/savedCharacters.svelte.js";
  import {
    currentPortraitSize,
    currentReferenceSize,
    generateCharacterPortrait,
    loadPortraitExtras,
    portraitPrompts,
    portraitStyle,
    portraitUsesCharacterBox,
    referencePrompts,
    savePortraitExtras,
    type CharacterRefFraming,
  } from "../../utils/characterPortrait.js";
  import {
    blobsToNovelAiBase64s,
    characterReferenceBase64,
    outputImagesToBlobs,
    readImageBlobsFromClipboard,
    tauriDropFiles,
    tauriPathsToFiles,
  } from "../../utils/characterReferenceInput.js";
  import { currentImageToCharacterThumbnail } from "../../utils/characterThumbnail.js";
  import { memoryContext, usualExpression } from "../../utils/characterMemory.js";
  import { defaultExpression, negativeForExpression, withExpression } from "../../utils/portraitExpression.js";
  import { mapLlmError } from "../../utils/llmError.js";
  import { estimateNovelAiCost, PRECISE_REFERENCE_COST } from "../../utils/novelaiCost.js";
  import {
    NAI_REF_OUTFIT,
    NOVELAI_MAX_DIRECTOR_REFERENCES,
    NOVELAI_MEDIUM_EFFORT,
    NOVELAI_OUTFIT_REFERENCE_OPTION,
    NOVELAI_REFERENCE_DESCRIPTIONS,
    naiV5Variant,
    novelAiEffortForcedByPreciseReference,
    novelAiReferenceBaseType,
    novelAiReferencePreferSameOutfit,
    novelAiReferenceSupportsSameOutfit,
    novelAiSetPreferSameOutfit,
    novelAiUsesMediumEffort,
  } from "../../utils/novelaiModels.js";
  import { novelAiBase64ToSrc } from "../../utils/novelaiImage.js";
  import { CHECKERBOARD_CLASS } from "../../utils/transparentBackdrop.js";
  import GalleryPickerModal from "../gallery/GalleryPickerModal.svelte";
  import EditableValue from "../ui/EditableValue.svelte";
  import type { NovelAiDirectorReference, OutputImage } from "../../types/index.js";

  interface Props {
    character: SavedCharacter;
    target?: "portrait" | "reference";
    onclose: () => void;
  }

  let { character, target = "portrait", onclose }: Props = $props();

  const isReference = untrack(() => target === "reference");
  // Mounted once per dialog, so the card is read once on purpose.
  const characterBox = untrack(() => portraitUsesCharacterBox());
  const style = untrack(() => portraitStyle(character));
  const remembered = loadPortraitExtras();

  let framing = $state<CharacterRefFraming>(isReference ? "full_body" : "portrait");
  const baseInitial = untrack(() =>
    isReference ? referencePrompts(character, "full_body", characterBox) : portraitPrompts(character, characterBox),
  );
  let activeInitial = $state(baseInitial);

  let positive = $state(baseInitial.positive);
  let negative = $state(baseInitial.negative);
  let characterPositive = $state(baseInitial.character?.positive ?? "");
  let characterNegative = $state(baseInitial.character?.negative ?? "");
  let leftOut = $state<string[]>(baseInitial.dropped);
  // The last portrait's, the card's own expression tags, or the usual one in memory.
  let expression = $state(
    untrack(() => character.expression ?? "") ||
      baseInitial.expression ||
      untrack(() => usualExpression(savedCharacters.memoryOf(character))),
  );
  /** Typed by the user, so a late suggestion must not overwrite it. */
  let expressionTyped = $state(false);
  let suggesting = $state(false);
  let artists = $state(remembered.artists);
  let chunkIds = $state<string[]>(remembered.chunkIds);
  let styleIds = $state<string[]>(remembered.styleIds);
  let saveDescription = $state<string>("character");
  let enablePrOnSave = $state<boolean>(untrack(() => !character.usePreciseReference));
  let guideOpen = $state(false);
  let guideRefs = $state<NovelAiDirectorReference[]>([]);
  let guideBusy = $state(false);
  let guideDragOver = $state(false);
  let guidePickerOpen = $state(false);
  let guideFileInput = $state<HTMLInputElement | null>(null);
  let zoomedSrc = $state<string | null>(null);
  let submitting = $state(false);

  const size = $derived(isReference ? currentReferenceSize(framing) : currentPortraitSize());
  const showNegative = $derived(!generation.disablesNegativePrompt);
  const canGuidePr = $derived(generation.isNovelAi && generation.supportsNovelAiPreciseReference);
  const novelAiModel = $derived(generation.novelAiModel);
  const supportsOutfits = $derived(novelAiModel?.outfitReferences ?? false);
  const strengthMax = $derived(novelAiModel?.preciseReferenceStrengthMax ?? 1);
  const guideFull = $derived(guideRefs.length >= NOVELAI_MAX_DIRECTOR_REFERENCES || guideBusy);
  const canGenerate = $derived(
    !submitting && (positive.trim().length > 0 || characterPositive.trim().length > 0) && generation.mode !== "video" && (!suggesting || expressionTyped),
  );

  const FRAMING_OPTIONS: ReadonlyArray<{ value: CharacterRefFraming; labelKey: string }> = [
    { value: "full_body", labelKey: "characters.reference_gen.framing_full_body" },
    { value: "upper_body", labelKey: "characters.reference_gen.framing_upper_body" },
    { value: "portrait", labelKey: "characters.reference_gen.framing_portrait" },
    { value: "sheet", labelKey: "characters.reference_gen.framing_sheet" },
  ];

  /** Deduplicated images already saved on the card for 1-click guide reuse. */
  const cardSavedImages = $derived.by(() => {
    const out: Array<{ key: string; thumbnail: string; reference: string }> = [];
    const seen = new Set<string>();
    for (const ref of character.preciseReferences ?? []) {
      const source = ref.reference || ref.thumbnail;
      if (!source || seen.has(source)) continue;
      seen.add(source);
      out.push({ key: ref.id, thumbnail: ref.thumbnail || ref.reference, reference: source });
    }
    const thumb = character.thumbnail;
    const refImg = character.reference ?? character.thumbnail;
    if (refImg && !seen.has(refImg)) {
      out.push({ key: "__card__", thumbnail: thumb ?? refImg, reference: refImg });
    }
    return out;
  });

  function selectFraming(next: CharacterRefFraming) {
    if (framing === next) return;
    framing = next;
    const prompts = referencePrompts(character, next, characterBox);
    activeInitial = prompts;
    positive = prompts.positive;
    negative = prompts.negative;
    characterPositive = prompts.character?.positive ?? "";
    characterNegative = prompts.character?.negative ?? "";
    leftOut = prompts.dropped;
  }

  /**
   * Ask the prompt assistant for an expression that fits the character. On
   * opening, a failure quietly falls back to a smile; asked for, it says why.
   */
  async function suggestExpression(asked: boolean) {
    if (suggesting) return;
    expressionTyped = false;
    suggesting = true;
    try {
      const notes = memoryContext(savedCharacters.memoryOf(character));
      const face = await promptAssistant.portraitExpression(
        { name: character.name, prompt: character.prompt },
        notes,
        style,
      );
      if (!expressionTyped) {
        expression = face;
        // Kept straight away, so opening the dialog again doesn't ask again.
        savedCharacters.setExpression(character.id, face);
      }
    } catch (e) {
      console.warn("Portrait expression failed:", e);
      if (asked) gallery.showToast(mapLlmError(String(e)), "error");
      if (!expressionTyped && !expression.trim()) expression = defaultExpression(style);
    } finally {
      suggesting = false;
    }
  }

  // No expression from the card or the last portrait: pick one now.
  untrack(() => {
    if (expression.trim()) return;
    if (promptAssistant.canUseMemory && !promptAssistant.isGenerating) void suggestExpression(false);
    else expression = defaultExpression(style);
  });

  /** Put a left-out part of the card's prompt back at the end of the prompt it came from. */
  function addBack(index: number) {
    const item = leftOut[index];
    if (item === undefined) return;
    const put = (text: string) =>
      style === "tags"
        ? [text.trim().replace(/[,\s]+$/, ""), item].filter(Boolean).join(", ")
        : `${text.trim().replace(/[.\s]*$/, "")}, ${item}.`;
    if (characterBox) characterPositive = put(characterPositive);
    else positive = put(positive);
    leftOut = leftOut.filter((_, i) => i !== index);
  }

  const activeGuideRefs = $derived(
    canGuidePr
      ? guideRefs.filter((r) => r.enabled !== false && r.image.length > 0)
      : [],
  );
  const forcedHigh = $derived(
    novelAiEffortForcedByPreciseReference(novelAiModel, activeGuideRefs, false),
  );

  /** Anlas for the generation on NovelAI, or null elsewhere. */
  const anlas = $derived.by(() => {
    if (!generation.isNovelAi) return null;
    const mediumEffort = novelAiUsesMediumEffort(
      novelAiModel,
      generation.novelaiSettings.effort,
      forcedHigh,
    );
    return estimateNovelAiCost({
      width: size.width,
      height: size.height,
      steps: mediumEffort ? NOVELAI_MEDIUM_EFFORT.steps : generation.steps,
      mediumEffort,
      nSamples: 1,
      strength: 1,
      isOpus: novelai.isOpus,
      opusExhausted: naiV5Variant(generation.checkpoint) !== null && novelai.opusAllowanceEmpty,
      vibeEncodes: 0,
      preciseReferences: activeGuideRefs.length,
    });
  });

  function pushGuideImages(base64s: readonly string[]): { added: number; dupes: number; hitCap: boolean } {
    let added = 0;
    let dupes = 0;
    let hitCap = false;
    const next = [...guideRefs];
    for (const img of base64s) {
      if (next.length >= NOVELAI_MAX_DIRECTOR_REFERENCES) {
        hitCap = true;
        break;
      }
      if (next.some((r) => r.image === img)) {
        dupes += 1;
        continue;
      }
      next.push({
        image: img,
        description: "character",
        strength: 1.0,
        fidelity: 1.0,
        enabled: true,
      });
      added += 1;
    }
    if (added > 0) {
      guideRefs = next;
      guideOpen = true;
    }
    return { added, dupes, hitCap };
  }

  async function addGuideBlobs(blobs: readonly Blob[]) {
    if (blobs.length === 0 || guideBusy) return;
    guideBusy = true;
    try {
      const { images, failed } = await blobsToNovelAiBase64s(blobs);
      const res = pushGuideImages(images);
      if (res.hitCap) {
        gallery.showToast(
          locale.t("generation.novelai.reference.toast_full", {
            max: String(NOVELAI_MAX_DIRECTOR_REFERENCES),
          }),
          "info",
        );
      } else if (res.dupes > 0 && res.added === 0) {
        gallery.showToast(locale.t("generation.novelai.reference.toast_duplicate"), "info");
      } else if (failed > 0 && res.added === 0 && res.dupes === 0) {
        gallery.showToast(locale.t("generation.novelai.reference.toast_failed"), "error");
      }
    } finally {
      guideBusy = false;
    }
  }

  async function addGuideFromCardImage(thumb: string, reference: string) {
    if (guideBusy) return;
    guideBusy = true;
    try {
      const base64 = await characterReferenceBase64({ thumbnail: thumb, reference });
      if (!base64) {
        gallery.showToast(locale.t("generation.novelai.reference.toast_failed"), "error");
        return;
      }
      const res = pushGuideImages([base64]);
      if (res.hitCap) {
        gallery.showToast(
          locale.t("generation.novelai.reference.toast_full", { max: String(NOVELAI_MAX_DIRECTOR_REFERENCES) }),
          "info",
        );
      } else if (res.dupes > 0 && res.added === 0) {
        gallery.showToast(locale.t("generation.novelai.reference.toast_duplicate"), "info");
      }
    } finally {
      guideBusy = false;
    }
  }

  async function addGuideFromCurrent() {
    const latest = gallery.sessionImages.find((image) => !isVideoImage(image)) ?? null;
    if (!progress.lastOutputImage && !latest) {
      gallery.showToast(locale.t("characters.thumbnail.no_current"), "info");
      return;
    }
    guideBusy = true;
    try {
      const imgs = await currentImageToCharacterThumbnail(progress.lastOutputImage, latest);
      if (!imgs) {
        gallery.showToast(locale.t("generation.novelai.reference.toast_failed"), "error");
        return;
      }
      const base64 = await characterReferenceBase64(imgs);
      if (!base64) {
        gallery.showToast(locale.t("generation.novelai.reference.toast_failed"), "error");
        return;
      }
      const res = pushGuideImages([base64]);
      if (res.hitCap) {
        gallery.showToast(
          locale.t("generation.novelai.reference.toast_full", { max: String(NOVELAI_MAX_DIRECTOR_REFERENCES) }),
          "info",
        );
      } else if (res.dupes > 0 && res.added === 0) {
        gallery.showToast(locale.t("generation.novelai.reference.toast_duplicate"), "info");
      }
    } finally {
      guideBusy = false;
    }
  }

  async function addGuideFromClipboard(clipboardItems?: DataTransferItemList) {
    const blobs = await readImageBlobsFromClipboard(clipboardItems);
    if (blobs.length === 0) {
      gallery.showToast(locale.t("common.no_clipboard_image"), "info");
      return;
    }
    await addGuideBlobs(blobs);
  }

  async function addGuideFromGallery(images: OutputImage[]) {
    guidePickerOpen = false;
    if (images.length === 0 || guideBusy) return;
    guideBusy = true;
    try {
      const blobs = await outputImagesToBlobs(images);
      guideBusy = false;
      if (blobs.length > 0) await addGuideBlobs(blobs);
      else gallery.showToast(locale.t("generation.novelai.reference.toast_failed"), "error");
    } finally {
      guideBusy = false;
    }
  }

  async function addGuideFromTauriPaths(paths: string[]) {
    try {
      const files = await tauriPathsToFiles(paths);
      await addGuideBlobs(files);
    } catch (err) {
      console.error("Character reference guide drop failed:", err);
      gallery.showToast(locale.t("generation.novelai.reference.toast_failed"), "error");
    }
  }

  $effect(() => {
    const handler = (event: ClipboardEvent) => {
      if (!guideOpen || guidePickerOpen || zoomedSrc || guideFull) return;
      const el = event.target as HTMLElement | null;
      if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable)) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      void addGuideFromClipboard(event.clipboardData?.items);
    };
    window.addEventListener("paste", handler, { capture: true });
    return () => window.removeEventListener("paste", handler, { capture: true });
  });

  /** UC the picked chunks and styles add to the negative prompt, by name. */
  const pickedUc = $derived([
    ...promptPresets.presets
      .filter((p) => chunkIds.includes(p.id) && p.negative.trim())
      .map((p) => ({ id: p.id, name: p.name, tags: p.negative.trim() })),
    ...styles.styles
      .filter((s) => styleIds.includes(s.id) && s.negative.trim())
      .map((s) => ({ id: s.id, name: s.name, tags: s.negative.trim() })),
  ]);

  function toggle(list: string[], id: string): string[] {
    return list.includes(id) ? list.filter((x) => x !== id) : [...list, id];
  }

  const edited = $derived(
    positive !== activeInitial.positive ||
      negative !== activeInitial.negative ||
      characterPositive !== (activeInitial.character?.positive ?? "") ||
      characterNegative !== (activeInitial.character?.negative ?? ""),
  );

  function resetPrompts() {
    positive = activeInitial.positive;
    negative = activeInitial.negative;
    characterPositive = activeInitial.character?.positive ?? "";
    characterNegative = activeInitial.character?.negative ?? "";
    leftOut = activeInitial.dropped;
  }

  async function generate() {
    if (!canGenerate) return;
    submitting = true;
    const extras = { artists: artists.trim(), chunkIds, styleIds };
    try {
      const mainNegative = showNegative ? negativeForExpression(negative, expression, style) : "";
      await generateCharacterPortrait(
        character,
        characterBox
          ? {
              // The expression is the character's, so it goes in its box.
              positive,
              negative: mainNegative,
              character: {
                positive: withExpression(characterPositive, expression, style),
                negative: showNegative ? characterNegative : "",
              },
            }
          : { positive: withExpression(positive, expression, style), negative: mainNegative },
        extras,
        {
          target,
          framing,
          directorReferences: activeGuideRefs,
          ...(isReference
            ? {
                referenceDescription: saveDescription,
                enablePr: !character.usePreciseReference && enablePrOnSave,
              }
            : {}),
        },
      );
      savePortraitExtras(extras);
      savedCharacters.setExpression(character.id, expression);
      if (promptAssistant.canUseMemory && expression.trim()) savedCharacters.rememberExpression(character, expression);
      gallery.showToast(
        locale.t(isReference ? "characters.reference_gen.queued" : "characters.portrait.queued", {
          name: character.name,
        }),
        "info",
      );
      onclose();
    } catch (e) {
      console.error("Character image generation failed:", e);
      gallery.showToast(e instanceof Error ? e.message : String(e), "error");
    } finally {
      submitting = false;
    }
  }

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (zoomedSrc) {
        e.preventDefault();
        e.stopImmediatePropagation();
        zoomedSrc = null;
        return;
      }
      if (guidePickerOpen) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      onclose();
    };
    document.addEventListener("keydown", onKey, { capture: true });
    return () => document.removeEventListener("keydown", onKey, { capture: true });
  });

  const chipBase = "inline-flex items-center gap-1 px-2 py-0.5 text-[11px] rounded-full border transition-colors max-w-[12rem]";
  const chipOn = "border-neutral-400 bg-neutral-700 text-neutral-50";
  const chipOff = "border-neutral-700 text-neutral-400 hover:border-neutral-500 hover:text-neutral-200";
</script>

<div
  use:portal
  class="fixed inset-0 z-220 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4"
  role="dialog"
  aria-modal="true"
  aria-label={locale.t(isReference ? "characters.reference_gen.title" : "characters.portrait.title", { name: character.name })}
>
  {#snippet leftOutChips()}
    {#if leftOut.length > 0}
      <div class="flex flex-wrap items-center gap-1" title={locale.t("characters.portrait.left_out_tip")}>
        <span class="text-[10px] text-neutral-500">{locale.t("characters.portrait.left_out")}</span>
        {#each leftOut as item, i (`${i}:${item}`)}
          <button
            type="button"
            class="max-w-[14rem] truncate rounded-full border border-dashed border-neutral-700 px-2 py-px text-[10px] text-neutral-500 line-through hover:border-neutral-500 hover:text-neutral-200 hover:no-underline"
            title={locale.t("characters.portrait.add_back", { tag: item })}
            aria-label={locale.t("characters.portrait.add_back", { tag: item })}
            onclick={() => addBack(i)}
          >{item}</button>
        {/each}
      </div>
    {/if}
  {/snippet}
  <button
    type="button"
    class="absolute inset-0 h-full w-full cursor-default"
    aria-label={locale.t("common.cancel")}
    onclick={onclose}
  ></button>
  <div class="relative z-10 flex max-h-[90vh] w-full max-w-xl flex-col gap-3 rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl">
    <div class="flex items-center gap-3">
      {#if character.thumbnail}
        <img src={character.thumbnail} alt={character.name} class="w-10 h-10 rounded-md object-cover border border-neutral-700" />
      {/if}
      <h3 class="flex-1 min-w-0 truncate text-sm font-semibold text-neutral-100">
        {locale.t(isReference ? "characters.reference_gen.title" : "characters.portrait.title", { name: character.name })}
      </h3>
      <button
        type="button"
        class="text-neutral-500 hover:text-neutral-200 text-lg leading-none"
        onclick={onclose}
        aria-label={locale.t("common.cancel")}
      >✕</button>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto flex flex-col gap-3">
      {#if isReference}
        <div class="flex flex-wrap items-center gap-2 rounded-lg border border-neutral-800 bg-neutral-950/60 p-2">
          <div class="flex flex-wrap items-center gap-1 flex-1 min-w-0">
            <span class="text-[11px] font-medium text-neutral-400 mr-1">
              {locale.t("characters.reference_gen.framing")}
            </span>
            {#each FRAMING_OPTIONS as opt (opt.value)}
              <button
                type="button"
                class="px-2 py-0.5 text-[11px] rounded border transition-colors {framing === opt.value
                  ? 'border-indigo-500 bg-indigo-500/20 text-indigo-200'
                  : 'border-neutral-700 text-neutral-400 hover:border-neutral-500 hover:text-neutral-200'}"
                aria-pressed={framing === opt.value}
                onclick={() => selectFraming(opt.value)}
              >{locale.t(opt.labelKey)}</button>
            {/each}
          </div>
          <label class="flex items-center gap-1.5 text-[11px] text-neutral-300">
            <span class="text-neutral-500">{locale.t("characters.reference_gen.save_as")}</span>
            <select
              class="px-1.5 py-0.5 text-[11px] rounded bg-neutral-900 border border-neutral-700 text-neutral-200 focus:outline-none focus:border-indigo-500"
              value={novelAiReferenceBaseType(saveDescription)}
              onchange={(e) => {
                saveDescription = e.currentTarget.value;
              }}
            >
              {#each NOVELAI_REFERENCE_DESCRIPTIONS as option (option.value)}
                <option value={option.value}>{locale.t(option.labelKey)}</option>
              {/each}
              {#if supportsOutfits}
                <option value={NOVELAI_OUTFIT_REFERENCE_OPTION.value}>
                  {locale.t(NOVELAI_OUTFIT_REFERENCE_OPTION.labelKey)}
                </option>
              {/if}
            </select>
          </label>
          {#if supportsOutfits && novelAiReferenceSupportsSameOutfit(saveDescription)}
            <label class="flex items-center gap-1 text-[11px] text-neutral-300">
              <input
                type="checkbox"
                class="accent-indigo-500"
                checked={novelAiReferencePreferSameOutfit(saveDescription)}
                onchange={(e) => {
                  saveDescription = novelAiSetPreferSameOutfit(saveDescription, e.currentTarget.checked);
                }}
              />
              {locale.t("generation.novelai.reference.prefer_same_outfit")}
            </label>
          {/if}
          {#if !character.usePreciseReference}
            <label class="flex items-center gap-1.5 text-[11px] text-neutral-300 w-full">
              <input type="checkbox" class="accent-indigo-500" bind:checked={enablePrOnSave} />
              {locale.t("characters.reference_gen.enable_pr")}
            </label>
          {/if}
        </div>
      {/if}
      <div class="flex flex-col gap-1">
        <div class="flex items-center justify-between gap-2">
          <label
            for="character-portrait-prompt"
            class="text-xs font-medium text-neutral-300"
            title={characterBox ? locale.t("characters.portrait.main_prompt_tip") : undefined}
          >
            {locale.t(characterBox ? "characters.portrait.main_prompt" : "characters.portrait.prompt")}
          </label>
          <button
            type="button"
            class="w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800 disabled:opacity-40"
            title={locale.t("characters.portrait.reset")}
            aria-label={locale.t("characters.portrait.reset")}
            disabled={!edited}
            onclick={resetPrompts}
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="1 4 1 10 7 10"/><path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10"/></svg>
          </button>
        </div>
        <textarea
          id="character-portrait-prompt"
          bind:value={positive}
          rows="4"
          spellcheck="false"
          class="w-full resize-y rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-[11px] leading-relaxed text-neutral-100 focus:outline-none focus:border-indigo-500"
        ></textarea>
        {#if !characterBox}
          {@render leftOutChips()}
        {/if}
      </div>

      {#if characterBox}
        <div class="flex flex-col gap-1">
          <label
            for="character-portrait-character"
            class="text-xs font-medium text-neutral-300"
            title={locale.t("characters.portrait.character_prompt_tip")}
          >
            {locale.t("characters.portrait.character_prompt")}
          </label>
          <textarea
            id="character-portrait-character"
            bind:value={characterPositive}
            rows="3"
            spellcheck="false"
            placeholder={locale.t("generation.novelai.characters.prompt_placeholder")}
            class="w-full resize-y rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-[11px] leading-relaxed text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500"
          ></textarea>
          {@render leftOutChips()}
        </div>
      {/if}

      <div class="flex flex-col gap-1">
        <div class="flex items-center justify-between gap-2">
          <label for="character-portrait-expression" class="text-xs font-medium text-neutral-300">
            {locale.t("characters.portrait.expression")}
          </label>
          {#if promptAssistant.canUseMemory}
            <button
              type="button"
              class="w-5 h-5 flex items-center justify-center rounded text-[11px] hover:bg-neutral-800 disabled:opacity-40"
              title={locale.t("characters.portrait.expression_suggest", { name: character.name })}
              aria-label={locale.t("characters.portrait.expression_suggest", { name: character.name })}
              disabled={suggesting || promptAssistant.isGenerating}
              onclick={() => suggestExpression(true)}
            >
              {#if suggesting}
                <span class="inline-block animate-spin text-neutral-400">⟳</span>
              {:else}
                ✨
              {/if}
            </button>
          {/if}
        </div>
        <input
          id="character-portrait-expression"
          type="text"
          bind:value={expression}
          oninput={() => { expressionTyped = true; }}
          spellcheck="false"
          placeholder={suggesting
            ? locale.t("characters.portrait.expression_picking", { name: character.name })
            : locale.t("characters.portrait.expression_placeholder")}
          class="w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500"
        />
      </div>

      {#if showNegative}
        <div class="flex flex-col gap-1">
          <label for="character-portrait-negative" class="text-xs font-medium text-neutral-300">
            {locale.t(characterBox ? "characters.portrait.main_negative" : "characters.portrait.negative")}
          </label>
          <textarea
            id="character-portrait-negative"
            bind:value={negative}
            rows="2"
            spellcheck="false"
            class="w-full resize-y rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-[11px] leading-relaxed text-neutral-100 focus:outline-none focus:border-red-500/70"
          ></textarea>
          {#if pickedUc.length > 0}
            <div class="flex flex-wrap items-center gap-1">
              {#each pickedUc as uc (uc.id)}
                <span
                  class="max-w-[16rem] truncate rounded-full border border-red-500/50 bg-red-500/10 px-2 py-px text-[10px] text-red-300"
                  title={locale.t("prompt_uc.from_title", { name: uc.name, tags: uc.tags })}
                >{uc.tags.replace(/\s*\n\s*/g, ", ")}</span>
              {/each}
            </div>
          {/if}
        </div>
        {#if characterBox}
          <div class="flex flex-col gap-1">
            <label for="character-portrait-character-negative" class="text-xs font-medium text-neutral-300">
              {locale.t("characters.portrait.character_negative")}
            </label>
            <textarea
              id="character-portrait-character-negative"
              bind:value={characterNegative}
              rows="2"
              spellcheck="false"
              placeholder={locale.t("generation.novelai.characters.negative_placeholder")}
              class="w-full resize-y rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-[11px] leading-relaxed text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-red-500/70"
            ></textarea>
          </div>
        {/if}
      {/if}

      <div class="flex flex-col gap-1">
        <label
          for="character-portrait-artists"
          class="text-xs font-medium text-neutral-300"
          title={locale.t("characters.portrait.artists_tip")}
        >
          {locale.t("characters.portrait.artists")}
        </label>
        <input
          id="character-portrait-artists"
          type="text"
          bind:value={artists}
          spellcheck="false"
          title={locale.t("characters.portrait.artists_tip")}
          class="w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-100 focus:outline-none focus:border-indigo-500"
        />
      </div>

      {#if promptPresets.presets.length > 0}
        <div class="flex flex-col gap-1">
          <span class="text-xs font-medium text-neutral-300">{locale.t("characters.portrait.chunks")}</span>
          <div class="flex flex-wrap gap-1">
            {#each promptPresets.presets as preset (preset.id)}
              <button
                type="button"
                class="{chipBase} {chunkIds.includes(preset.id) ? chipOn : chipOff}"
                aria-pressed={chunkIds.includes(preset.id)}
                title={preset.negative.trim()
                  ? `${preset.content}\n${locale.t("prompt_uc.pill_title", { tags: preset.negative.trim() })}`.trim()
                  : preset.content}
                onclick={() => { chunkIds = toggle(chunkIds, preset.id); }}
              >
                <span class="truncate">{preset.name}</span>
                {#if preset.negative.trim()}
                  <span class="shrink-0 rounded-full border border-red-500/50 bg-red-500/10 px-1.5 py-px text-[9px] leading-none text-red-300">{locale.t("prompt_uc.label")}</span>
                {/if}
              </button>
            {/each}
          </div>
        </div>
      {/if}

      {#if styles.styles.length > 0}
        <div class="flex flex-col gap-1">
          <span class="text-xs font-medium text-neutral-300">{locale.t("characters.portrait.styles")}</span>
          <div class="flex flex-wrap gap-1">
            {#each styles.styles as style (style.id)}
              <button
                type="button"
                class="{chipBase} {styleIds.includes(style.id) ? chipOn : chipOff}"
                aria-pressed={styleIds.includes(style.id)}
                title={style.negative.trim()
                  ? `${style.artists.map((a) => a.tag).join(", ")}\n${locale.t("prompt_uc.pill_title", { tags: style.negative.trim() })}`.trim()
                  : style.artists.map((a) => a.tag).join(", ")}
                onclick={() => { styleIds = toggle(styleIds, style.id); }}
              >
                <span class="truncate">{style.name}</span>
                {#if style.negative.trim()}
                  <span class="shrink-0 rounded-full border border-red-500/50 bg-red-500/10 px-1.5 py-px text-[9px] leading-none text-red-300">{locale.t("prompt_uc.label")}</span>
                {/if}
              </button>
            {/each}
          </div>
        </div>
      {/if}

      {#if canGuidePr}
        <div class="rounded-lg border border-neutral-800 bg-neutral-950/50 p-2.5 flex flex-col gap-2">
          <div class="flex items-center justify-between gap-2">
            <button
              type="button"
              class="flex items-center gap-1.5 text-xs font-medium text-neutral-300 hover:text-white"
              aria-expanded={guideOpen}
              onclick={() => { guideOpen = !guideOpen; }}
            >
              <span class="text-[10px] text-neutral-500">{guideOpen ? "▾" : "▸"}</span>
              {locale.t("characters.reference_gen.via_pr")}
              {#if guideRefs.length > 0}
                <span class="rounded bg-indigo-500/20 border border-indigo-500/40 px-1.5 py-px text-[10px] tabular-nums text-indigo-200">
                  {activeGuideRefs.length}/{guideRefs.length} · {locale.t("characters.references.anlas_badge", { anlas: String(activeGuideRefs.length * PRECISE_REFERENCE_COST) })}
                </span>
              {/if}
            </button>
            {#if guideRefs.length > 0}
              <button
                type="button"
                class="text-[11px] text-neutral-500 hover:text-neutral-200"
                onclick={() => { guideRefs = []; }}
              >
                {locale.t("generation.novelai.reference.clear_all")}
              </button>
            {/if}
          </div>

          {#if guideOpen}
            {#if cardSavedImages.length > 0 && !guideFull}
              <div class="flex flex-wrap items-center gap-1.5">
                <span class="text-[10px] text-neutral-500">{locale.t("characters.reference_gen.use_saved")}</span>
                {#each cardSavedImages as img (img.key)}
                  <button
                    type="button"
                    class="w-9 h-9 rounded overflow-hidden border border-neutral-700 hover:border-indigo-500 transition-colors {CHECKERBOARD_CLASS}"
                    title={locale.t("characters.reference_gen.use_saved_tip")}
                    onclick={() => void addGuideFromCardImage(img.thumbnail, img.reference)}
                  >
                    <img src={img.thumbnail} alt="" class="w-full h-full object-cover" />
                  </button>
                {/each}
              </div>
            {/if}

            {#if guideRefs.length < NOVELAI_MAX_DIRECTOR_REFERENCES}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                use:tauriDropFiles={(paths) => void addGuideFromTauriPaths(paths)}
                class="border border-dashed rounded-lg px-2.5 py-2 text-center transition-colors {guideDragOver
                  ? 'border-indigo-500 bg-indigo-500/10'
                  : 'border-neutral-700 hover:border-neutral-600 bg-neutral-900/50'} {guideBusy
                  ? 'opacity-40 pointer-events-none'
                  : ''}"
                ondragenter={(e) => { e.preventDefault(); e.stopPropagation(); guideDragOver = true; }}
                ondragover={(e) => { e.preventDefault(); e.stopPropagation(); guideDragOver = true; }}
                ondragleave={() => { guideDragOver = false; }}
                ondrop={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  guideDragOver = false;
                  const files = e.dataTransfer?.files;
                  if (files && files.length > 0) void addGuideBlobs(Array.from(files));
                }}
              >
                <div class="flex items-center justify-center gap-2 flex-wrap text-[11px]">
                  <span class="text-[10px] text-neutral-500">{locale.t("generation.image.drag_drop_or")}</span>
                  <button
                    type="button"
                    class="text-emerald-400/80 hover:text-emerald-300"
                    onclick={() => void addGuideFromClipboard()}
                  >{locale.t("generation.image.ctrl_v_paste")}</button>
                  <span class="text-neutral-700">|</span>
                  <button
                    type="button"
                    class="text-neutral-300 hover:text-white"
                    onclick={() => guideFileInput?.click()}
                  >{locale.t("generation.image.select_image")}</button>
                  <span class="text-neutral-700">|</span>
                  <button
                    type="button"
                    class="text-indigo-400 hover:text-indigo-300"
                    onclick={() => { guidePickerOpen = true; }}
                  >{locale.t("generation.novelai.reference.from_gallery")}</button>
                  <span class="text-neutral-700">|</span>
                  <button
                    type="button"
                    class="text-sky-400 hover:text-sky-300"
                    onclick={() => void addGuideFromCurrent()}
                  >{locale.t("characters.thumbnail.current")}</button>
                </div>
              </div>
              <input
                bind:this={guideFileInput}
                type="file"
                accept="image/*"
                multiple
                class="hidden"
                onchange={(e) => {
                  const files = e.currentTarget.files;
                  if (files && files.length > 0) void addGuideBlobs(Array.from(files));
                  e.currentTarget.value = "";
                }}
              />
            {/if}

            {#if forcedHigh}
              <p class="text-[10px] text-amber-300/90">
                {locale.t("generation.novelai.reference.forced_high_note")}
              </p>
            {/if}

            {#each guideRefs as reference, index (index)}
              {@const refEnabled = reference.enabled !== false}
              {@const baseDesc = novelAiReferenceBaseType(reference.description)}
              <div class="rounded-lg border border-neutral-800 bg-neutral-900/70 p-2 flex gap-2.5 {!refEnabled ? 'opacity-50' : ''}">
                <button
                  type="button"
                  class="shrink-0 w-14 h-14 rounded overflow-hidden border border-neutral-800 hover:border-indigo-500/70 cursor-zoom-in {CHECKERBOARD_CLASS}"
                  title={locale.t("generation.novelai.reference.zoom")}
                  onclick={() => { zoomedSrc = novelAiBase64ToSrc(reference.image); }}
                >
                  <img src={novelAiBase64ToSrc(reference.image)} alt="" class="w-full h-full object-cover" />
                </button>
                <div class="flex-1 min-w-0 space-y-1">
                  <div class="flex items-center gap-1.5">
                    <input
                      type="checkbox"
                      class="accent-indigo-500 shrink-0"
                      checked={refEnabled}
                      aria-label={locale.t("generation.novelai.reference.enable")}
                      onchange={(e) => {
                        const checked = e.currentTarget.checked;
                        guideRefs = guideRefs.map((r, i) => (i === index ? { ...r, enabled: checked } : r));
                      }}
                    />
                    <select
                      class="flex-1 min-w-0 px-1.5 py-0.5 text-[11px] rounded bg-neutral-950 border border-neutral-800 text-neutral-200 focus:outline-none focus:border-indigo-600"
                      value={baseDesc}
                      onchange={(e) => {
                        const next = e.currentTarget.value;
                        guideRefs = guideRefs.map((r, i) => (i === index ? { ...r, description: next } : r));
                      }}
                    >
                      {#each NOVELAI_REFERENCE_DESCRIPTIONS as option (option.value)}
                        <option value={option.value}>{locale.t(option.labelKey)}</option>
                      {/each}
                      {#if supportsOutfits || reference.description === NAI_REF_OUTFIT}
                        <option value={NOVELAI_OUTFIT_REFERENCE_OPTION.value}>
                          {locale.t(NOVELAI_OUTFIT_REFERENCE_OPTION.labelKey)}
                        </option>
                      {/if}
                    </select>
                    <button
                      type="button"
                      class="px-1.5 py-0.5 text-[11px] rounded text-neutral-400 hover:text-red-400 hover:bg-neutral-800"
                      onclick={() => { guideRefs = guideRefs.filter((_, i) => i !== index); }}
                    >{locale.t("generation.novelai.reference.remove")}</button>
                  </div>
                  {#if supportsOutfits && novelAiReferenceSupportsSameOutfit(reference.description)}
                    <label class="flex items-center gap-1.5 text-[10px] text-neutral-300">
                      <input
                        type="checkbox"
                        class="accent-indigo-500"
                        checked={novelAiReferencePreferSameOutfit(reference.description)}
                        onchange={(e) => {
                          const checked = e.currentTarget.checked;
                          guideRefs = guideRefs.map((r, i) =>
                            i === index ? { ...r, description: novelAiSetPreferSameOutfit(r.description, checked) } : r,
                          );
                        }}
                      />
                      {locale.t("generation.novelai.reference.prefer_same_outfit")}
                    </label>
                  {/if}
                  <div class="flex items-center gap-2 text-[10px] text-neutral-400">
                    <span class="shrink-0">{locale.t("generation.novelai.reference.strength")}</span>
                    <input
                      type="range"
                      min="0"
                      max={strengthMax}
                      step="0.05"
                      class="flex-1 accent-indigo-500"
                      value={Math.min(strengthMax, Math.max(0, reference.strength))}
                      oninput={(e) => {
                        const v = Number(e.currentTarget.value);
                        guideRefs = guideRefs.map((r, i) => (i === index ? { ...r, strength: v } : r));
                      }}
                    />
                    <EditableValue
                      value={reference.strength}
                      min={0}
                      max={10}
                      step={0.05}
                      decimals={2}
                      onchange={(v) => {
                        guideRefs = guideRefs.map((r, i) => (i === index ? { ...r, strength: v } : r));
                      }}
                    />
                  </div>
                </div>
              </div>
            {/each}
          {/if}
        </div>
      {/if}
    </div>

    <div class="flex flex-wrap items-center justify-end gap-2">
      <span
        class="mr-auto text-[11px] text-neutral-500"
        title={isReference ? undefined : locale.t("characters.portrait.size_tip")}
      >
        {size.width} × {size.height}
      </span>
      <button
        type="button"
        class="px-3 py-1.5 text-xs rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800"
        onclick={onclose}
      >{locale.t("common.cancel")}</button>
      <button
        type="button"
        class="px-3 py-1.5 text-xs font-medium rounded bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-40 disabled:cursor-not-allowed flex items-center gap-1.5"
        disabled={!canGenerate}
        title={generation.mode === "video" ? locale.t("characters.portrait.no_video") : undefined}
        onclick={generate}
      >
        {#if submitting}
          <svg class="w-3 h-3 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 12a9 9 0 1 1-6.22-8.56" stroke-linecap="round"/></svg>
        {/if}
        {locale.t("characters.portrait.generate")}
        {#if anlas !== null}
          <span class="rounded bg-black/25 px-1.5 py-px text-[10px] font-normal">
            {anlas === 0 ? locale.t("novelai.enhance.free") : locale.t("novelai.enhance.cost", { anlas: String(anlas) })}
          </span>
        {/if}
      </button>
    </div>
  </div>
</div>

{#if guidePickerOpen}
  <GalleryPickerModal
    open={true}
    multiple
    max={Math.max(1, NOVELAI_MAX_DIRECTOR_REFERENCES - guideRefs.length)}
    title={locale.t("generation.novelai.reference.gallery_pick_title")}
    onselect={(images) => void addGuideFromGallery(images)}
    onclose={() => { guidePickerOpen = false; }}
  />
{/if}

{#if zoomedSrc}
  <div
    use:portal
    class="fixed inset-0 z-[10000] bg-black/80 backdrop-blur-sm flex items-center justify-center p-4"
    role="dialog"
    aria-modal="true"
    aria-label={locale.t("generation.novelai.reference.zoom")}
    tabindex="-1"
  >
    <button
      type="button"
      class="absolute inset-0 h-full w-full cursor-default"
      aria-label={locale.t("common.close")}
      onclick={() => { zoomedSrc = null; }}
    ></button>
    <div class="relative z-10 max-w-[90vw] max-h-[90vh]">
      <img
        src={zoomedSrc}
        alt=""
        class="max-w-[90vw] max-h-[85vh] rounded-lg border border-neutral-700 object-contain {CHECKERBOARD_CLASS}"
      />
      <button
        type="button"
        class="absolute top-2 right-2 w-7 h-7 flex items-center justify-center rounded-full bg-neutral-900/90 border border-neutral-700 text-neutral-300 hover:text-white"
        aria-label={locale.t("common.close")}
        onclick={() => { zoomedSrc = null; }}
      >✕</button>
    </div>
  </div>
{/if}
