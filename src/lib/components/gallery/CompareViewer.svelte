<script lang="ts">
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { accessibility } from "../../stores/accessibility.svelte.js";
  import ImageCompare from "../ui/ImageCompare.svelte";
  import type { CompareMode } from "../../types/index.js";

  // Divider position in slider mode, blend amount in fade mode.
  let position = $state(50);
  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let sizes = $state({ aw: 0, ah: 0, bw: 0, bh: 0 });

  const SIDES: ("a" | "b")[] = ["a", "b"];
  const MODES: { id: CompareMode; labelKey: string }[] = [
    { id: "slider", labelKey: "gallery.compare.mode_slider" },
    { id: "fade", labelKey: "gallery.compare.mode_fade" },
    { id: "difference", labelKey: "gallery.compare.mode_difference" },
    { id: "side_by_side", labelKey: "gallery.compare.mode_side_by_side" },
  ];

  const visionSimClass = $derived(
    accessibility.visionSimulatorMode === "none"
      ? ""
      : `sim-${accessibility.visionSimulatorMode}`,
  );

  const dimsKnown = $derived(sizes.aw > 0 && sizes.bw > 0);
  const dimsMatch = $derived(dimsKnown && sizes.aw === sizes.bw && sizes.ah === sizes.bh);
  const bothLoaded = $derived(!!gallery.compareUrlA && !!gallery.compareUrlB);
  const showsDivider = $derived(
    gallery.compareMode === "slider" || gallery.compareMode === "fade",
  );
  const axisToggleable = $derived(
    gallery.compareMode === "slider" || gallery.compareMode === "side_by_side",
  );

  // Difference blending only means anything when the pixels line up, so drop
  // back to the slider as soon as a mismatched pair loads.
  $effect(() => {
    if (gallery.compareMode === "difference" && dimsKnown && !dimsMatch) {
      gallery.compareMode = "slider";
    }
  });

  $effect(() => {
    const handler = (e: KeyboardEvent) => {
      // The divider handle consumes its own arrow keys (preventDefault), and
      // range/select inputs need theirs for fine adjustment.
      if (e.defaultPrevented) return;
      const target = e.target as HTMLElement | null;
      const tag = target?.tagName;
      if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA" || target?.isContentEditable) {
        return;
      }
      if (e.key === "Escape") {
        gallery.closeCompare();
        return;
      }
      // Arrows walk side B; hold Shift to walk side A instead.
      const side = e.shiftKey ? "a" : "b";
      if (e.key === "ArrowLeft") void gallery.navigateCompare(side, "prev");
      else if (e.key === "ArrowRight") void gallery.navigateCompare(side, "next");
      else return;
      e.preventDefault();
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  });

  function resetView() {
    zoom = 1;
    panX = 0;
    panY = 0;
  }

  function focusOnMount(node: HTMLElement) {
    node.focus();
  }

  function sideLabel(side: "a" | "b") {
    const image = side === "a" ? gallery.compareA : gallery.compareB;
    const prefix = side === "a" ? locale.t("gallery.compare.side_a") : locale.t("gallery.compare.side_b");
    return image ? `${prefix} · ${image.filename}` : prefix;
  }

  function sideDims(side: "a" | "b") {
    const w = side === "a" ? sizes.aw : sizes.bw;
    const h = side === "a" ? sizes.ah : sizes.bh;
    return w > 0 && h > 0 ? `${w}×${h}` : "";
  }
</script>

<div
  class="lightbox-backdrop fixed inset-0 z-70 bg-black/95 flex flex-col {visionSimClass}"
  role="dialog"
  aria-modal="true"
  aria-label={locale.t("gallery.compare.title")}
  tabindex="-1"
  use:focusOnMount
>
  <!-- Toolbar -->
  <div
    class="flex flex-wrap items-center gap-2 px-3 py-2 border-b border-neutral-800 bg-neutral-900/70"
  >
    <span class="text-sm font-medium text-neutral-200 mr-1">{locale.t("gallery.compare.title")}</span>
    <div class="flex flex-wrap gap-1">
      {#each MODES as mode}
        <button
          class="px-2.5 py-1 text-xs rounded border transition-colors disabled:opacity-40 disabled:cursor-not-allowed {gallery.compareMode ===
          mode.id
            ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300'
            : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}"
          disabled={mode.id === "difference" && dimsKnown && !dimsMatch}
          title={mode.id === "difference" && dimsKnown && !dimsMatch
            ? locale.t("gallery.compare.difference_needs_same_size")
            : locale.t(mode.labelKey)}
          onclick={() => (gallery.compareMode = mode.id)}
        >
          {locale.t(mode.labelKey)}
        </button>
      {/each}
    </div>

    <div class="flex-1"></div>

    {#if dimsKnown && !dimsMatch}
      <span
        class="px-2 py-0.5 rounded bg-amber-900/50 border border-amber-700/60 text-[11px] text-amber-200"
        title={locale.t("gallery.compare.difference_needs_same_size")}
      >
        {locale.t("gallery.compare.dimensions_differ")}
      </span>
    {/if}

    {#if axisToggleable}
      <button
        class="px-2.5 py-1 text-xs rounded border border-neutral-700 text-neutral-300 hover:border-neutral-500 transition-colors"
        onclick={() =>
          (gallery.compareOrientation =
            gallery.compareOrientation === "horizontal" ? "vertical" : "horizontal")}
      >
        {gallery.compareOrientation === "horizontal"
          ? locale.t("gallery.compare.orientation_horizontal")
          : locale.t("gallery.compare.orientation_vertical")}
      </button>
    {/if}
    <button
      class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
      title={locale.t("gallery.compare.swap")}
      onclick={() => gallery.swapCompare()}
    >
      ⇅
    </button>
    <button
      class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
      title={locale.t("gallery.compare.reset_view")}
      onclick={resetView}
    >
      ⟲
    </button>
    <button
      class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
      title={locale.t("gallery.compare.close")}
      onclick={() => gallery.closeCompare()}
    >
      ✕
    </button>
  </div>

  <!-- Viewer -->
  <div class="relative flex-1 min-h-0 p-3">
    {#if bothLoaded}
      <ImageCompare
        urlA={gallery.compareUrlA!}
        urlB={gallery.compareUrlB!}
        labelA={sideLabel("a")}
        labelB={sideLabel("b")}
        ariaLabel={locale.t("gallery.compare.divider")}
        mode={gallery.compareMode}
        orientation={gallery.compareOrientation}
        bind:position
        bind:zoom
        bind:panX
        bind:panY
        onsizes={(next) => (sizes = next)}
      />
    {:else}
      <div class="w-full h-full flex items-center justify-center text-sm text-neutral-400">
        {gallery.compareLoading
          ? locale.t("gallery.compare.loading")
          : locale.t("gallery.compare.load_failed")}
      </div>
    {/if}
    {#if gallery.compareLoading && bothLoaded}
      <div
        class="absolute top-5 left-1/2 -translate-x-1/2 px-2 py-0.5 rounded bg-black/70 text-[11px] text-neutral-300"
      >
        {locale.t("gallery.compare.loading")}
      </div>
    {/if}
  </div>

  <!-- Controls -->
  <div class="border-t border-neutral-800 bg-neutral-900/70 px-3 py-2 space-y-2">
    {#if showsDivider}
      <div class="flex items-center gap-3">
        <span class="text-xs text-neutral-400 w-16 shrink-0">
          {gallery.compareMode === "fade"
            ? locale.t("gallery.compare.blend")
            : locale.t("gallery.compare.position")}
        </span>
        <input
          type="range"
          min="0"
          max="100"
          step="0.5"
          bind:value={position}
          class="flex-1 accent-indigo-500"
          aria-label={locale.t("gallery.compare.divider")}
        />
        <span class="text-xs text-neutral-500 w-10 text-right tabular-nums"
          >{Math.round(position)}%</span
        >
      </div>
    {/if}

    <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
      {#each SIDES as side}
        <div class="flex items-center gap-2 min-w-0">
          <button
            class="flex items-center justify-center w-7 h-7 shrink-0 rounded bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
            title={locale.t("gallery.compare.prev")}
            onclick={() => gallery.navigateCompare(side, "prev")}
          >
            ‹
          </button>
          <button
            class="flex items-center justify-center w-7 h-7 shrink-0 rounded bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
            title={locale.t("gallery.compare.next")}
            onclick={() => gallery.navigateCompare(side, "next")}
          >
            ›
          </button>
          <span class="text-xs text-neutral-300 truncate" title={sideLabel(side)}>
            {sideLabel(side)}
          </span>
          {#if sideDims(side)}
            <span class="text-[11px] text-neutral-500 shrink-0 tabular-nums">{sideDims(side)}</span>
          {/if}
        </div>
      {/each}
    </div>

    <div class="text-[11px] text-neutral-500">{locale.t("gallery.compare.hint")}</div>
  </div>
</div>
