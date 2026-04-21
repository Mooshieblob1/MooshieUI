<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { scrollCapture } from "../../utils/scrollCapture.js";
  import { models } from "../../stores/models.svelte.js";
  import { lazyThumbnail } from "../../utils/lazyThumbnail.js";
  import { compare } from "../../stores/compare.svelte.js";
  import { artistFavourites } from "../../artist-gallery/favourites.svelte.js";
  import { createArtistGalleryStore } from "../../artist-gallery/store.svelte.js";
  import { artistInsert } from "../../stores/artistInsert.svelte.js";
  import type { ArtistSearchHit } from "../../artist-gallery/types.js";
  import LoraGallery from "./LoraGallery.svelte";
  import CheckpointGallery from "./CheckpointGallery.svelte";
  import CompareGrid from "./CompareGrid.svelte";
  import type { OutputImage } from "../../types/index.js";

  interface Props {
    onupscale: (image: OutputImage) => void;
    oninpaint: (image: OutputImage) => void;
    oncontextmenu?: (image: OutputImage, x: number, y: number) => void;
  }

  let { onupscale, oninpaint, oncontextmenu }: Props = $props();

  type TabId = "loras" | "checkpoints" | "images" | "prompts" | "compare" | "artists";

  const TAB_KEY = "mooshieui.bottomPanel.activeTab.v1";

  const showCheckpointsTab = $derived(models.checkpoints.length > 10 || generation.devMode);

  let activeTab = $state<TabId>(
    (typeof window !== "undefined" && (localStorage.getItem(TAB_KEY) as TabId | null)) || "loras"
  );

  // If the checkpoints tab disappears (count dropped), fall back to loras
  $effect(() => {
    if (activeTab === "checkpoints" && !showCheckpointsTab) {
      activeTab = "loras";
    }
  });

  $effect(() => {
    try { localStorage.setItem(TAB_KEY, activeTab); } catch {}
  });

  const allTabs: TabId[] = ["loras", "checkpoints", "images", "prompts", "artists", "compare"];
  const visibleTabs = $derived(
    showCheckpointsTab ? allTabs : allTabs.filter((t) => t !== "checkpoints")
  );
  const tabLabelKeys: Record<TabId, string> = {
    loras: "bottom_panel.tab.loras",
    checkpoints: "bottom_panel.tab.checkpoints",
    images: "bottom_panel.tab.images",
    prompts: "bottom_panel.tab.prompts",
    compare: "bottom_panel.tab.compare",
    artists: "bottom_panel.tab.artists",
  };

  // Prompt history
  const sortedPromptHistory = $derived(
    [...generation.promptHistory]
      .sort((a, b) => {
        if (a.favorite !== b.favorite) return a.favorite ? -1 : 1;
        return b.createdAt - a.createdAt;
      })
  );

  function historyLabel(ts: number): string {
    return new Date(ts).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  // Badge counts
  const activeLoraCount = $derived(
    generation.loras.filter((l) => l.enabled && l.name).length
  );
  const sessionImageCount = $derived(gallery.sessionImages.length);
  const favoriteCount = $derived(
    generation.promptHistory.filter((p) => p.favorite).length
  );

  let imageSearch = $state("");
  let promptSearch = $state("");

  const filteredSessionImages = $derived.by(() => {
    const q = imageSearch.toLowerCase().trim();
    if (!q) return gallery.sessionImages;
    return gallery.sessionImages.filter((img) =>
      img.filename.toLowerCase().includes(q)
    );
  });

  const filteredPromptHistory = $derived.by(() => {
    const q = promptSearch.toLowerCase().trim();
    if (!q) return sortedPromptHistory;
    return sortedPromptHistory.filter(
      (entry) =>
        (entry.positivePrompt || "").toLowerCase().includes(q) ||
        (entry.negativePrompt || "").toLowerCase().includes(q)
    );
  });

  // Card size sliders (persisted)
  const CARD_SIZE_KEY = "mooshieui.bottomPanel.cardSize.v1";

  function loadCardSizes(): { lora: number; image: number } {
    try {
      const raw = localStorage.getItem(CARD_SIZE_KEY);
      if (raw) return JSON.parse(raw);
    } catch {}
    return { lora: 120, image: 72 };
  }

  const savedSizes = loadCardSizes();
  let loraCardSize = $state(savedSizes.lora);
  let imageCardSize = $state(savedSizes.image);

  $effect(() => {
    try {
      localStorage.setItem(CARD_SIZE_KEY, JSON.stringify({ lora: loraCardSize, image: imageCardSize }));
    } catch {}
  });

  // ---------------------------------------------------------------------------
  // Favourite artists tab
  // ---------------------------------------------------------------------------
  const artistStore = $derived(
    connection.artistGalleryManifestUrl
      ? createArtistGalleryStore(connection.artistGalleryManifestUrl)
      : null
  );

  // Ensure the manifest + search index are loaded the first time the tab is
  // viewed so cards can render thumbnails without visiting the gallery page.
  $effect(() => {
    if (activeTab !== "artists" || !artistStore) return;
    if (!artistStore.manifest && !artistStore.manifestLoading) {
      void artistStore.init();
    }
    if (artistStore.allEntries.length === 0 && !artistStore.allEntriesLoading) {
      artistStore.allEntriesLoading = true;
      artistStore.client
        .loadSearchIndex()
        .then((entries) => {
          artistStore.allEntries = entries;
        })
        .catch((err) => {
          artistStore.allEntriesError = err instanceof Error ? err.message : String(err);
        })
        .finally(() => {
          artistStore.allEntriesLoading = false;
        });
    }
  });

  let artistSearch = $state("");
  let artistCategoryFilter = $state<"all" | "__uncat" | string>("all");
  let artistCardSize = $state(110);

  const ARTIST_CARD_SIZE_KEY = "mooshieui.bottomPanel.artistCardSize.v1";
  try {
    const raw = localStorage.getItem(ARTIST_CARD_SIZE_KEY);
    if (raw) {
      const n = parseInt(raw, 10);
      if (!isNaN(n)) artistCardSize = Math.max(72, Math.min(200, n));
    }
  } catch {}
  $effect(() => {
    try { localStorage.setItem(ARTIST_CARD_SIZE_KEY, String(artistCardSize)); } catch {}
  });

  /** Resolve a favourite slug → ArtistSearchHit from the loaded index. */
  const favouriteArtistHits = $derived.by((): ArtistSearchHit[] => {
    const favMap = artistFavourites.favourites;
    const entries = artistStore?.allEntries ?? [];
    const bySlug = new Map<string, ArtistSearchHit>();
    for (const e of entries) bySlug.set(e.slug, e);
    const hits: ArtistSearchHit[] = [];
    // Sort favourites by addedAt desc (most recent first).
    const favs = Object.values(favMap).sort((a, b) => b.addedAt - a.addedAt);
    for (const fav of favs) {
      const hit = bySlug.get(fav.slug);
      if (hit) {
        hits.push(hit);
      } else {
        // Index not yet loaded (or tag gone from index). Provide a stub so
        // the card still renders with a working tag + heart.
        hits.push({
          slug: fav.slug,
          tag: `@${fav.slug}`,
          imageId: "",
          postCount: 0,
          shard: "",
          hasImage: false,
        });
      }
    }
    return hits;
  });

  const filteredFavouriteArtists = $derived.by(() => {
    const q = artistSearch.toLowerCase().trim();
    let list = favouriteArtistHits;
    if (artistCategoryFilter !== "all") {
      list = list.filter((hit) => {
        const fav = artistFavourites.favourites[hit.slug];
        if (!fav) return false;
        if (artistCategoryFilter === "__uncat") return fav.categoryId === null;
        return fav.categoryId === artistCategoryFilter;
      });
    }
    if (q) {
      list = list.filter(
        (hit) =>
          hit.slug.toLowerCase().includes(q) || hit.tag.toLowerCase().includes(q)
      );
    }
    return list;
  });

  function artistThumbUrl(hit: ArtistSearchHit): string {
    const m = artistStore?.manifest;
    if (!m || !hit.hasImage || !hit.imageId) return "";
    return `${m.imageBaseUrl}/${m.releasePrefix}/images/${hit.imageId}.webp`;
  }

  function applyArtistTag(hit: ArtistSearchHit) {
    // Delegate to the shared store so the same replace/append confirmation
    // modal that the gallery page uses is reused here.
    artistInsert.request(hit.slug);
  }

  function displayArtistTag(tag: string): string {
    return tag.replace(/^@/, "").replace(/\\([()[\]]])/g, "$1").replace(/_/g, " ");
  }
</script>

<div class="flex flex-col h-full">
  <!-- Tab bar -->
  <div class="flex items-center gap-0.5 px-2 pt-1 pb-0.5 border-b border-neutral-800 shrink-0">
    {#each visibleTabs as tab}
      <button
        onclick={() => { activeTab = tab; }}
        class="px-3 py-1.5 text-[11px] font-medium rounded-t-md transition-colors flex items-center gap-1.5 {activeTab === tab
          ? 'bg-neutral-800/80 text-neutral-100 border-b-2 border-indigo-500'
          : 'text-neutral-500 hover:text-neutral-300 hover:bg-neutral-800/40'}"
      >
        {#if tab === "loras"}
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2L2 7l10 5 10-5-10-5z"/><path d="M2 17l10 5 10-5"/><path d="M2 12l10 5 10-5"/></svg>
        {:else if tab === "checkpoints"}
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/><polyline points="3.27 6.96 12 12.01 20.73 6.96"/><line x1="12" y1="22.08" x2="12" y2="12"/></svg>
        {:else if tab === "images"}
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>
        {:else if tab === "prompts"}
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
        {:else if tab === "compare"}
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/></svg>
        {:else if tab === "artists"}
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/></svg>
        {/if}
        {locale.t(tabLabelKeys[tab])}
        {#if tab === "loras" && activeLoraCount > 0}
          <span class="text-[9px] px-1 py-0 rounded-full bg-indigo-600/30 text-indigo-400 tabular-nums">{activeLoraCount}</span>
        {:else if tab === "images" && sessionImageCount > 0}
          <span class="text-[9px] px-1 py-0 rounded-full bg-indigo-600/30 text-indigo-400 tabular-nums">{sessionImageCount}</span>
        {:else if tab === "prompts" && favoriteCount > 0}
          <span class="text-[9px] px-1 py-0 rounded-full bg-amber-500/30 text-amber-400 tabular-nums">{favoriteCount}</span>
        {:else if tab === "artists" && artistFavourites.count > 0}
          <span class="text-[9px] px-1 py-0 rounded-full bg-red-500/30 text-red-400 tabular-nums">{artistFavourites.count}</span>
        {:else if tab === "compare" && compare.enabled}
          <span class="text-[9px] px-1 py-0 rounded-full bg-indigo-600/30 text-indigo-400 tabular-nums">{compare.cellCount}</span>
        {/if}
      </button>
    {/each}
  </div>

  <!-- Tab content -->
  <div class="flex-1 min-h-0 overflow-hidden">
    {#if activeTab === "loras"}
      <LoraGallery cardSize={loraCardSize} onCardSizeChange={(s) => { loraCardSize = s; }} />    {:else if activeTab === "checkpoints"}
      <CheckpointGallery />    {:else if activeTab === "images"}
      <!-- Storage expiry warning (browser mode) -->
      {#if gallery.hasExpiry}
        <div class="mx-2 mt-1.5 mb-1 px-3 py-2 rounded-lg bg-amber-900/30 border border-amber-700/50 text-amber-300 text-[11px] flex items-center gap-2 shrink-0">
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
          <span>
            {locale.t('gallery.expiry_warning')}
            {#if gallery.expiringWithin24h > 0}
              <strong class="text-amber-200">{locale.t('gallery.expiry_soon', { count: String(gallery.expiringWithin24h) })}</strong>
            {/if}
            {#if gallery.storageInfo}
              <span class="text-amber-400/70 ml-1">({gallery.storageLabel})</span>
            {/if}
          </span>
        </div>
      {/if}
      <!-- Session History -->
      {#if gallery.sessionImages.length === 0}
        <div class="flex items-center justify-center h-full text-neutral-500 text-xs">
          <p>{locale.t('bottom_panel.no_images')}</p>
        </div>
      {:else}
        <div class="flex flex-col h-full">
          <div class="px-2 pt-1.5 pb-1 shrink-0 flex items-center gap-2">
            <input
              type="text"
              bind:value={imageSearch}
              placeholder={locale.t('bottom_panel.image_search_placeholder')}
              class="flex-1 bg-neutral-800 border border-neutral-700 rounded px-2.5 py-1 text-xs text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
            />
            <div use:scrollCapture>
              <input
                type="range"
                min="48"
                max="160"
                bind:value={imageCardSize}
                class="w-16 h-4 accent-indigo-500 cursor-pointer"
                title={locale.t('bottom_panel.card_size')}
              />
            </div>
          </div>
          {#if filteredSessionImages.length === 0}
            <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs">
              <p>{locale.t('bottom_panel.no_image_results')}</p>
            </div>
          {:else}
            <div class="grid gap-2 flex-1 min-h-0 overflow-y-auto px-2 py-2" style="grid-template-columns: repeat(auto-fill, minmax({imageCardSize}px, 1fr)); align-content: start;">
              {#each filteredSessionImages as image}
                <div class="group relative aspect-square rounded-lg overflow-hidden border border-neutral-800 hover:border-indigo-500 transition-colors" oncontextmenu={(e) => { if (oncontextmenu) { e.preventDefault(); oncontextmenu(image, e.clientX, e.clientY); } }}>
              <button
                class="w-full h-full"
                onclick={() => gallery.openLightbox(image)}
              >
                <img
                  use:lazyThumbnail={{ image }}
                  alt={image.filename}
                  class="w-full h-full object-cover"
                />
              </button>
              <div class="absolute inset-0 bg-black/50 opacity-0 group-hover:opacity-100 transition-opacity flex items-end justify-center p-1.5 pointer-events-none">
                <div class="flex gap-1 pointer-events-auto">
                  {#if !image.is_upscaled}
                    <button
                      class="w-7 h-7 flex items-center justify-center rounded bg-indigo-900/90 hover:bg-indigo-700 text-neutral-300"
                      title={locale.t('bottom_panel.upscale')}
                      onclick={(e) => { e.stopPropagation(); onupscale(image); }}
                    >
                      <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/><line x1="11" y1="8" x2="11" y2="14"/><line x1="8" y1="11" x2="14" y2="11"/></svg>
                    </button>
                  {/if}
                  <button
                    class="w-7 h-7 flex items-center justify-center rounded bg-indigo-900/90 hover:bg-indigo-700 text-neutral-300"
                    title={locale.t('bottom_panel.inpaint')}
                    onclick={(e) => { e.stopPropagation(); oninpaint(image); }}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/></svg>
                  </button>
                  <button
                    class="w-7 h-7 flex items-center justify-center rounded bg-neutral-900/95 hover:bg-neutral-700 text-neutral-300 disabled:opacity-50 disabled:cursor-not-allowed"
                    title={locale.t('bottom_panel.save_as')}
                    disabled={gallery.saving}
                    onclick={(e) => { e.stopPropagation(); gallery.saveImageAs(image); }}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                  </button>
                  <button
                    class="w-7 h-7 flex items-center justify-center rounded bg-neutral-900/95 hover:bg-neutral-700 text-neutral-300"
                    title={locale.t('bottom_panel.copy')}
                    onclick={(e) => { e.stopPropagation(); gallery.copyToClipboard(image); }}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
                  </button>
                  <button
                    class="w-7 h-7 flex items-center justify-center rounded bg-red-900/90 hover:bg-red-800 text-neutral-300"
                    title={locale.t('bottom_panel.delete')}
                    onclick={(e) => { e.stopPropagation(); gallery.deleteImage(image); }}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
                  </button>
                </div>
              </div>
            </div>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    {:else if activeTab === "prompts"}
      <!-- Prompt History & Favorites -->
      {#if sortedPromptHistory.length === 0}
        <div class="flex items-center justify-center h-full text-neutral-500 text-xs">
          <p>{locale.t('bottom_panel.no_prompts')}</p>
        </div>
      {:else}
        <div class="flex flex-col h-full">
          <div class="px-2 pt-1.5 pb-1 shrink-0">
            <input
              type="text"
              bind:value={promptSearch}
              placeholder={locale.t('bottom_panel.prompt_search_placeholder')}
              class="w-full bg-neutral-800 border border-neutral-700 rounded px-2.5 py-1 text-xs text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
            />
          </div>
          {#if filteredPromptHistory.length === 0}
            <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs">
              <p>{locale.t('bottom_panel.no_prompt_results')}</p>
            </div>
          {:else}
            <div class="flex flex-col gap-2 flex-1 min-h-0 overflow-y-auto px-2 py-2">
              {#each filteredPromptHistory as entry}
                <div class="shrink-0 rounded-lg border bg-neutral-900/60 overflow-hidden {entry.favorite ? 'border-amber-500/40' : 'border-neutral-800 hover:border-neutral-700'} transition-colors">
              <button
                class="w-full text-left p-2.5"
                onclick={() => generation.applyPromptHistoryEntry(entry.id)}
                title={locale.t('bottom_panel.load_prompt')}
              >
                <p class="text-[11px] text-neutral-200 leading-relaxed line-clamp-4">{entry.positivePrompt || locale.t('bottom_panel.empty_prompt')}</p>
                {#if entry.negativePrompt}
                  <p class="text-[10px] text-neutral-500 mt-1 line-clamp-1">{locale.t('bottom_panel.neg_prefix')} {entry.negativePrompt}</p>
                {/if}
              </button>
              <div class="px-2.5 pb-2 flex items-center justify-between gap-2 shrink-0">
                <div class="flex items-center gap-1.5 text-[10px] text-neutral-500">
                  <span>{historyLabel(entry.createdAt)}</span>
                  <span class="px-1 py-0.5 rounded bg-neutral-800 text-neutral-400">{entry.mode}</span>
                </div>
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
                    title={locale.t('bottom_panel.remove')}
                  >
                    ×
                  </button>
                </div>
              </div>
            </div>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    {:else if activeTab === "compare"}
      <CompareGrid />
    {:else if activeTab === "artists"}
      {#if artistFavourites.count === 0}
        <div class="flex items-center justify-center h-full text-neutral-500 text-xs px-4 text-center">
          <p>{locale.t('bottom_panel.no_favourite_artists')}</p>
        </div>
      {:else}
        <div class="flex flex-col h-full">
          <div class="px-2 pt-1.5 pb-1 shrink-0 flex items-center gap-2">
            <input
              type="text"
              name="artist-favourite-search"
              bind:value={artistSearch}
              placeholder={locale.t('bottom_panel.artist_search_placeholder')}
              class="flex-1 bg-neutral-800 border border-neutral-700 rounded px-2.5 py-1 text-xs text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
            />
            <div use:scrollCapture>
              <input
                type="range"
                min="72"
                max="200"
                bind:value={artistCardSize}
                class="w-16 h-4 accent-indigo-500 cursor-pointer"
                title={locale.t('bottom_panel.card_size')}
              />
            </div>
          </div>
          <!-- Category filter chips -->
          {#if artistFavourites.categories.length > 0}
            {@const counts = artistFavourites.countsByCategory}
            <div class="mx-2 mb-1 flex flex-wrap items-center gap-1 rounded-md border border-neutral-800 bg-neutral-900/50 p-1 shrink-0">
              <button
                type="button"
                class="rounded px-2 py-0.5 text-[10px] transition-colors {artistCategoryFilter === 'all' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
                onclick={() => artistCategoryFilter = 'all'}
              >All ({artistFavourites.count})</button>
              <button
                type="button"
                class="rounded px-2 py-0.5 text-[10px] transition-colors {artistCategoryFilter === '__uncat' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
                onclick={() => artistCategoryFilter = '__uncat'}
              >Uncategorised ({counts[''] ?? 0})</button>
              {#each artistFavourites.categories as cat (cat.id)}
                <button
                  type="button"
                  class="flex items-center gap-1 rounded px-2 py-0.5 text-[10px] transition-colors {artistCategoryFilter === cat.id ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
                  onclick={() => artistCategoryFilter = cat.id}
                  title={cat.name}
                >
                  <span class="h-2 w-2 rounded-full border border-neutral-700" style="background-color: {cat.color}" aria-hidden="true"></span>
                  <span class="max-w-24 truncate">{cat.name}</span>
                  <span class="text-neutral-500">({counts[cat.id] ?? 0})</span>
                </button>
              {/each}
            </div>
          {/if}
          {#if filteredFavouriteArtists.length === 0}
            <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs">
              <p>{locale.t('bottom_panel.no_artist_results')}</p>
            </div>
          {:else}
            <div
              class="grid gap-2 flex-1 min-h-0 overflow-y-auto px-2 py-2"
              style="grid-template-columns: repeat(auto-fill, minmax({artistCardSize}px, 1fr)); align-content: start;"
            >
              {#each filteredFavouriteArtists as hit (hit.slug)}
                {@const thumb = artistThumbUrl(hit)}
                {@const favCat = artistFavourites.categoryOf(hit.slug)}
                <div
                  role="button"
                  tabindex="0"
                  class="group relative flex flex-col rounded-lg border border-neutral-800 bg-neutral-900 cursor-pointer transition-colors hover:border-indigo-500 focus:outline-none focus:ring-2 focus:ring-indigo-500"
                  onclick={() => applyArtistTag(hit)}
                  onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); applyArtistTag(hit); } }}
                  title={locale.t('bottom_panel.apply_artist_tag', { tag: hit.tag })}
                >
                  <div class="relative aspect-3/4 w-full overflow-hidden rounded-t-lg bg-neutral-800">
                    {#if thumb}
                      <img src={thumb} alt={hit.tag} loading="lazy" decoding="async" class="h-full w-full object-cover" />
                    {:else}
                      <div class="flex h-full w-full items-center justify-center text-[10px] text-neutral-500">no preview</div>
                    {/if}
                    <button
                      type="button"
                      class="absolute right-1 top-1 flex h-6 w-6 items-center justify-center rounded-full border border-neutral-700 bg-neutral-900/90 text-sm leading-none text-red-400 transition-colors hover:bg-neutral-800"
                      onclick={(e) => { e.stopPropagation(); artistFavourites.toggle(hit.slug); }}
                      aria-label={locale.t('bottom_panel.unfavorite')}
                      title={locale.t('bottom_panel.unfavorite')}
                    >♥</button>
                    {#if favCat}
                      <span
                        class="absolute left-1 top-1 h-3 w-3 rounded-full border border-black/40"
                        style="background-color: {favCat.color}"
                        title={favCat.name}
                        aria-label={`Category: ${favCat.name}`}
                      ></span>
                    {/if}
                  </div>
                  <div class="px-2 py-1.5">
                    <div class="truncate text-xs text-red-400">{displayArtistTag(hit.tag)}</div>
                    {#if hit.postCount > 0}
                      <div class="text-[10px] text-neutral-500">{hit.postCount.toLocaleString()} posts</div>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    {/if}
  </div>
</div>
