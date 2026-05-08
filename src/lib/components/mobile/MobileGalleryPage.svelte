<script lang="ts">
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { lazyThumbnail } from "../../utils/lazyThumbnail.js";
  import MobileTopBar from "./MobileTopBar.svelte";
  import MobileActionSheet from "./MobileActionSheet.svelte";
  import MobileLightbox from "./MobileLightbox.svelte";
  import MobileBottomSheet from "./MobileBottomSheet.svelte";
  import type { OutputImage } from "../../types/index.js";
  import {
    sendImageToImg2Img,
    sendImageToUpscale,
  } from "../../utils/galleryActions.js";
  import type { ActionSheetItem } from "./MobileActionSheet.svelte";

  interface Props {
    onSwitchToGenerate?: () => void;
  }
  let { onSwitchToGenerate }: Props = $props();

  // Filters
  let boardFilter = $state<string>("all");
  let sortBy = $state<"date" | "name" | "size">("date");
  let sortDir = $state<"asc" | "desc">("desc");
  let filtersOpen = $state(false);

  // Action sheet for long-press
  let actionImage = $state<OutputImage | null>(null);
  let actionOpen = $state(false);

  // Long-press tracking
  let pressTimer: ReturnType<typeof setTimeout> | null = null;
  let pressedImage: OutputImage | null = null;
  let longPressFired = false;

  function tt(key: string, fb: string) {
    const v = locale.t(key);
    return v === key ? fb : v;
  }

  const filteredSorted = $derived.by(() => {
    let imgs = gallery.images.slice();
    if (boardFilter !== "all") {
      imgs = imgs.filter((i) => gallery.getBoard(i) === boardFilter);
    }
    imgs.sort((a, b) => {
      let cmp = 0;
      if (sortBy === "date") {
        cmp = (a.generated_at_ms ?? 0) - (b.generated_at_ms ?? 0);
      } else if (sortBy === "name") {
        cmp = a.filename.localeCompare(b.filename);
      } else if (sortBy === "size") {
        cmp = (a.file_size_bytes ?? 0) - (b.file_size_bytes ?? 0);
      }
      return sortDir === "asc" ? cmp : -cmp;
    });
    return imgs;
  });

  function startPress(e: PointerEvent, image: OutputImage) {
    pressedImage = image;
    longPressFired = false;
    if (pressTimer) clearTimeout(pressTimer);
    pressTimer = setTimeout(() => {
      longPressFired = true;
      // Haptic if available
      try {
        navigator.vibrate?.(8);
      } catch {}
      actionImage = image;
      actionOpen = true;
    }, 450);
  }

  function endPress() {
    if (pressTimer) {
      clearTimeout(pressTimer);
      pressTimer = null;
    }
  }

  function tapImage(image: OutputImage) {
    if (longPressFired) return;
    gallery.openLightbox(image);
  }

  const actionItems = $derived<ActionSheetItem[]>(
    actionImage
      ? [
          {
            label: tt("gallery.send_to_generate", "Send to Generate"),
            onSelect: async () => {
              if (!actionImage) return;
              try {
                await sendImageToImg2Img(actionImage);
                onSwitchToGenerate?.();
              } catch (e) {
                console.error(e);
              }
            },
          },
          {
            label: tt("gallery.use_for_upscale", "Use for Upscale"),
            onSelect: async () => {
              if (!actionImage) return;
              try {
                await sendImageToUpscale(actionImage);
                onSwitchToGenerate?.();
              } catch (e) {
                console.error(e);
              }
            },
          },
          {
            label: tt("gallery.copy_image", "Copy image"),
            onSelect: () => actionImage && gallery.copyToClipboard(actionImage),
          },
          {
            label: tt("gallery.save_as", "Download"),
            onSelect: () => actionImage && gallery.saveImageAs(actionImage),
          },
          {
            label: tt("gallery.delete", "Delete"),
            destructive: true,
            onSelect: () => actionImage && gallery.deleteImage(actionImage),
          },
        ]
      : [],
  );

  const boardOptions = $derived(["all", "Unsorted", ...gallery.boards]);
</script>

<div class="h-full flex flex-col bg-neutral-950">
  <MobileTopBar title={tt("nav.gallery", "Gallery")}>
    {#snippet rightAction()}
      <button
        type="button"
        class="touch-target px-3 text-xs text-neutral-300 hover:text-neutral-100 flex items-center gap-1"
        onclick={() => (filtersOpen = true)}
        aria-label="Filters"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none"
          stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3"/>
        </svg>
        <span>{tt("gallery.filters", "Filters")}</span>
      </button>
    {/snippet}
  </MobileTopBar>

  <div class="flex-1 min-h-0 overflow-y-auto no-scroll-chain p-1">
    {#if gallery.loading}
      <div class="flex items-center justify-center py-12 text-neutral-500 text-sm">
        {tt("gallery.loading", "Loading…")}
      </div>
    {:else if filteredSorted.length === 0}
      <div class="flex items-center justify-center py-12 text-neutral-500 text-sm text-center px-6">
        {tt("gallery.empty_generate", "No images yet — generate something!")}
      </div>
    {:else}
      <div class="grid grid-cols-2 gap-1">
        {#each filteredSorted as image (image.filename + image.subfolder)}
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <div
            role="button"
            tabindex="0"
            class="relative aspect-square overflow-hidden rounded-lg bg-neutral-900 border border-neutral-800 active:scale-[0.98] transition-transform tap-highlight-none"
            onpointerdown={(e) => startPress(e, image)}
            onpointerup={endPress}
            onpointercancel={endPress}
            onpointerleave={endPress}
            onclick={() => tapImage(image)}
            oncontextmenu={(e) => {
              e.preventDefault();
              actionImage = image;
              actionOpen = true;
            }}
          >
            <img
              use:lazyThumbnail={{ image, size: 256 }}
              alt={image.filename}
              class="w-full h-full object-cover"
              draggable="false"
            />
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<!-- Filters sheet -->
<MobileBottomSheet
  open={filtersOpen}
  snap="half"
  snaps={["half"]}
  title={tt("gallery.filters", "Filters")}
  onClose={() => (filtersOpen = false)}
>
  <div class="space-y-4">
    <label class="block">
      <span class="text-xs text-neutral-400 mb-1 block">{tt("gallery.board_filter", "Board")}</span>
      <select
        bind:value={boardFilter}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-3 text-sm text-neutral-100"
      >
        {#each boardOptions as opt}
          <option value={opt}>{opt === "all" ? tt("gallery.all_boards", "All boards") : opt}</option>
        {/each}
      </select>
    </label>
    <label class="block">
      <span class="text-xs text-neutral-400 mb-1 block">{tt("gallery.sort_by", "Sort by")}</span>
      <select
        bind:value={sortBy}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-3 text-sm text-neutral-100"
      >
        <option value="date">{tt("gallery.sort_date", "Date")}</option>
        <option value="name">{tt("gallery.sort_name", "Name")}</option>
        <option value="size">{tt("gallery.sort_size", "Size")}</option>
      </select>
    </label>
    <div class="flex gap-2">
      <button
        type="button"
        class="touch-target flex-1 px-3 py-2.5 rounded-lg border text-sm transition-colors {sortDir === 'desc'
          ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300'
          : 'border-neutral-700 text-neutral-300'}"
        onclick={() => (sortDir = "desc")}
      >
        {tt("gallery.descending", "Descending")}
      </button>
      <button
        type="button"
        class="touch-target flex-1 px-3 py-2.5 rounded-lg border text-sm transition-colors {sortDir === 'asc'
          ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300'
          : 'border-neutral-700 text-neutral-300'}"
        onclick={() => (sortDir = "asc")}
      >
        {tt("gallery.ascending", "Ascending")}
      </button>
    </div>
  </div>
</MobileBottomSheet>

<MobileActionSheet
  open={actionOpen}
  title={actionImage?.filename ?? ""}
  items={actionItems}
  onClose={() => (actionOpen = false)}
/>

<MobileLightbox images={filteredSorted} {onSwitchToGenerate} />
