<script lang="ts">
  import type { ArtistEntry, ArtistImage } from "../types.js";
  import { formatPostCount } from "../counts.js";
  import { cachedSrc } from "../imageCache.js";
  import { locale } from "../../stores/locale.svelte.js";

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
    /** Currently displayed variant (1-based). Index v2+; defaults to the primary. */
    variant?: number;
    /** Show the in-modal flip button (only when the artist has >= 2 variants). */
    canFlip?: boolean;
    /** Flip the active artist's variant. Routed through the same override path as the grid. */
    onflip?: () => void;
  }

  let { entry, onclose, oninsertTag, onprev, onnext, zoomed, ontogglezoom, variant = 1, canFlip = false, onflip }: Props = $props();

  /** Index v2+: variants with confirmed images. Falls back to the v1 primary when absent. */
  const visibleImages = $derived(
    entry.images
      ? entry.images.filter((i: ArtistImage) => i.hasImage)
      : entry.hasImage && entry.imageUrl
        ? [{ variantId: "p1", imageId: entry.imageId, imageUrl: entry.imageUrl, objectKey: entry.objectKey, hasImage: true }]
        : []
  );

  /** Clamp the requested variant into the available range (1-based). */
  const clampedVariant = $derived(
    Math.min(Math.max(1, variant), Math.max(1, visibleImages.length)),
  );
  /** The single image shown for the current variant. */
  const currentImage = $derived(visibleImages[clampedVariant - 1] ?? visibleImages[0]);
  /** Total variants for flip math; falls back to 2 while the shard entry loads. */
  const variantTotal = $derived(Math.max(visibleImages.length, canFlip ? 2 : 1));
  /** Variant the flip button switches to (wraps around). */
  const nextVariant = $derived(variantTotal >= 2 ? (clampedVariant % variantTotal) + 1 : clampedVariant);

  /**
   * 3D rotate animation, played only when the same artist's variant changes —
   * NOT when navigating prev/next to a different artist (slug changes).
   */
  function flipImage(node: HTMLElement, params: { slug: string; variant: number }) {
    let prev = params;
    return {
      update(next: { slug: string; variant: number }) {
        if (next.slug === prev.slug && next.variant !== prev.variant) {
          node.animate(
            [
              { transform: "perspective(1000px) rotateY(0deg)" },
              { transform: "perspective(1000px) rotateY(90deg)", offset: 0.5 },
              { transform: "perspective(1000px) rotateY(0deg)" },
            ],
            { duration: 400, easing: "ease-in-out" },
          );
        }
        prev = next;
      },
    };
  }

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
  aria-label={locale.t('artist_gallery.lightbox.aria', { tag: entry.tag })}
>
  <button
    type="button"
    class="absolute inset-0 h-full w-full cursor-default"
    aria-label={locale.t('artist_gallery.lightbox.close_aria')}
    onclick={onclose}
  ></button>

  <div class="relative z-10 flex max-h-[92vh] flex-col items-center">
    <!-- Prev / Next arrow overlays. Kept on this non-scrolling positioned
         parent so their negative offsets never add to the scroll container's
         horizontal overflow (which would surface a stray horizontal scrollbar). -->
    {#if onprev}
      <button
        type="button"
        onclick={onprev}
        class="absolute -left-12 top-1/2 -translate-y-1/2 flex h-10 w-10 items-center justify-center rounded-full border border-neutral-700 bg-neutral-900/90 text-neutral-200 transition-colors hover:border-indigo-500 hover:text-white focus:outline-none"
        aria-label={locale.t('artist_gallery.lightbox.prev_aria')}
      >
        ←
      </button>
    {/if}
    {#if onnext}
      <button
        type="button"
        onclick={onnext}
        class="absolute -right-12 top-1/2 -translate-y-1/2 flex h-10 w-10 items-center justify-center rounded-full border border-neutral-700 bg-neutral-900/90 text-neutral-200 transition-colors hover:border-indigo-500 hover:text-white focus:outline-none"
        aria-label={locale.t('artist_gallery.lightbox.next_aria')}
      >
        →
      </button>
    {/if}
    <div class="flex w-full max-h-[92vh] flex-col items-center gap-3 overflow-y-auto p-4">
    {#if currentImage}
      <div class="flex items-start justify-center">
        <img
          use:cachedSrc={currentImage.imageUrl}
          use:flipImage={{ slug: entry.slug, variant: clampedVariant }}
          alt="{entry.tag} {currentImage.variantId}"
          class="max-h-[80vh] max-w-[92vw] w-auto rounded-lg border border-neutral-800 object-contain shadow-2xl"
          style="zoom: {zoomed ? 1.5 : 1}; transition: zoom 0.4s cubic-bezier(0.34, 1.56, 0.64, 1); cursor: {zoomed ? 'zoom-out' : 'zoom-in'};"
          onclick={toggleZoom}
        />
      </div>
    {:else}
      <div
        class="flex aspect-3/4 w-[60vh] max-w-[92vw] items-center justify-center rounded-lg border border-neutral-800 bg-neutral-900 text-sm text-neutral-500"
      >
        {locale.t('artist_gallery.lightbox.no_preview')}
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
          {locale.t('artist_gallery.lightbox.posts', { count: formatPostCount(entry, false) })}
          {#if entry.aliases.length > 0}
            {locale.t('artist_gallery.lightbox.aliases', { list: entry.aliases.map((a) => a.replace(/^@/, "")).join(", ") })}
          {/if}
        </div>
      </div>
      <div class="flex shrink-0 gap-2">
        {#if canFlip && onflip}
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 transition-colors hover:border-indigo-500 hover:bg-neutral-700"
            onclick={onflip}
          >
            {locale.t('artist_gallery.lightbox.flip', { n: nextVariant })}
          </button>
        {/if}
        <button
          type="button"
          class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 transition-colors hover:border-indigo-500 hover:bg-neutral-700"
          onclick={copyTag}
        >
          {locale.t('artist_gallery.lightbox.copy_tag')}
        </button>
        {#if oninsertTag}
          <button
            type="button"
            class="rounded-md bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-indigo-500"
            onclick={() => oninsertTag?.(entry.tag)}
          >
            {locale.t('artist_gallery.lightbox.insert')}
          </button>
        {/if}
        <button
          type="button"
          class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 transition-colors hover:border-neutral-500"
          onclick={onclose}
        >
          {locale.t('artist_gallery.lightbox.close')}
        </button>
      </div>
    </div>
    </div>
  </div>
</div>
