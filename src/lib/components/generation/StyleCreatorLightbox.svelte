<script lang="ts">
  /**
   * Full-screen view of the round in flight. The pair is compared on the whole
   * frame instead of side by side in the bottom panel, either one card at a
   * time or both at once, and the controls stay out of the way until the
   * pointer enters the frame.
   */
  import { styleCreator, type RoundCard } from "../../stores/styleCreator.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { stripArtistSigil } from "../../utils/artistTag.js";
  import type { StyleArtist } from "../../stores/styles.svelte.js";

  const cards = $derived(styleCreator.round?.cards ?? []);
  // The round can shrink between renders (a second card that could not be
  // drawn), so the stored index is clamped rather than trusted.
  const index = $derived(Math.min(styleCreator.viewerIndex, Math.max(cards.length - 1, 0)));
  const card = $derived(cards[index] ?? null);
  const choosing = $derived(styleCreator.phase === "choosing");
  const open = $derived(styleCreator.viewerOpen && cards.length > 0);
  // With one card there is nothing to lay out beside it, so the pair controls
  // collapse to the single-card form regardless of the stored preference.
  const sideBySide = $derived(styleCreator.viewerSideBySide && cards.length > 1);
  // The anchor-alone card is a reference, not a candidate, so a round that
  // holds one has nothing to save "both" of.
  const saveableCount = $derived(cards.filter((c) => c.saveable).length);

  function chipLabel(artist: StyleArtist): string {
    const tag = generation.isNovelAi ? stripArtistSigil(artist.tag) : artist.tag;
    return artist.weight === 1 ? tag : `${tag}:${artist.weight}`;
  }

  function onKeydown(e: KeyboardEvent) {
    if (!open) return;
    // The name field owns its own keys: typing a left arrow in it must move
    // the caret, not the card.
    const target = e.target as HTMLElement | null;
    if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA")) return;
    if (e.key === "Escape") {
      e.preventDefault();
      styleCreator.closeViewer();
    } else if (!sideBySide && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
      e.preventDefault();
      styleCreator.switchCard();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#snippet frame(c: RoundCard | null)}
  {#if c?.image}
    <img src={c.image.url} alt="" class="h-full w-full object-contain" />
  {:else}
    <div class="absolute inset-0 flex flex-col items-center justify-center gap-3">
      <div
        class="h-8 w-8 animate-spin rounded-full border-2 border-indigo-500 border-t-transparent"
      ></div>
      {#if styleCreator.staggering}
        <p class="max-w-md px-6 text-center text-[11px] text-neutral-400">
          {locale.t("style_creator.nai_stagger")}
        </p>
      {/if}
    </div>
  {/if}
{/snippet}

{#snippet cardControls(c: RoundCard, i: number)}
  <div class="flex min-w-0 flex-col items-center gap-2">
    <div class="flex flex-wrap justify-center gap-1">
      {#each c.artists as artist, j (artist.tag + j)}
        <span class="rounded bg-neutral-800 px-1.5 py-0.5 text-[11px] text-neutral-200">
          {chipLabel(artist)}
        </span>
      {/each}
    </div>
    <div class="flex flex-wrap items-center justify-center gap-2">
      {#if c.saveable}
        <input
          type="text"
          value={c.name}
          placeholder={locale.t("style_creator.name")}
          oninput={(e) => styleCreator.setCardName(i, (e.currentTarget as HTMLInputElement).value)}
          class="w-48 rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-xs text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
        />
        <button
          type="button"
          class="rounded bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
          disabled={!choosing}
          onclick={() => void styleCreator.pick(i)}>{locale.t("style_creator.pick")}</button
        >
        <button
          type="button"
          class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-[11px] text-neutral-300 hover:text-indigo-200 disabled:opacity-50"
          disabled={!choosing}
          onclick={() => void styleCreator.pick(i, true)}
          >{locale.t("style_creator.pick_edit")}</button
        >
      {:else}
        <span class="text-[11px] text-neutral-500">{locale.t("style_creator.anchor_alone")}</span>
        <button
          type="button"
          class="rounded bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
          disabled={!choosing}
          onclick={() => void styleCreator.pick(i)}>{locale.t("style_creator.pick")}</button
        >
      {/if}
    </div>
  </div>
{/snippet}

{#if open}
  <div class="fixed inset-0 z-[60] flex flex-col bg-black/95">
    <button
      type="button"
      class="absolute right-3 top-3 z-10 rounded border border-neutral-700 bg-neutral-900/80 px-2 py-1 text-xs text-neutral-300 hover:border-indigo-500 hover:text-indigo-200"
      aria-label={locale.t("common.close")}
      title={locale.t("common.close")}
      onclick={() => styleCreator.closeViewer()}>x</button
    >

    <div class="group relative min-h-0 flex-1">
      {#if sideBySide}
        <div class="flex h-full w-full gap-1">
          {#each cards as c, i (i)}
            <div class="relative min-w-0 flex-1">{@render frame(c)}</div>
          {/each}
        </div>
      {:else}
        {@render frame(card)}
      {/if}

      <!-- Controls: hidden until the pointer is over the frame, so the image
           is judged on its own. focus-within keeps them up for keyboard use. -->
      <div
        class="pointer-events-none absolute inset-x-0 bottom-0 flex justify-center p-4 opacity-0 transition-opacity duration-150 focus-within:opacity-100 group-hover:opacity-100"
      >
        <div
          class="pointer-events-auto flex max-w-full flex-col items-center gap-2 rounded-lg border border-neutral-800 bg-neutral-950/90 p-3 shadow-xl"
        >
          {#if sideBySide}
            <div class="flex flex-wrap items-start justify-center gap-6">
              {#each cards as c, i (i)}
                {@render cardControls(c, i)}
              {/each}
            </div>
          {:else if card}
            {@render cardControls(card, index)}
          {/if}

          {#if styleCreator.error}
            <p class="text-[11px] text-red-400">{styleCreator.error}</p>
          {/if}
          {#if styleCreator.note}
            <p class="text-[11px] text-amber-400">{styleCreator.note}</p>
          {/if}

          <div class="flex flex-wrap items-center justify-center gap-2">
            {#if cards.length > 1}
              {#if !sideBySide}
                <button
                  type="button"
                  class="rounded border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:border-indigo-500"
                  onclick={() => styleCreator.switchCard()}
                >
                  {locale.t("style_creator.switch_card")}
                  <span class="ml-1 text-neutral-500">{index + 1}/{cards.length}</span>
                </button>
              {/if}
              <button
                type="button"
                class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-[11px] text-neutral-300 hover:text-indigo-200"
                onclick={() => styleCreator.setViewerSideBySide(!sideBySide)}
                >{sideBySide
                  ? locale.t("style_creator.view_single")
                  : locale.t("style_creator.view_side_by_side")}</button
              >
            {/if}

            {#if saveableCount > 1}
              <button
                type="button"
                class="rounded bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
                disabled={!choosing}
                onclick={() => void styleCreator.pickAll()}
                >{locale.t("style_creator.pick_both")}</button
              >
            {/if}
            <button
              type="button"
              class="rounded border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:border-indigo-500 disabled:opacity-50"
              disabled={!choosing}
              onclick={() => styleCreator.skip()}>{locale.t("style_creator.skip")}</button
            >
            <!-- Rounds keep drawing until they are stopped, so the stop has to
                 be reachable without leaving the lightbox. -->
            {#if styleCreator.running}
              <button
                type="button"
                class="rounded bg-neutral-700 px-3 py-1.5 text-xs font-medium text-white hover:bg-neutral-600"
                onclick={() => styleCreator.stop()}>{locale.t("style_creator.stop")}</button
              >
            {/if}
          </div>
        </div>
      </div>
    </div>
  </div>
{/if}
