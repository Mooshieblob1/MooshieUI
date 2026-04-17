<script lang="ts">
  import type { ArtistEntry } from "../types.js";

  interface Props {
    entry: ArtistEntry;
    onclose: () => void;
    /** Optional integrator hook: "Insert tag into prompt" action. */
    oninsertTag?: (tag: string) => void;
    onprev?: () => void;
    onnext?: () => void;
    /** Zoom state is owned by the parent so it survives modal close/reopen. */
    zoomed: boolean;
    ontogglezoom: () => void;
  }

  let { entry, onclose, oninsertTag, onprev, onnext, zoomed, ontogglezoom }: Props = $props();

  function toggleZoom() { ontogglezoom(); }

  function displayTag(tag: string): string {
    return tag.replace(/^@/, "").replace(/_/g, " ");
  }

  function onBackdropKey(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
    if (e.key === "ArrowLeft") { e.preventDefault(); onprev?.(); }
    if (e.key === "ArrowRight") { e.preventDefault(); onnext?.(); }
  }

  async function copyTag() {
    try {
      const formatted = "@" + entry.tag.replace(/^@/, "");
      await navigator.clipboard.writeText(formatted);
    } catch {
      // no-op; clipboard may be unavailable in some webviews
    }
  }
</script>

<svelte:window onkeydown={onBackdropKey} />

<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm"
  role="dialog"
  aria-modal="true"
  aria-label={`Artist preview: ${entry.tag}`}
>
  <button
    type="button"
    class="absolute inset-0 h-full w-full cursor-default"
    aria-label="Close"
    onclick={onclose}
  ></button>

  <div class="relative z-10 flex max-h-[92vh] overflow-y-auto flex-col items-center gap-3 p-4">
    <!-- Prev / Next arrow overlays -->
    {#if onprev}
      <button
        type="button"
        onclick={onprev}
        class="absolute -left-12 top-1/2 -translate-y-1/2 flex h-10 w-10 items-center justify-center rounded-full border border-neutral-700 bg-neutral-900/90 text-neutral-200 transition-colors hover:border-indigo-500 hover:text-white focus:outline-none"
        aria-label="Previous artist"
      >
        ←
      </button>
    {/if}
    {#if onnext}
      <button
        type="button"
        onclick={onnext}
        class="absolute -right-12 top-1/2 -translate-y-1/2 flex h-10 w-10 items-center justify-center rounded-full border border-neutral-700 bg-neutral-900/90 text-neutral-200 transition-colors hover:border-indigo-500 hover:text-white focus:outline-none"
        aria-label="Next artist"
      >
        →
      </button>
    {/if}
    {#if entry.hasImage && entry.imageUrl}
      <img
        src={entry.imageUrl}
        alt={entry.tag}
        class="max-h-[80vh] max-w-[92vw] w-auto rounded-lg border border-neutral-800 object-contain shadow-2xl"
        style="zoom: {zoomed ? 1.5 : 1}; transition: zoom 0.4s cubic-bezier(0.34, 1.56, 0.64, 1); cursor: {zoomed ? 'zoom-out' : 'zoom-in'};"
        onclick={toggleZoom}
      />
    {:else}
      <div
        class="flex aspect-3/4 w-[60vh] max-w-[92vw] items-center justify-center rounded-lg border border-neutral-800 bg-neutral-900 text-sm text-neutral-500"
      >
        no preview available
      </div>
    {/if}

    <div
      class="w-full max-w-[92vw] flex flex-wrap items-center justify-between gap-3 rounded-lg border border-neutral-800 bg-neutral-900/90 px-4 py-3"
    >
      <div class="min-w-0 flex-1">
        <a
          href="https://danbooru.donmai.us/artists/show_or_new?name={encodeURIComponent(entry.tag.replace(/^@/, ''))}"
          target="_blank"
          rel="noopener noreferrer"
          class="truncate text-base font-medium text-red-400 hover:underline"
        >
          {displayTag(entry.tag)}
        </a>
        <div class="mt-0.5 text-xs text-neutral-500">
          {entry.postCount.toLocaleString()} posts
          {#if entry.aliases.length > 0}
            · aliases: {entry.aliases.map((a) => a.replace(/^@/, "")).join(", ")}
          {/if}
        </div>
      </div>
      <div class="flex shrink-0 gap-2">
        <button
          type="button"
          class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 transition-colors hover:border-indigo-500 hover:bg-neutral-700"
          onclick={copyTag}
        >
          Copy tag
        </button>
        {#if oninsertTag}
          <button
            type="button"
            class="rounded-md bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-indigo-500"
            onclick={() => oninsertTag?.("@" + entry.tag.replace(/^@/, ""))}
          >
            Insert into prompt
          </button>
        {/if}
        <button
          type="button"
          class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 transition-colors hover:border-neutral-500"
          onclick={onclose}
        >
          Close
        </button>
      </div>
    </div>
  </div>
</div>
