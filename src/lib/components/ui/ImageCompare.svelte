<script lang="ts">
  import type { CompareMode, CompareOrientation } from "../../types/index.js";

  interface Props {
    urlA: string;
    urlB: string;
    /** Corner badge text for each side. */
    labelA?: string;
    labelB?: string;
    ariaLabel?: string;
    mode?: CompareMode;
    orientation?: CompareOrientation;
    /** Divider position in slider mode, B opacity in fade mode. 0-100. */
    position?: number;
    zoom?: number;
    panX?: number;
    panY?: number;
    /** Fires whenever either image finishes decoding, with both natural sizes. */
    onsizes?: (sizes: { aw: number; ah: number; bw: number; bh: number }) => void;
  }

  let {
    urlA,
    urlB,
    labelA = "A",
    labelB = "B",
    ariaLabel = "Comparison divider",
    mode = "slider",
    orientation = "horizontal",
    position = $bindable(50),
    zoom = $bindable(1),
    panX = $bindable(0),
    panY = $bindable(0),
    onsizes,
  }: Props = $props();

  let containerEl = $state<HTMLElement | null>(null);
  let dragging = $state(false);
  let panning = $state(false);
  // Pan tracking — plain variables, no reactivity needed
  let panStartX = 0;
  let panStartY = 0;
  let panStartOffsetX = 0;
  let panStartOffsetY = 0;
  let naturalA = { w: 0, h: 0 };
  let naturalB = { w: 0, h: 0 };

  const horizontal = $derived(orientation === "horizontal");
  const slider = $derived(mode === "slider");

  // Both layers are clipped so transparent pixels in B never reveal A across
  // the divider — an exact split for RGBA images, not just opaque ones.
  const clipA = $derived(
    !slider ? "none" : horizontal ? `inset(0 ${100 - position}% 0 0)` : `inset(0 0 ${100 - position}% 0)`,
  );
  const clipB = $derived(
    !slider ? "none" : horizontal ? `inset(0 0 0 ${position}%)` : `inset(${position}% 0 0 0)`,
  );
  const opacityB = $derived(mode === "fade" ? position / 100 : 1);
  const blendB = $derived(mode === "difference" ? "difference" : "normal");

  function reportSize(e: Event, isB: boolean) {
    const img = e.currentTarget as HTMLImageElement;
    const size = { w: img.naturalWidth, h: img.naturalHeight };
    if (isB) naturalB = size;
    else naturalA = size;
    onsizes?.({ aw: naturalA.w, ah: naturalA.h, bw: naturalB.w, bh: naturalB.h });
  }

  function positionFromEvent(e: PointerEvent) {
    if (!containerEl) return position;
    const rect = containerEl.getBoundingClientRect();
    const pct = horizontal
      ? ((e.clientX - rect.left) / rect.width) * 100
      : ((e.clientY - rect.top) / rect.height) * 100;
    return Math.min(100, Math.max(0, pct));
  }

  function startDrag(e: PointerEvent) {
    if (e.button !== 0) return;
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    position = positionFromEvent(e);
    // Keep the drag from also starting a pan on the container underneath.
    e.stopPropagation();
    e.preventDefault();
  }

  function moveDrag(e: PointerEvent) {
    if (!dragging) return;
    position = positionFromEvent(e);
    e.preventDefault();
  }

  function endDrag(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    try {
      (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    } catch {
      // Pointer already released (cancel path) — nothing to do.
    }
  }

  function handleKey(e: KeyboardEvent) {
    const step = e.shiftKey ? 10 : 1;
    const decKey = horizontal ? "ArrowLeft" : "ArrowUp";
    const incKey = horizontal ? "ArrowRight" : "ArrowDown";
    if (e.key === decKey) position = Math.max(0, position - step);
    else if (e.key === incKey) position = Math.min(100, position + step);
    else if (e.key === "Home") position = 0;
    else if (e.key === "End") position = 100;
    else return;
    // Stop the viewer's global arrow handler from also navigating image B.
    e.stopPropagation();
    e.preventDefault();
  }

  function startPan(e: PointerEvent) {
    if (e.button !== 0 && e.button !== 1) return;
    panning = true;
    panStartX = e.clientX;
    panStartY = e.clientY;
    panStartOffsetX = panX;
    panStartOffsetY = panY;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    e.preventDefault();
  }

  function movePan(e: PointerEvent) {
    if (!panning) return;
    panX = panStartOffsetX + (e.clientX - panStartX);
    panY = panStartOffsetY + (e.clientY - panStartY);
    e.preventDefault();
  }

  function endPan(e: PointerEvent) {
    if (!panning) return;
    panning = false;
    try {
      (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    } catch {
      // Pointer already released (cancel path) — nothing to do.
    }
  }

  function zoomAtCursor(e: WheelEvent) {
    e.preventDefault();
    // Zoom around the cursor: the box the images are fit into is the reference,
    // which in side-by-side mode is the hovered pane rather than the whole view.
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const dx = e.clientX - (rect.left + rect.width / 2);
    const dy = e.clientY - (rect.top + rect.height / 2);
    const next = Math.min(10, Math.max(0.5, zoom * (e.deltaY > 0 ? 0.9 : 1.1)));
    if (next === zoom) return;
    const ratio = 1 - next / zoom;
    panX += dx * ratio;
    panY += dy * ratio;
    zoom = next;
    if (zoom <= 1) {
      panX = 0;
      panY = 0;
    }
  }

  function resetView() {
    zoom = 1;
    panX = 0;
    panY = 0;
  }
</script>

{#snippet picture(url: string, alt: string, isB: boolean)}
  <img
    src={url}
    {alt}
    draggable="false"
    class="max-w-full max-h-full object-contain pointer-events-none origin-center"
    style="transform: translate(var(--cx, 0px), var(--cy, 0px)) scale(var(--cz, 1));"
    onload={(e) => reportSize(e, isB)}
  />
{/snippet}

{#snippet badge(text: string, extraClass: string)}
  <div
    class="absolute px-2 py-0.5 rounded bg-black/70 text-[11px] font-medium text-neutral-100 pointer-events-none {extraClass}"
  >
    {text}
  </div>
{/snippet}

{#if mode === "side_by_side"}
  <div
    bind:this={containerEl}
    class="w-full h-full flex gap-1 select-none touch-none {horizontal ? 'flex-row' : 'flex-col'}"
    style="--cx: {panX}px; --cy: {panY}px; --cz: {zoom};"
  >
    <!-- Pan/zoom surfaces: no ARIA role fits a viewport, and the keyboard path
         lives on the divider handle and the footer controls. -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="relative flex-1 min-w-0 min-h-0 overflow-hidden flex items-center justify-center {panning
        ? 'cursor-grabbing'
        : 'cursor-grab'}"
      onwheel={zoomAtCursor}
      onpointerdown={startPan}
      onpointermove={movePan}
      onpointerup={endPan}
      onpointercancel={endPan}
      ondblclick={resetView}
    >
      {@render picture(urlA, labelA, false)}
      {@render badge(labelA, "top-2 left-2")}
    </div>
    <div class="{horizontal ? 'w-px h-full' : 'h-px w-full'} bg-neutral-700 shrink-0"></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="relative flex-1 min-w-0 min-h-0 overflow-hidden flex items-center justify-center {panning
        ? 'cursor-grabbing'
        : 'cursor-grab'}"
      onwheel={zoomAtCursor}
      onpointerdown={startPan}
      onpointermove={movePan}
      onpointerup={endPan}
      onpointercancel={endPan}
      ondblclick={resetView}
    >
      {@render picture(urlB, labelB, true)}
      {@render badge(labelB, "top-2 right-2")}
    </div>
  </div>
{:else}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={containerEl}
    class="relative w-full h-full overflow-hidden select-none touch-none isolate {panning
      ? 'cursor-grabbing'
      : 'cursor-grab'}"
    style="--cx: {panX}px; --cy: {panY}px; --cz: {zoom};"
    onwheel={zoomAtCursor}
    onpointerdown={startPan}
    onpointermove={movePan}
    onpointerup={endPan}
    onpointercancel={endPan}
    ondblclick={resetView}
  >
    <div class="absolute inset-0 flex items-center justify-center" style="clip-path: {clipA};">
      {@render picture(urlA, labelA, false)}
    </div>
    <div
      class="absolute inset-0 flex items-center justify-center"
      style="clip-path: {clipB}; opacity: {opacityB}; mix-blend-mode: {blendB};"
    >
      {@render picture(urlB, labelB, true)}
    </div>

    {#if slider}
      {@render badge(labelA, horizontal ? "top-2 left-2" : "top-2 left-2")}
      {@render badge(labelB, horizontal ? "top-2 right-2" : "bottom-2 left-2")}
      <div
        class="absolute z-10 flex items-center justify-center touch-none {horizontal
          ? 'top-0 bottom-0 w-8 -ml-4 cursor-ew-resize'
          : 'left-0 right-0 h-8 -mt-4 cursor-ns-resize'}"
        style={horizontal ? `left: ${position}%;` : `top: ${position}%;`}
        role="slider"
        tabindex="0"
        aria-label={ariaLabel}
        aria-orientation={horizontal ? "horizontal" : "vertical"}
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow={Math.round(position)}
        onpointerdown={startDrag}
        onpointermove={moveDrag}
        onpointerup={endDrag}
        onpointercancel={endDrag}
        onkeydown={handleKey}
      >
        <div
          class="absolute bg-white/90 {horizontal ? 'top-0 bottom-0 w-0.5' : 'left-0 right-0 h-0.5'}"
        ></div>
        <div
          class="w-8 h-8 rounded-full bg-white/95 text-neutral-900 flex items-center justify-center text-sm font-bold shadow-lg ring-1 ring-black/30 focus-within:ring-2 focus-within:ring-indigo-400"
        >
          {horizontal ? "⇄" : "⇅"}
        </div>
      </div>
    {:else}
      {@render badge(labelA, "top-2 left-2")}
      {@render badge(labelB, "top-2 right-2")}
    {/if}
  </div>
{/if}
