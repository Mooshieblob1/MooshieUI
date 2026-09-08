<script lang="ts">
  /**
   * Full-size view of a style's thumbnail. Mounted at the app root rather than
   * in the Styles tab so the overlay covers the whole window, not just the
   * bottom panel the list lives in.
   */
  import { styles } from "../../stores/styles.svelte.js";
  import { styleEditors } from "../../stores/styleEditors.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";

  const style = $derived(
    styleEditors.thumbnailStyleId
      ? (styles.styles.find((s) => s.id === styleEditors.thumbnailStyleId) ?? null)
      : null,
  );
  // A style can lose its thumbnail while the lightbox is up (cleared from the
  // editor), which leaves nothing to show.
  const open = $derived(style !== null && !!style.thumbnail);

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
    <div class="min-h-0 flex-1 p-4">
      <img
        src={style.thumbnail}
        alt={locale.t("styles.editor.thumbnail_alt")}
        class="h-full w-full object-contain"
      />
    </div>
  </div>
{/if}
