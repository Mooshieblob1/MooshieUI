<script lang="ts">
  import { createArtistGalleryStore } from "../store.svelte.js";
  import type { ArtistEntry } from "../types.js";
  import { cachedSrc } from "../imageCache.js";
  import { locale } from "../../stores/locale.svelte.js";

  interface Props {
    manifestUrl: string;
    /** Either the slug or the raw artist tag ("@dairi"). */
    slugOrTag: string;
  }

  let { manifestUrl, slugOrTag }: Props = $props();

  const store = createArtistGalleryStore(manifestUrl);

  let entry = $state<ArtistEntry | null>(null);
  let loading = $state(false);
  let failed = $state(false);

  async function load(key: string) {
    if (!key) {
      entry = null;
      return;
    }
    loading = true;
    failed = false;
    try {
      await store.init();
      entry = await store.client.getArtist(key);
      failed = !entry || !entry.hasImage;
    } catch {
      entry = null;
      failed = true;
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    load(slugOrTag);
  });
</script>

<div
  class="pointer-events-none flex h-44 w-32 items-center justify-center overflow-hidden rounded-md border border-neutral-700 bg-neutral-900 shadow-xl"
>
  {#if loading}
    <div class="h-full w-full animate-pulse bg-neutral-800"></div>
  {:else if entry && entry.imageUrl && !failed}
    <img
      use:cachedSrc={entry.imageUrl}
      src={entry.imageUrl}
      alt={entry.tag}
      loading="eager"
      decoding="async"
      class="h-full w-full object-cover"
    />
  {:else}
    <span class="px-2 text-center text-[10px] text-neutral-500">
      {locale.t('artist_gallery.no_preview')}
    </span>
  {/if}
</div>
