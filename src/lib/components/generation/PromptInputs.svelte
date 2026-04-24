<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { artistFavourites } from "../../artist-gallery/favourites.svelte.js";
  import { detectArtistsInPrompt } from "../../artist-gallery/detection.js";
  import { styles } from "../../stores/styles.svelte.js";
  import { promptPresets } from "../../stores/promptPresets.svelte.js";
  import PromptTextarea from "./PromptTextarea.svelte";
  import InfoTip from "../ui/InfoTip.svelte";
  import { parseScheduledPrompt, hasSchedulingTags } from "../../utils/promptSchedule.js";

  interface Props {
    showHistory?: boolean;
  }

  let { showHistory = true }: Props = $props();

  const hasPositiveSchedule = $derived(hasSchedulingTags(generation.positivePrompt));
  const hasNegativeSchedule = $derived(hasSchedulingTags(generation.negativePrompt));
  const hasAnySchedule = $derived(hasPositiveSchedule || hasNegativeSchedule);
  const positiveSegments = $derived(hasPositiveSchedule ? parseScheduledPrompt(generation.positivePrompt).segments : []);
  const negativeSegments = $derived(hasNegativeSchedule ? parseScheduledPrompt(generation.negativePrompt).segments : []);
  let schedulePanelOpen = $state(true);

  // Lazy-load the artist tag index so typing an artist in the prompt without
  // ever opening the gallery still lights up the heart chip.
  $effect(() => {
    if (!gallery.artistIndexReady && connection.artistGalleryManifestUrl) {
      void gallery.loadArtistIndex(connection.artistGalleryManifestUrl);
    }
  });

  /** Artist tags detected in the current positive prompt. */
  const detectedArtists = $derived.by(() => {
    if (!gallery.artistIndexReady || gallery.artistTagIndex.size === 0) return [];
    return detectArtistsInPrompt(generation.positivePrompt, gallery.artistTagIndex);
  });

  const sortedPromptHistory = $derived(
    [...generation.promptHistory].sort((a, b) => {
      if (a.favorite !== b.favorite) return a.favorite ? -1 : 1;
      return b.createdAt - a.createdAt;
    }).slice(0, 12)
  );
  let historySectionOpen = $state(true);

  function historyLabel(ts: number): string {
    return new Date(ts).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }
</script>

<div class="space-y-2">
  {#if generation.stylePresetsEnabled}
    <div>
      <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.prompts.style_preset')}<InfoTip text={locale.t('generation.prompts.style_preset_tip')} /></label>
      <select
        bind:value={generation.stylePreset}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        {#each generation.stylePresetOptions as preset}
          <option value={preset.id}>{preset.label}</option>
        {/each}
      </select>
    </div>
  {/if}

  <div>
    <div class="flex items-center justify-between gap-2 mb-1">
      <div class="flex items-center gap-1.5 shrink-0">
        <label class="text-xs text-neutral-400">{locale.t('generation.prompts.positive')}<InfoTip text={locale.t('generation.prompts.positive_tip')} /></label>
      </div>
      <div class="flex items-center justify-end gap-1.5 flex-wrap min-w-0">
      {#if generation.isAnima || generation.isIllustrious}
        <span class="shrink-0 text-[10px] px-2 py-0.5 rounded-full bg-emerald-600/20 text-emerald-400 border border-emerald-600/30">{locale.t('generation.prompts.quality_applied')}</span>
      {/if}
      {#each styles.activeStyles as activeStyle (activeStyle.id)}
        <button
          type="button"
          onclick={() => styles.deactivate(activeStyle.id)}
          class="shrink-0 inline-flex items-center gap-1 rounded-full border border-indigo-500/50 bg-indigo-500/10 text-indigo-200 hover:bg-red-500/15 hover:border-red-500/50 hover:text-red-200 px-2 py-0.5 text-[10px] transition-colors"
          title={`Click to deactivate — ${activeStyle.artists.length} artists × ${activeStyle.overallWeight.toFixed(2)}`}
          aria-label={`Deactivate style ${activeStyle.name}`}
        >
          {#if activeStyle.thumbnail}
            <img src={activeStyle.thumbnail} alt="" class="h-3.5 w-3.5 rounded-sm object-cover" />
          {:else}
            <span class="inline-block h-1.5 w-1.5 rounded-full bg-indigo-400" aria-hidden="true"></span>
          {/if}
          <span class="leading-none">✦</span>
          <span class="max-w-28 truncate">{activeStyle.name}</span>
          <span class="font-mono text-[9px] text-indigo-300/80">×{activeStyle.overallWeight.toFixed(2)}</span>
        </button>
      {/each}
      {#each promptPresets.activeEntries as entry (entry.preset.id)}
        {@const icon = entry.mode === "prepend" ? "↑" : entry.mode === "append" ? "↓" : "🎲"}
        <button
          type="button"
          onclick={() => promptPresets.deactivate(entry.preset.id)}
          class="shrink-0 inline-flex items-center gap-1 rounded-full border border-indigo-500/50 bg-indigo-500/10 text-indigo-200 hover:bg-red-500/15 hover:border-red-500/50 hover:text-red-200 px-2 py-0.5 text-[10px] transition-colors"
          title={`Click to deactivate — ${entry.mode}`}
          aria-label={`Deactivate preset ${entry.preset.name}`}
        >
          <span class="leading-none">⚡</span>
          <span class="max-w-28 truncate">{entry.preset.name}</span>
          <span class="font-mono text-[9px] text-indigo-300/80">{icon}</span>
        </button>
      {/each}
      {#each detectedArtists as hit (hit.slug)}
        {@const isFav = artistFavourites.isFavourite(hit.slug)}
        {@const favCat = artistFavourites.categoryOf(hit.slug)}
        {@const displayName = hit.tag.replace(/^@/, "").replace(/\\([()\[\]])/g, "$1")}
        <button
          type="button"
          onclick={() => artistFavourites.toggle(hit.slug)}
          class="shrink-0 inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] transition-colors {isFav ? 'border-red-500/50 bg-red-500/10 text-red-300 hover:bg-red-500/20' : 'border-neutral-700 bg-neutral-800/60 text-neutral-400 hover:border-red-500/60 hover:text-red-300'}"
          title={isFav ? `Unfavourite ${hit.tag}` : `Favourite ${hit.tag}`}
          aria-label={isFav ? `Unfavourite artist ${displayName}` : `Favourite artist ${displayName}`}
        >
          {#if favCat}
            <span class="h-2 w-2 rounded-full border border-black/20" style="background-color: {favCat.color}" aria-hidden="true"></span>
          {/if}
          <span class="leading-none">{isFav ? '♥' : '♡'}</span>
          <span class="font-mono max-w-28 truncate">@{displayName}</span>
        </button>
      {/each}
      </div>
    </div>
    {#if generation.isAnima}
      <div class="text-[10px] text-amber-400/80 mb-1">{locale.t('generation.prompts.anima_artist_tip')}</div>
    {/if}
    <PromptTextarea
      bind:value={generation.positivePrompt}
      placeholder={generation.isAnima ? "1girl, long hair, @artist_name, ..." : "A beautiful landscape, golden hour lighting, ..."}
      rows={4}
      minHeight="min-h-25"
      storageKey="mooshieui.promptHeight.positive"
    />
  </div>

  <div>
    <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.prompts.negative')}<InfoTip text={locale.t('generation.prompts.negative_tip')} /></label>
    <PromptTextarea
      bind:value={generation.negativePrompt}
      placeholder={locale.t('generation.prompts.negative_placeholder')}
      rows={3}
      minHeight="min-h-18"
      storageKey="mooshieui.promptHeight.negative"
    />
  </div>

  {#if hasAnySchedule}
    <div class="rounded-lg border border-neutral-800 bg-neutral-900/50 p-2.5 space-y-2">
      <button
        class="w-full text-left flex items-center justify-between text-xs text-neutral-400 hover:text-neutral-200 transition-colors"
        onclick={() => (schedulePanelOpen = !schedulePanelOpen)}
      >
        <span class="flex items-center gap-1.5">
          <span class="inline-block w-2 h-2 rounded-full bg-amber-400/60"></span>
          {locale.t('generation.prompts.scheduling')}
          <span class="text-[10px] text-neutral-500">({locale.t('generation.prompts.scheduling_segments', { count: String(positiveSegments.length + negativeSegments.length) })})</span>
        </span>
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {schedulePanelOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
      </button>
      {#if schedulePanelOpen}
        <div class="space-y-1.5">
          {#each positiveSegments as seg, i}
            <div class="flex items-center gap-2 rounded border border-amber-400/20 bg-amber-400/5 px-2 py-1.5">
              <span class="text-[10px] text-amber-300 shrink-0">+{i + 1}</span>
              <div class="flex-1 min-w-0">
                <p class="text-[11px] text-neutral-200 truncate">{seg.text}</p>
                <div class="mt-1 h-1.5 w-full rounded-full bg-neutral-800 overflow-hidden">
                  <div
                    class="h-full rounded-full bg-amber-400/50"
                    style="margin-left: {seg.start * 100}%; width: {(seg.end - seg.start) * 100}%;"
                  ></div>
                </div>
              </div>
              <span class="text-[10px] text-neutral-500 shrink-0">{Math.round(seg.start * 100)}%–{Math.round(seg.end * 100)}%</span>
            </div>
          {/each}
          {#each negativeSegments as seg, i}
            <div class="flex items-center gap-2 rounded border border-amber-400/20 bg-amber-400/5 px-2 py-1.5">
              <span class="text-[10px] text-amber-300 shrink-0">-{i + 1}</span>
              <div class="flex-1 min-w-0">
                <p class="text-[11px] text-neutral-200 truncate">{seg.text}</p>
                <div class="mt-1 h-1.5 w-full rounded-full bg-neutral-800 overflow-hidden">
                  <div
                    class="h-full rounded-full bg-amber-400/50"
                    style="margin-left: {seg.start * 100}%; width: {(seg.end - seg.start) * 100}%;"
                  ></div>
                </div>
              </div>
              <span class="text-[10px] text-neutral-500 shrink-0">{Math.round(seg.start * 100)}%–{Math.round(seg.end * 100)}%</span>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  {#if showHistory && sortedPromptHistory.length > 0}
    <div class="rounded-lg border border-neutral-800 bg-neutral-900/50 p-2.5 space-y-2">
      <div class="flex items-center justify-between">
        <button
          class="w-full text-left flex items-center justify-between text-xs text-neutral-400 hover:text-neutral-200 transition-colors"
          onclick={() => (historySectionOpen = !historySectionOpen)}
          title={historySectionOpen ? "Collapse Prompt History & Favorites" : "Expand Prompt History & Favorites"}
        >
          <span>{locale.t('generation.prompts.history')}<InfoTip text={locale.t('generation.prompts.history_tip')} /></span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {historySectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
      </div>
      {#if historySectionOpen}
        <div class="space-y-1.5 max-h-56 overflow-y-auto pr-1">
          {#each sortedPromptHistory as entry}
            <div class="rounded border border-neutral-800 bg-neutral-900/80 p-2">
              <button
                class="w-full text-left"
                onclick={() => generation.applyPromptHistoryEntry(entry.id)}
                title={locale.t('bottom_panel.load_prompt')}
              >
                <p class="text-[11px] text-neutral-200 max-h-8 overflow-hidden">{entry.positivePrompt || locale.t('bottom_panel.empty_prompt')}</p>
                {#if entry.negativePrompt}
                  <p class="text-[10px] text-neutral-500 mt-0.5 whitespace-nowrap overflow-hidden text-ellipsis">{locale.t('bottom_panel.neg_prefix')} {entry.negativePrompt}</p>
                {/if}
              </button>
              <div class="mt-1.5 flex items-center justify-between gap-2">
                <span class="text-[10px] text-neutral-500">{historyLabel(entry.createdAt)}</span>
                <div class="flex items-center gap-1">
                  <button
                    class="px-1.5 py-0.5 text-[10px] rounded border transition-colors {entry.favorite ? 'border-amber-500 text-amber-300 bg-amber-500/10' : 'border-neutral-700 text-neutral-400 hover:border-neutral-500 hover:text-neutral-300'}"
                    onclick={() => generation.togglePromptFavorite(entry.id)}
                    title={entry.favorite ? locale.t('bottom_panel.unfavorite') : locale.t('bottom_panel.favorite')}
                  >
                    ★
                  </button>
                  <button
                    class="px-1.5 py-0.5 text-[10px] rounded border border-neutral-700 text-neutral-400 hover:border-red-500 hover:text-red-300 transition-colors"
                    onclick={() => generation.removePromptHistoryEntry(entry.id)}
                    title={locale.t('common.remove')}
                  >
                    {locale.t('common.remove')}
                  </button>
                </div>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>
