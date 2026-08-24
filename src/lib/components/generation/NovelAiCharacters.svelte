<script lang="ts">
  /**
   * Per-character prompts for NovelAI's V4+ structured prompt block.
   *
   * NovelAI splits a scene into a base prompt plus one block per character,
   * each with its own undesired content and an optional placement. Placement
   * is a 5 by 5 grid in its own UI; the coordinates it sends are the cell
   * centres (0.1, 0.3, 0.5, 0.7, 0.9), which is what `NovelAiCoord::from_grid`
   * produces on the Rust side.
   */
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { NOVELAI_MAX_CHARACTERS } from "../../utils/novelaiModels.js";
  import InfoTip from "../ui/InfoTip.svelte";
  import NovelAiCharacterPrompt from "./NovelAiCharacterPrompt.svelte";

  const GRID = [0, 1, 2, 3, 4];

  /** Cell centre for a 5x5 grid index, matching `NovelAiCoord::from_grid`. */
  function cellCentre(index: number): number {
    return (index * 2 + 1) / 10;
  }

  /** Which grid cell a stored coordinate falls in, for highlighting. */
  function cellIndex(value: number): number {
    return Math.min(4, Math.max(0, Math.round((value * 10 - 1) / 2)));
  }

  const characters = $derived(generation.novelaiSettings.characters);
  const atLimit = $derived(characters.length >= NOVELAI_MAX_CHARACTERS);
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

        {#if generation.novelaiSettings.use_coords}
          <div class="space-y-1">
            <span class="text-[11px] text-neutral-500">
              {locale.t("generation.novelai.characters.position")}
            </span>
            <div class="grid grid-cols-5 gap-1 w-fit">
              {#each GRID as row (row)}
                {#each GRID as col (col)}
                  {@const active =
                    cellIndex(character.center.x) === col && cellIndex(character.center.y) === row}
                  <button
                    class="w-6 h-6 rounded border transition-colors {active
                      ? 'bg-indigo-600 border-indigo-500'
                      : 'bg-neutral-950 border-neutral-800 hover:border-neutral-600'}"
                    aria-label={locale.t("generation.novelai.characters.position_cell", {
                      col: String(col + 1),
                      row: String(row + 1),
                    })}
                    aria-pressed={active}
                    onclick={() =>
                      generation.updateNovelAiCharacter(index, {
                        center: { x: cellCentre(col), y: cellCentre(row) },
                      })}
                  ></button>
                {/each}
              {/each}
            </div>
          </div>
        {/if}
      </div>
    {/each}

    <div class="flex items-center justify-between">
      <label class="flex items-center gap-2 text-xs text-neutral-300">
        <input
          type="checkbox"
          class="accent-indigo-500"
          checked={generation.novelaiSettings.use_coords}
          onchange={(e) =>
            generation.updateNovelAiSettings({ use_coords: e.currentTarget.checked })}
        />
        {locale.t("generation.novelai.characters.use_coords")}
      </label>
    </div>
    <p class="text-[11px] text-neutral-500">
      {locale.t("generation.novelai.characters.use_coords_desc")}
    </p>

    {#if atLimit}
      <p class="text-[11px] text-neutral-500">
        {locale.t("generation.novelai.characters.limit", { count: String(NOVELAI_MAX_CHARACTERS) })}
      </p>
    {/if}
  {/if}
</div>
