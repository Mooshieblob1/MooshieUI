<script lang="ts">
  import { onMount } from "svelte";
  import { createArtistGalleryStore } from "../store.svelte.js";
  import type { ArtistEntry } from "../types.js";
  import ArtistCard from "./ArtistCard.svelte";
  import ArtistLightbox from "./ArtistLightbox.svelte";

  interface Props {
    manifestUrl: string;
    /** Optional integrator hook for "Insert tag into prompt" in the lightbox. */
    oninsertTag?: (tag: string) => void;
  }

  let { manifestUrl, oninsertTag }: Props = $props();

  const store = createArtistGalleryStore(manifestUrl);

  let selectedBucket = $state<string | null>(null);
  let bucketEntries = $state<ArtistEntry[]>([]);
  let bucketLoading = $state(false);
  let bucketError = $state<string | null>(null);
  let visibleCount = $state(60);
  const PAGE_SIZE = 60;

  let active = $state<ArtistEntry | null>(null);
  let queryInput = $state("");
  let searchDebounce: number | null = null;

  onMount(() => {
    store.init().then(() => {
      if (!selectedBucket && store.manifest && store.manifest.shards.length > 0) {
        void selectBucket(store.manifest.shards[0].bucket);
      }
    });
  });

  async function selectBucket(bucket: string) {
    selectedBucket = bucket;
    visibleCount = PAGE_SIZE;
    bucketLoading = true;
    bucketError = null;
    try {
      const shard = await store.client.loadShard(bucket);
      bucketEntries = Object.values(shard.entries).sort(
        (a, b) => b.postCount - a.postCount || a.slug.localeCompare(b.slug),
      );
    } catch (err) {
      bucketError = err instanceof Error ? err.message : String(err);
      bucketEntries = [];
    } finally {
      bucketLoading = false;
    }
  }

  function onSearchInput(value: string) {
    queryInput = value;
    if (searchDebounce !== null) window.clearTimeout(searchDebounce);
    searchDebounce = window.setTimeout(() => {
      void store.setQuery(value);
      searchDebounce = null;
    }, 120);
  }

  async function openHit(slug: string) {
    await store.openArtist(slug);
    active = store.activeArtist;
  }

  function closeLightbox() {
    active = null;
    store.closeArtist();
  }

  function loadMore() {
    visibleCount = Math.min(bucketEntries.length, visibleCount + PAGE_SIZE);
  }

  const visibleEntries = $derived(
    store.query.trim()
      ? []
      : bucketEntries.slice(0, visibleCount),
  );
</script>

<div class="flex h-full w-full flex-col overflow-hidden bg-neutral-950 text-neutral-100">
  <header class="flex-none border-b border-neutral-800 bg-neutral-900/60 px-4 py-3">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h1 class="text-lg font-semibold">Artist Gallery</h1>
        <p class="text-xs text-neutral-500">
          {#if store.manifest}
            {store.manifest.artistsWithImage.toLocaleString()} artists ·
            Anima preview · release {store.manifest.releasePrefix}
          {:else if store.manifestError}
            <span class="text-red-400">failed to load: {store.manifestError}</span>
          {:else}
            loading manifest…
          {/if}
        </p>
      </div>
      <div class="relative w-full max-w-sm">
        <input
          type="search"
          placeholder="Search artist tag…"
          value={queryInput}
          oninput={(e) => onSearchInput(e.currentTarget.value)}
          class="w-full rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
        />
        {#if store.searchLoading}
          <span class="absolute right-2 top-1/2 -translate-y-1/2 text-xs text-neutral-500">…</span>
        {/if}
      </div>
    </div>

    {#if store.manifest && !store.query.trim()}
      <div class="mt-3 flex flex-wrap gap-1">
        {#each store.manifest.shards as shard}
          <button
            type="button"
            class="rounded-md px-2 py-1 text-xs font-mono transition-colors {selectedBucket ===
            shard.bucket
              ? 'bg-indigo-600 text-white'
              : 'bg-neutral-800 text-neutral-300 hover:bg-neutral-700'}"
            onclick={() => selectBucket(shard.bucket)}
            title={`${shard.count} artists`}
          >
            {shard.bucket}
          </button>
        {/each}
      </div>
    {/if}
  </header>

  <div class="flex-1 overflow-y-auto">
    {#if store.query.trim()}
      {#if store.results.length === 0 && !store.searchLoading}
        <div class="p-8 text-center text-sm text-neutral-500">
          No artists match "{store.query}".
        </div>
      {:else}
        <div class="grid grid-cols-[repeat(auto-fill,minmax(140px,1fr))] gap-3 p-4">
          {#each store.results as hit (hit.slug)}
            {@const thumbUrl =
              store.manifest && hit.hasImage
                ? `${store.manifest.imageBaseUrl}/${store.manifest.releasePrefix}/images/${hit.imageId}.webp`
                : ""}
            <button
              type="button"
              class="group flex flex-col items-stretch overflow-hidden rounded-lg border border-neutral-800 bg-neutral-900 text-left transition-colors hover:border-indigo-500 focus:outline-none focus:ring-2 focus:ring-indigo-500"
              onclick={() => openHit(hit.slug)}
              title={hit.tag}
            >
              <div class="relative aspect-3/4 w-full bg-neutral-800">
                {#if thumbUrl}
                  <img
                    src={thumbUrl}
                    alt={hit.tag}
                    loading="lazy"
                    decoding="async"
                    class="h-full w-full object-cover"
                  />
                {:else}
                  <div class="flex h-full w-full items-center justify-center text-xs text-neutral-500">
                    no preview
                  </div>
                {/if}
              </div>
              <div class="flex items-center justify-between gap-2 px-2 py-1.5">
                <span class="truncate text-sm text-red-400">
                  {hit.tag.replace(/^@/, "").replace(/_/g, " ")}
                </span>
                <span class="shrink-0 text-xs text-neutral-500">
                  {hit.postCount >= 1000 ? `${Math.round(hit.postCount / 1000)}k` : hit.postCount}
                </span>
              </div>
            </button>
          {/each}
        </div>
      {/if}
    {:else if bucketError}
      <div class="p-8 text-center text-sm text-red-400">
        Failed to load shard: {bucketError}
      </div>
    {:else if bucketLoading && bucketEntries.length === 0}
      <div class="p-8 text-center text-sm text-neutral-500">loading…</div>
    {:else}
      <div class="grid grid-cols-[repeat(auto-fill,minmax(140px,1fr))] gap-3 p-4">
        {#each visibleEntries as entry (entry.slug)}
          <ArtistCard {entry} onclick={(e) => (active = e)} />
        {/each}
      </div>
      {#if visibleCount < bucketEntries.length}
        <div class="p-4 text-center">
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-4 py-2 text-sm text-neutral-200 transition-colors hover:border-indigo-500"
            onclick={loadMore}
          >
            Load more ({bucketEntries.length - visibleCount} remaining)
          </button>
        </div>
      {/if}
    {/if}
  </div>
</div>

{#if active}
  <ArtistLightbox entry={active} onclose={closeLightbox} {oninsertTag} />
{/if}
