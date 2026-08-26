<script lang="ts">
  /**
   * Character placement canvas for NovelAI's V5 free-form coordinates.
   *
   * Every enabled character is a numbered circle on a canvas drawn at the
   * selected aspect ratio; dragging a circle sets the normalised 0..1 centre
   * the API takes. The canvas used to sit inline under the character list,
   * but at panel width it was too small to place anything precisely, so it
   * lives here instead and the panel keeps only the toggle that opens it.
   *
   * Mounted at the app root like the other NovelAI modals, not inside the
   * prompt panel: the panel is a scroll container, which bounds a `fixed`
   * overlay rendered within it. `novelai.characterPositionOpen` is therefore
   * the open state, so the button and the modal need not be siblings.
   *
   * The last generation is used as the backdrop when its aspect ratio matches
   * the one about to be generated, so placement is judged against a real
   * composition rather than an empty rectangle. `OutputImage` carries no
   * dimensions, so the ratio is measured off the decoded image; anything that
   * does not match (or fails to load) falls back to the plain black canvas.
   */
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { novelai } from "../../stores/novelai.svelte.js";
  import { novelAiOverlappingCharacters } from "../../utils/novelaiModels.js";

  function onclose() {
    novelai.characterPositionOpen = false;
  }

  /**
   * Panel width in px, resizable by the corner grip.
   *
   * The panel is centred by the overlay's flexbox and stays centred while it
   * grows: only the width is user-set, so neither edge is anchored and the
   * canvas keeps its position on screen. Component-local because the modal is
   * root-mounted and never unmounts, so a session's choice survives reopening
   * without being written to settings.
   */
  const DEFAULT_PANEL_WIDTH = 706;
  const MIN_PANEL_WIDTH = 360;
  /** Arrow-key step for resizing without a pointer. */
  const RESIZE_STEP = 48;

  let panelWidth = $state(DEFAULT_PANEL_WIDTH);
  let resizing = $state(false);

  function maxPanelWidth(): number {
    return Math.max(MIN_PANEL_WIDTH, window.innerWidth - 32);
  }

  function setPanelWidth(px: number) {
    panelWidth = Math.min(maxPanelWidth(), Math.max(MIN_PANEL_WIDTH, px));
  }

  function startResize(e: PointerEvent) {
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    resizing = true;
  }

  /**
   * The panel is centred, so its right edge sits at (viewport + width) / 2.
   * Solving for width keeps the grip under the pointer instead of drifting at
   * half speed, which is what tracking the raw delta would do.
   */
  function moveResize(e: PointerEvent) {
    if (!resizing) return;
    setPanelWidth(2 * e.clientX - window.innerWidth);
  }

  function endResize() {
    resizing = false;
  }

  function resizeKey(e: KeyboardEvent) {
    if (e.key === "ArrowLeft") setPanelWidth(panelWidth - RESIZE_STEP);
    else if (e.key === "ArrowRight") setPanelWidth(panelWidth + RESIZE_STEP);
    else return;
    e.preventDefault();
  }

  const characters = $derived(generation.novelaiSettings.characters);

  let canvasEl: HTMLDivElement | null = $state(null);
  /** Character being dragged, or null. */
  let dragIndex: number | null = $state(null);
  /**
   * Live position while a drag is in flight. The store is only written on
   * release: `updateNovelAiSettings` persists settings on every call, and a
   * drag emits pointermoves far faster than settings should be saved.
   */
  let dragPos: { x: number; y: number } | null = $state(null);

  /** Backdrop URL, set only once the image has decoded at a matching ratio. */
  let backdropUrl = $state<string | null>(null);

  /**
   * Aspect ratios rarely land on the same float: a 832x1216 generation and a
   * gallery entry saved at 833x1216 are the same composition to the eye. 2%
   * is wide enough to absorb that and narrow enough to reject a portrait
   * backdrop under a landscape canvas.
   */
  const RATIO_TOLERANCE = 0.02;

  $effect(() => {
    // Root-mounted, so this component is alive whether or not the modal is up.
    // No probe until it is opened: nothing would see the result.
    if (!novelai.characterPositionOpen) {
      backdropUrl = null;
      return;
    }
    const target = generation.height > 0 ? generation.width / generation.height : 0;
    const latest = gallery.sessionImages[0] ?? gallery.images[0];
    const url = latest?.fullImageUrl ?? latest?.url ?? latest?.thumbnailUrl;
    if (!url || target <= 0) {
      backdropUrl = null;
      return;
    }
    let cancelled = false;
    const probe = new Image();
    probe.onload = () => {
      if (cancelled) return;
      const ratio = probe.naturalHeight > 0 ? probe.naturalWidth / probe.naturalHeight : 0;
      const matches = ratio > 0 && Math.abs(ratio - target) / target <= RATIO_TOLERANCE;
      backdropUrl = matches ? url : null;
    };
    probe.onerror = () => {
      if (!cancelled) backdropUrl = null;
    };
    probe.src = url;
    return () => {
      cancelled = true;
    };
  });

  function clamp01(v: number): number {
    return Math.min(1, Math.max(0, v));
  }

  /** Where a character's circle sits right now, drag override included. */
  function circlePos(index: number): { x: number; y: number } {
    if (dragIndex === index && dragPos) return dragPos;
    const c = characters[index]?.center;
    return { x: c?.x ?? 0.5, y: c?.y ?? 0.5 };
  }

  function pointerToCanvas(e: PointerEvent): { x: number; y: number } | null {
    if (!canvasEl) return null;
    const rect = canvasEl.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return null;
    return {
      x: clamp01((e.clientX - rect.left) / rect.width),
      y: clamp01((e.clientY - rect.top) / rect.height),
    };
  }

  function startDrag(e: PointerEvent, index: number) {
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragIndex = index;
    dragPos = circlePos(index);
  }

  function moveDrag(e: PointerEvent) {
    if (dragIndex === null) return;
    const pos = pointerToCanvas(e);
    if (pos) dragPos = pos;
  }

  function endDrag() {
    if (dragIndex === null) return;
    if (dragPos) {
      generation.updateNovelAiCharacter(dragIndex, {
        center: {
          x: Math.round(dragPos.x * 100) / 100,
          y: Math.round(dragPos.y * 100) / 100,
        },
      });
    }
    dragIndex = null;
    dragPos = null;
  }

  /** Arrow keys nudge a circle in 5% steps, for placement without a pointer. */
  function nudge(e: KeyboardEvent, index: number) {
    const step = 0.05;
    let dx = 0;
    let dy = 0;
    if (e.key === "ArrowLeft") dx = -step;
    else if (e.key === "ArrowRight") dx = step;
    else if (e.key === "ArrowUp") dy = -step;
    else if (e.key === "ArrowDown") dy = step;
    else return;
    e.preventDefault();
    const pos = circlePos(index);
    generation.updateNovelAiCharacter(index, {
      center: {
        x: Math.round(clamp01(pos.x + dx) * 100) / 100,
        y: Math.round(clamp01(pos.y + dy) * 100) / 100,
      },
    });
  }

  /**
   * Characters whose circle sits within NovelAI's stacking distance of another.
   *
   * Keyed by character index so the canvas can mark the exact circles at
   * fault. Only enabled characters take part, since the disabled ones are
   * neither drawn nor sent. Reads the live drag position rather than the
   * stored one so the warning tracks the circle under the cursor instead of
   * waiting for release.
   */
  const overlapping = $derived.by(() => {
    const drawn = characters
      .map((character, index) => ({ character, index }))
      .filter(({ character }) => character.enabled);
    const stacked = novelAiOverlappingCharacters(drawn.map(({ index }) => circlePos(index)));
    return new Set([...stacked].map((i) => drawn[i].index));
  });

  /** Circle colours, turning red once a character is stacked on another. */
  function circleClass(index: number): string {
    const dragging = dragIndex === index;
    if (overlapping.has(index)) {
      return dragging
        ? "bg-red-500 text-white cursor-grabbing"
        : "bg-red-600 text-white hover:bg-red-500 cursor-grab";
    }
    return dragging
      ? "bg-indigo-500 text-white cursor-grabbing"
      : "bg-indigo-600 text-white hover:bg-indigo-500 cursor-grab";
  }

  function handleEscape(e: KeyboardEvent) {
    if (!novelai.characterPositionOpen || e.key !== "Escape") return;
    e.preventDefault();
    onclose();
  }
</script>

<!-- Outside the block: `<svelte:window>` may not sit inside one. handleEscape
     ignores the key unless the modal is up. -->
<svelte:window onkeydown={handleEscape} />

{#if novelai.characterPositionOpen}
  <div
    class="fixed inset-0 z-80 flex items-center justify-center bg-black/70 p-4"
    role="dialog"
    aria-modal="true"
    aria-label={locale.t("generation.novelai.characters.position_modal_title")}
  >
    <div
      class="flex max-h-full w-full flex-col overflow-hidden rounded-xl border border-neutral-700 bg-neutral-900 shadow-2xl"
      style="width: {panelWidth}px; max-width: calc(100vw - 2rem);"
    >
      <div class="flex shrink-0 items-center justify-between gap-3 border-b border-neutral-700 px-5 py-4">
        <div>
          <h2 class="text-sm font-medium text-neutral-100">
            {locale.t("generation.novelai.characters.position_modal_title")}
          </h2>
          <p class="mt-0.5 text-[11px] text-neutral-500">
            {locale.t("generation.novelai.characters.drag_hint")}
          </p>
        </div>
        <button
          class="rounded-md p-1 text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-200 cursor-pointer"
          onclick={onclose}
          aria-label={locale.t("common.close")}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>

      <div class="min-h-0 flex-1 space-y-2 overflow-y-auto p-4">
        <!-- Capping the width by the aspect ratio keeps a tall canvas inside the
           viewport as the panel widens, without distorting it. -->
      <div
        class="mx-auto w-full"
        style="max-width: calc(65vh * {generation.width} / {generation.height});"
      >
          <div
            bind:this={canvasEl}
            class="relative w-full select-none touch-none overflow-hidden rounded-lg border border-neutral-800 bg-black"
            style="aspect-ratio: {generation.width} / {generation.height};"
          >
            {#if backdropUrl}
              <img src={backdropUrl} alt="" class="absolute inset-0 h-full w-full object-cover" />
              <div class="absolute inset-0 bg-black/55"></div>
            {/if}
            {#each characters as character, index (index)}
              {#if character.enabled}
                {@const pos = circlePos(index)}
                <button
                  class="absolute w-7 h-7 -ml-3.5 -mt-3.5 rounded-full text-xs font-semibold flex items-center justify-center transition-colors focus:outline-none focus:ring-2 focus:ring-indigo-400 {circleClass(
                    index,
                  )}"
                  style="left: {pos.x * 100}%; top: {pos.y * 100}%;"
                  aria-label={locale.t("generation.novelai.characters.label", {
                    index: String(index + 1),
                  })}
                  onpointerdown={(e) => startDrag(e, index)}
                  onpointermove={moveDrag}
                  onpointerup={endDrag}
                  onpointercancel={endDrag}
                  onkeydown={(e) => nudge(e, index)}
                >
                  {index + 1}
                </button>
              {/if}
            {/each}
          </div>
        </div>

        {#if overlapping.size > 0}
          <p class="text-[11px] text-red-400">
            {locale.t("generation.novelai.characters.overlap_warning")}
          </p>
        {/if}
      </div>

      <div class="flex shrink-0 items-center justify-end gap-2 border-t border-neutral-700 py-3 pl-5 pr-3">
        <button
          class="rounded-lg bg-indigo-600 px-4 py-1.5 text-xs text-white transition-colors hover:bg-indigo-500 cursor-pointer"
          onclick={onclose}
        >
          {locale.t("generation.novelai.characters.position_modal_done")}
        </button>
        <button
          class="cursor-ew-resize touch-none rounded-md p-1 transition-colors {resizing
            ? 'text-neutral-200'
            : 'text-neutral-600 hover:text-neutral-300'}"
          aria-label={locale.t("generation.novelai.characters.resize")}
          title={locale.t("generation.novelai.characters.resize")}
          onpointerdown={startResize}
          onpointermove={moveResize}
          onpointerup={endResize}
          onpointercancel={endResize}
          ondblclick={() => setPanelWidth(DEFAULT_PANEL_WIDTH)}
          onkeydown={resizeKey}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="21" y1="15" x2="15" y2="21"/><line x1="21" y1="9" x2="9" y2="21"/></svg>
        </button>
      </div>
    </div>
  </div>
{/if}
