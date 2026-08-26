<script lang="ts">
  /**
   * Per-character prompts for NovelAI's V4+ structured prompt block.
   *
   * NovelAI splits a scene into a base prompt plus one block per character,
   * each with its own undesired content and an optional placement. Placement
   * is either left to NovelAI or set by hand; the hand-placed case opens the
   * position canvas in a modal (NovelAiPositionModal, mounted at the app root),
   * because at panel width the canvas was too small to place anything
   * precisely. Positions saved by the old inline canvas, and by the 5 by 5 grid
   * before it, load unchanged: all three write the same normalised 0..1 centre.
   */
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { novelai } from "../../stores/novelai.svelte.js";
  import { novelAiMaxCharacters } from "../../utils/novelaiModels.js";
  import InfoTip from "../ui/InfoTip.svelte";
  import NovelAiCharacterPrompt from "./NovelAiCharacterPrompt.svelte";

  const characters = $derived(generation.novelaiSettings.characters);
  const maxCharacters = $derived(novelAiMaxCharacters(generation.checkpoint));
  const atLimit = $derived(characters.length >= maxCharacters);

  /** Nothing to place until at least one character is enabled. */
  const hasEnabledCharacter = $derived(characters.some((character) => character.enabled));
</script>

<div class="space-y-3">
  <div class="flex items-center justify-between">
    <label class="text-xs text-neutral-400">
      {locale.t("generation.novelai.characters.title")}
      <InfoTip text={locale.t("generation.novelai.characters.tip")} />
    </label>
    <button
      class="px-2 py-1 text-[11px] rounded-md bg-neutral-800 hover:bg-neutral-700 text-neutral-200 disabled:opacity-40 disabled:cursor-not-allowed"
      onclick={() => generation.addNovelAiCharacter()}
      disabled={atLimit}
    >
      {locale.t("generation.novelai.characters.add")}
    </button>
  </div>

  {#if !generation.supportsNovelAiCharacters}
    <p class="text-[11px] text-amber-400/90">
      {locale.t("generation.novelai.characters.unsupported")}
    </p>
  {:else}
    {#if characters.length === 0}
      <p class="text-[11px] text-neutral-500">
        {locale.t("generation.novelai.characters.empty")}
      </p>
    {/if}

    {#each characters as character, index (index)}
      <div class="rounded-lg border border-neutral-800 bg-neutral-900/60 p-3 space-y-2">
        <div class="flex items-center justify-between gap-2">
          <label class="flex items-center gap-2 text-xs text-neutral-300">
            <input
              type="checkbox"
              class="accent-indigo-500"
              checked={character.enabled}
              onchange={(e) =>
                generation.updateNovelAiCharacter(index, {
                  enabled: e.currentTarget.checked,
                })}
            />
            {locale.t("generation.novelai.characters.label", { index: String(index + 1) })}
          </label>
          <button
            class="px-2 py-0.5 text-[11px] rounded-md text-neutral-400 hover:text-red-400 hover:bg-neutral-800"
            title={locale.t("generation.novelai.characters.remove")}
            onclick={() => generation.removeNovelAiCharacter(index)}
          >
            {locale.t("generation.novelai.characters.remove")}
          </button>
        </div>

        <NovelAiCharacterPrompt
          {index}
          field="prompt"
          value={character.prompt}
          placeholder={locale.t("generation.novelai.characters.prompt_placeholder")}
          minHeight="min-h-16"
        />

        <NovelAiCharacterPrompt
          {index}
          field="negative_prompt"
          value={character.negative_prompt}
          placeholder={locale.t("generation.novelai.characters.negative_placeholder")}
          minHeight="min-h-12"
        />
      </div>
    {/each}

    <div class="space-y-1.5">
      <span class="text-[11px] text-neutral-500">
        {locale.t("generation.novelai.characters.position")}
      </span>
      <div class="flex items-center gap-2">
        <div class="flex gap-1 bg-neutral-900 rounded-lg p-1">
          <button
            class="px-2.5 py-0.5 text-[11px] rounded-md transition-colors {generation
              .novelaiSettings.use_coords
              ? 'text-neutral-400 hover:text-neutral-200'
              : 'bg-neutral-700 text-white'}"
            onclick={() => generation.updateNovelAiSettings({ use_coords: false })}
          >
            {locale.t("generation.novelai.characters.position_auto")}
          </button>
          <button
            class="px-2.5 py-0.5 text-[11px] rounded-md transition-colors {generation
              .novelaiSettings.use_coords
              ? 'bg-neutral-700 text-white'
              : 'text-neutral-400 hover:text-neutral-200'}"
            onclick={() => generation.updateNovelAiSettings({ use_coords: true })}
          >
            {locale.t("generation.novelai.characters.position_custom")}
          </button>
        </div>
        {#if generation.novelaiSettings.use_coords}
          <button
            class="px-2 py-1 text-[11px] rounded-md bg-neutral-800 hover:bg-neutral-700 text-neutral-200 disabled:opacity-40 disabled:cursor-not-allowed"
            onclick={() => (novelai.characterPositionOpen = true)}
            disabled={!hasEnabledCharacter}
          >
            {locale.t("generation.novelai.characters.edit_positions")}
          </button>
        {/if}
      </div>
      <p class="text-[11px] text-neutral-500">
        {locale.t("generation.novelai.characters.use_coords_desc")}
      </p>
    </div>

    {#if atLimit}
      <p class="text-[11px] text-neutral-500">
        {locale.t("generation.novelai.characters.limit", { count: String(maxCharacters) })}
      </p>
    {/if}
  {/if}
</div>
