<script lang="ts">
  import { untrack } from "svelte";
  import { gallery, isVideoImage } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { lazyThumbnail } from "../../utils/lazyThumbnail.js";
  import type { OutputImage } from "../../types/index.js";

  interface Props {
    open: boolean;
    title: string;
    /** Multi mode ticks several images and confirms; single mode picks and closes. */
    multiple?: boolean;
    /** Multi mode only: hard cap on the selection, so it cannot exceed free slots. */
    max?: number;
    onselect: (images: OutputImage[]) => void;
    onclose: () => void;
  }

  let { open, title, multiple = false, max = 1, onselect, onclose }: Props = $props();

  let tab = $state<"session" | "gallery">("session");
  let search = $state("");
  let picked = $state<OutputImage[]>([]);
  let renderLimit = $state(60);

  // Videos cannot be a frame or a reference, so they never reach the grid.
  const sessionStills = $derived(gallery.sessionImages.filter((image) => !isVideoImage(image)));
  const galleryStills = $derived(gallery.images.filter((image) => !isVideoImage(image)));

  const filtered = $derived.by(() => {
    const source = tab === "session" ? sessionStills : galleryStills;
    const query = search.toLowerCase().trim();
    if (!query) return source;
    return source.filter((image) => image.filename.toLowerCase().includes(query));
  });

  // Only mount a bounded window of grid cells at a time — a full gallery can
  // be thousands of images, and rendering every <img> at once on open is what
  // caused the lag spike.
  const visible = $derived(filtered.slice(0, renderLimit));

  function loadMore(node: HTMLElement) {
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) renderLimit += 60;
      },
      { rootMargin: "200px" },
    );
    observer.observe(node);
    return { destroy() { observer.disconnect(); } };
  }

  // Reset on each opening only. The body is untracked so a gallery refresh
  // mid-selection cannot wipe what the user has already ticked.
  $effect(() => {
    if (!open) return;
    untrack(() => {
      tab = gallery.sessionImages.some((image) => !isVideoImage(image)) ? "session" : "gallery";
      search = "";
      picked = [];
      renderLimit = 60;
    });
  });

  $effect(() => {
    void tab;
    void search;
    renderLimit = 60;
  });

  function isPicked(image: OutputImage): boolean {
    return picked.includes(image);
  }

  function toggle(image: OutputImage) {
    if (!multiple) {
      onselect([image]);
      onclose();
      return;
    }
    if (isPicked(image)) {
      picked = picked.filter((entry) => entry !== image);
    } else if (picked.length < max) {
      picked = [...picked, image];
    }
  }

  function confirm() {
    if (picked.length === 0) return;
    onselect(picked);
    onclose();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!open || event.key !== "Escape") return;
    event.stopPropagation();
    onclose();
  }

  /** Teleport element to document.body so it escapes overflow/transform containers (mobile panel slide). */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      },
    };
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    use:portal
    class="fixed inset-0 z-[10000] flex items-center justify-center bg-black/70 backdrop-blur-sm p-4"
    onclick={onclose}
  >
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="w-full max-w-3xl max-h-[80vh] flex flex-col rounded-xl border border-neutral-700 bg-neutral-900 shadow-2xl overflow-hidden"
      onclick={(e) => e.stopPropagation()}
    >
      <div class="flex items-center gap-2 px-4 py-3 border-b border-neutral-800">
        <h2 class="flex-1 truncate text-sm font-medium text-neutral-100">{title}</h2>
        <button
          type="button"
          class="w-7 h-7 flex items-center justify-center rounded-md text-neutral-400 hover:text-neutral-100 hover:bg-neutral-800 transition-colors"
          title={locale.t("common.close")}
          aria-label={locale.t("common.close")}
          onclick={onclose}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>

      <div class="flex items-center gap-2 px-4 py-2 border-b border-neutral-800">
        <button
          type="button"
          class="px-2.5 py-1 text-xs rounded-md transition-colors {tab === 'session'
            ? 'bg-indigo-600 text-white'
            : 'text-neutral-400 hover:text-neutral-100 hover:bg-neutral-800'}"
          onclick={() => (tab = "session")}
        >
          {locale.t("gallery.picker.session")} ({sessionStills.length})
        </button>
        <button
          type="button"
          class="px-2.5 py-1 text-xs rounded-md transition-colors {tab === 'gallery'
            ? 'bg-indigo-600 text-white'
            : 'text-neutral-400 hover:text-neutral-100 hover:bg-neutral-800'}"
          onclick={() => (tab = "gallery")}
        >
          {locale.t("gallery.picker.gallery")} ({galleryStills.length})
        </button>
        <input
          type="text"
          bind:value={search}
          placeholder={locale.t("gallery.picker.search")}
          class="flex-1 bg-neutral-800 border border-neutral-700 rounded px-2.5 py-1 text-xs text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
        />
      </div>

      <div class="flex-1 min-h-0 overflow-y-auto p-3">
        {#if filtered.length === 0}
          <p class="py-8 text-center text-xs text-neutral-500">
            {locale.t("gallery.picker.empty")}
          </p>
        {:else}
          <div
            class="grid gap-2"
            style="grid-template-columns: repeat(auto-fill, minmax(96px, 1fr));"
          >
            {#each visible as image (image)}
              <button
                type="button"
                class="relative aspect-square rounded-lg overflow-hidden border transition-colors {isPicked(
                  image,
                )
                  ? 'border-indigo-500'
                  : 'border-neutral-800 hover:border-neutral-600'}"
                title={image.filename}
                onclick={() => toggle(image)}
              >
                <img
                  use:lazyThumbnail={{ image }}
                  alt={image.filename}
                  class="w-full h-full object-cover"
                />
                {#if isPicked(image)}
                  <span
                    class="absolute top-1 right-1 w-5 h-5 rounded-full bg-indigo-600 text-white flex items-center justify-center"
                  >
                    <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="3" d="M5 13l4 4L19 7"/></svg>
                  </span>
                {/if}
              </button>
            {/each}
          </div>
          {#if renderLimit < filtered.length}
            <div use:loadMore class="h-4 w-full"></div>
          {/if}
        {/if}
      </div>

      {#if multiple}
        <div class="flex items-center gap-2 px-4 py-3 border-t border-neutral-800">
          <span class="mr-auto text-[11px] text-neutral-500">{picked.length} / {max}</span>
          <button
            type="button"
            class="px-3 py-1.5 text-xs rounded-lg border border-neutral-700 text-neutral-300 hover:text-neutral-100 hover:border-neutral-500 transition-colors"
            onclick={onclose}
          >
            {locale.t("common.cancel")}
          </button>
          <button
            type="button"
            disabled={picked.length === 0}
            class="px-3 py-1.5 text-xs rounded-lg bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 disabled:hover:bg-indigo-600 text-white transition-colors"
            onclick={confirm}
          >
            {locale.t("gallery.picker.add", { count: picked.length })}
          </button>
        </div>
      {/if}
    </div>
  </div>
{/if}
