<script lang="ts">
  import type { ArtistEntry } from "../types.js";

  interface Props {
    entry: ArtistEntry;
    onclick?: (entry: ArtistEntry) => void;
  }

  let { entry, onclick }: Props = $props();

  function formatCount(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(0)}k`;
    return String(n);
  }

  function displayTag(tag: string): string {
    return tag.replace(/^@/, "").replace(/_/g, " ");
  }
</script>

<button
  type="button"
  class="group relative flex flex-col items-stretch overflow-hidden rounded-lg border border-neutral-800 bg-neutral-900 text-left transition-colors hover:border-indigo-500 focus:outline-none focus:ring-2 focus:ring-indigo-500"
  onclick={() => onclick?.(entry)}
  title={entry.tag}
>
  <div class="relative aspect-3/4 w-full bg-neutral-800">
    {#if entry.hasImage && entry.imageUrl}
      <img
        src={entry.imageUrl}
        alt={entry.tag}
        loading="lazy"
        decoding="async"
        class="h-full w-full object-cover transition-opacity duration-200"
      />
    {:else}
      <div class="flex h-full w-full items-center justify-center text-xs text-neutral-500">
        no preview
      </div>
    {/if}
  </div>
  <div class="flex items-center justify-between gap-2 px-2 py-1.5">
    <span class="truncate text-sm text-red-400">{displayTag(entry.tag)}</span>
    <span class="shrink-0 text-xs text-neutral-500">{formatCount(entry.postCount)}</span>
  </div>
</button>
