<script lang="ts">
  /**
   * Generate a front-on, neck-up portrait of a saved character as its card's
   * picture. The prompt starts from the card and can be edited; chunks, Artist
   * Styles and artist tags go on top and are remembered for the next portrait.
   */
  import { untrack } from "svelte";
  import { portal } from "../../utils/portal.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { novelai } from "../../stores/novelai.svelte.js";
  import { promptPresets } from "../../stores/promptPresets.svelte.js";
  import { styles } from "../../stores/styles.svelte.js";
  import type { SavedCharacter } from "../../stores/savedCharacters.svelte.js";
  import {
    currentPortraitSize,
    generateCharacterPortrait,
    loadPortraitExtras,
    portraitPrompts,
    savePortraitExtras,
  } from "../../utils/characterPortrait.js";
  import { estimateNovelAiCost } from "../../utils/novelaiCost.js";
  import { naiV5Variant } from "../../utils/novelaiModels.js";

  interface Props {
    character: SavedCharacter;
    onclose: () => void;
  }

  let { character, onclose }: Props = $props();

  // Mounted once per portrait, so the card is read once on purpose.
  const initial = untrack(() => portraitPrompts(character));
  const remembered = loadPortraitExtras();

  let positive = $state(initial.positive);
  let negative = $state(initial.negative);
  let artists = $state(remembered.artists);
  let chunkIds = $state<string[]>(remembered.chunkIds);
  let styleIds = $state<string[]>(remembered.styleIds);
  let submitting = $state(false);

  const size = $derived(currentPortraitSize());
  const showNegative = $derived(!generation.disablesNegativePrompt);
  const canGenerate = $derived(!submitting && positive.trim().length > 0 && generation.mode !== "video");

  /** Anlas for the portrait on NovelAI, or null elsewhere. */
  const anlas = $derived.by(() => {
    if (!generation.isNovelAi) return null;
    return estimateNovelAiCost({
      width: size.width,
      height: size.height,
      steps: generation.novelAiSteps,
      mediumEffort: generation.novelAiMediumEffort,
      nSamples: 1,
      strength: 1,
      isOpus: novelai.isOpus,
      opusExhausted: naiV5Variant(generation.checkpoint) !== null && novelai.opusAllowanceEmpty,
      vibeEncodes: 0,
      preciseReferences: 0,
    });
  });

  function toggle(list: string[], id: string): string[] {
    return list.includes(id) ? list.filter((x) => x !== id) : [...list, id];
  }

  function resetPrompts() {
    positive = initial.positive;
    negative = initial.negative;
  }

  async function generate() {
    if (!canGenerate) return;
    submitting = true;
    const extras = { artists: artists.trim(), chunkIds, styleIds };
    try {
      await generateCharacterPortrait(character, { positive, negative: showNegative ? negative : "" }, extras);
      savePortraitExtras(extras);
      gallery.showToast(locale.t("characters.portrait.queued", { name: character.name }), "info");
      onclose();
    } catch (e) {
      console.error("Character portrait failed:", e);
      gallery.showToast(e instanceof Error ? e.message : String(e), "error");
    } finally {
      submitting = false;
    }
  }

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onclose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  });

  const chipBase = "px-2 py-0.5 text-[11px] rounded-full border transition-colors max-w-[12rem] truncate";
  const chipOn = "border-neutral-400 bg-neutral-700 text-neutral-50";
  const chipOff = "border-neutral-700 text-neutral-400 hover:border-neutral-500 hover:text-neutral-200";
</script>

<div
  use:portal
  class="fixed inset-0 z-210 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4"
  role="dialog"
  aria-modal="true"
  aria-label={locale.t("characters.portrait.title", { name: character.name })}
>
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
        {locale.t("characters.portrait.title", { name: character.name })}
      </h3>
      <button
        type="button"
        class="text-neutral-500 hover:text-neutral-200 text-lg leading-none"
        onclick={onclose}
        aria-label={locale.t("common.cancel")}
      >✕</button>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto flex flex-col gap-3">
      <div class="flex flex-col gap-1">
        <div class="flex items-center justify-between gap-2">
          <label for="character-portrait-prompt" class="text-xs font-medium text-neutral-300">
            {locale.t("characters.portrait.prompt")}
          </label>
          <button
            type="button"
            class="w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800 disabled:opacity-40"
            title={locale.t("characters.portrait.reset")}
            aria-label={locale.t("characters.portrait.reset")}
            disabled={positive === initial.positive && negative === initial.negative}
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
      </div>

      {#if showNegative}
        <div class="flex flex-col gap-1">
          <label for="character-portrait-negative" class="text-xs font-medium text-neutral-300">
            {locale.t("characters.portrait.negative")}
          </label>
          <textarea
            id="character-portrait-negative"
            bind:value={negative}
            rows="2"
            spellcheck="false"
            class="w-full resize-y rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-[11px] leading-relaxed text-neutral-100 focus:outline-none focus:border-red-500/70"
          ></textarea>
        </div>
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
                title={preset.content}
                onclick={() => { chunkIds = toggle(chunkIds, preset.id); }}
              >{preset.name}</button>
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
                title={style.artists.map((a) => a.tag).join(", ")}
                onclick={() => { styleIds = toggle(styleIds, style.id); }}
              >{style.name}</button>
            {/each}
          </div>
        </div>
      {/if}
    </div>

    <div class="flex flex-wrap items-center justify-end gap-2">
      <span class="mr-auto text-[11px] text-neutral-500" title={locale.t("characters.portrait.size_tip")}>
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
