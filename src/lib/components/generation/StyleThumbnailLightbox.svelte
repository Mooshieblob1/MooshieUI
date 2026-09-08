<script lang="ts">
  /**
   * Full-size view of a style's thumbnail. Mounted at the app root rather than
   * in the Styles tab so the overlay covers the whole window, not just the
   * bottom panel the list lives in.
   */
  import { styles } from "../../stores/styles.svelte.js";
  import { styleEditors } from "../../stores/styleEditors.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";

  const style = $derived(
    styleEditors.thumbnailStyleId
      ? (styles.styles.find((s) => s.id === styleEditors.thumbnailStyleId) ?? null)
      : null,
  );
  // A style can lose its thumbnail while the lightbox is up (cleared from the
  // editor), which leaves nothing to show.
  const open = $derived(style !== null && !!style.thumbnail);

  /** Full-resolution image, once the gallery has decoded it. */
  let fullUrl = $state<string | null>(null);
  let fullLoading = $state(false);

  // The stored thumbnail is a 384 px JPEG: sharp at tile size, mush at full
  // screen. Styles that remember which gallery entry they came from get the
  // real picture loaded here (loadFullImage transcodes JXL to WebP on the
  // way), with the small one on screen until it arrives.
  $effect(() => {
    const filename = open ? (style?.thumbnailImage ?? null) : null;
    if (!filename) {
      fullLoading = false;
      return;
    }
    let cancelled = false;
    let created: string | null = null;
    fullLoading = true;
    void gallery
      .loadFullImage(filename)
      .then((url) => {
        if (cancelled) {
          URL.revokeObjectURL(url);
          return;
        }
        created = url;
        fullUrl = url;
      })
      // The gallery entry can be gone (deleted, or a different install
      // importing the style). The small thumbnail stays on screen.
      .catch((e) => console.error("Failed to load full style thumbnail:", e))
      .finally(() => {
        if (!cancelled) fullLoading = false;
      });
    return () => {
      cancelled = true;
      fullUrl = null;
      fullLoading = false;
      if (created) URL.revokeObjectURL(created);
    };
  });

  function onKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      styleEditors.closeThumbnail();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open && style}
  <div class="fixed inset-0 z-[70] flex flex-col bg-black/95">
    <div class="flex shrink-0 items-center justify-between gap-3 px-4 py-3">
      <p class="truncate text-sm text-neutral-100">{style.name}</p>
      <button
        type="button"
        class="shrink-0 rounded border border-neutral-700 bg-neutral-900/80 px-2 py-1 text-xs text-neutral-300 hover:border-indigo-500 hover:text-indigo-200"
        aria-label={locale.t("common.close")}
        title={locale.t("common.close")}
        onclick={() => styleEditors.closeThumbnail()}>x</button
      >
    </div>
    <div class="relative min-h-0 flex-1 p-4">
      <img
        src={fullUrl ?? style.thumbnail}
        alt={locale.t("styles.editor.thumbnail_alt")}
        class="h-full w-full object-contain"
      />
      {#if fullLoading}
        <div
          class="absolute bottom-6 right-6 h-5 w-5 animate-spin rounded-full border-2 border-indigo-500 border-t-transparent"
        ></div>
      {/if}
    </div>
  </div>
{/if}
