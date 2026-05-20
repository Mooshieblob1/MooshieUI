<script lang="ts">
  import { onMount, onDestroy } from "svelte";

  type Snap = "peek" | "half" | "full" | "closed";

  interface Props {
    open: boolean;
    snap?: Snap;
    snaps?: Snap[];
    onClose?: () => void;
    onSnapChange?: (s: Snap) => void;
    title?: string;
    /** When true, sheet has no max-height and fills the viewport (action-sheet style). */
    auto?: boolean;
    children?: import("svelte").Snippet;
  }

  let {
    open,
    snap = $bindable("half"),
    snaps = ["peek", "half", "full"],
    onClose,
    onSnapChange,
    title,
    auto = false,
    children,
  }: Props = $props();

  // Heights (vh) per snap point.
  const heights: Record<Snap, number> = {
    closed: 0,
    peek: 30,
    half: 60,
    full: 92,
  };

  let dragging = $state(false);
  let dragStartY = 0;
  let dragStartHeight = 0;
  let currentHeightVh = $state(60);
  let sheetEl = $state<HTMLDivElement | null>(null);

  $effect(() => {
    if (open && !dragging) {
      currentHeightVh = heights[snap];
    } else if (!open) {
      currentHeightVh = 0;
    }
  });

  function onHandlePointerDown(e: PointerEvent) {
    if (!open) return;
    dragging = true;
    dragStartY = e.clientY;
    dragStartHeight = currentHeightVh;
    (e.target as HTMLElement).setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }

  function onHandlePointerMove(e: PointerEvent) {
    if (!dragging) return;
    const dy = dragStartY - e.clientY; // up = positive
    const vh = window.innerHeight || 800;
    const dvh = (dy / vh) * 100;
    currentHeightVh = Math.max(0, Math.min(95, dragStartHeight + dvh));
  }

  function onHandlePointerUp() {
    if (!dragging) return;
    dragging = false;
    // Snap to closest allowed snap.
    const allowed = snaps.filter((s) => heights[s] > 0);
    let best: Snap = allowed[0] ?? "half";
    let bestDiff = Infinity;
    for (const s of allowed) {
      const diff = Math.abs(heights[s] - currentHeightVh);
      if (diff < bestDiff) {
        bestDiff = diff;
        best = s;
      }
    }
    // If user dragged below 50% of smallest snap, treat as close intent.
    const minH = heights[allowed[0] ?? "peek"];
    if (currentHeightVh < minH * 0.5) {
      onClose?.();
      return;
    }
    snap = best;
    currentHeightVh = heights[best];
    onSnapChange?.(best);
  }

  // Esc closes
  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && open) onClose?.();
  }
  onMount(() => window.addEventListener("keydown", onKey));
  onDestroy(() => window.removeEventListener("keydown", onKey));

  function onBackdropClick() {
    onClose?.();
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-50 bg-black/60 transition-opacity tap-highlight-none"
    onclick={onBackdropClick}
  ></div>
  <div
    bind:this={sheetEl}
    class="fixed left-0 right-0 bottom-0 z-50 flex flex-col bg-neutral-900 border-t border-neutral-800 rounded-t-2xl shadow-2xl no-scroll-chain"
    style="height: {currentHeightVh}vh; max-height: {auto ? '90vh' : '95vh'}; transition: {dragging ? 'none' : 'height 0.18s cubic-bezier(0.32, 0.72, 0, 1)'};"
    role="dialog"
    aria-modal="true"
  >
    <!-- Drag handle -->
    <button
      type="button"
      class="w-full py-1.5 flex flex-col items-center cursor-grab active:cursor-grabbing select-none touch-none"
      onpointerdown={onHandlePointerDown}
      onpointermove={onHandlePointerMove}
      onpointerup={onHandlePointerUp}
      onpointercancel={onHandlePointerUp}
      aria-label="Drag to resize"
    >
      <div class="bottom-sheet-handle"></div>
    </button>
    {#if title}
      <div class="px-4 pb-2 flex items-center justify-between">
        <h2 class="text-sm font-semibold text-neutral-100">{title}</h2>
        <button
          type="button"
          class="touch-target text-neutral-400 hover:text-neutral-200 -mr-2 px-2"
          onclick={onClose}
          aria-label="Close"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none"
            stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
          </svg>
        </button>
      </div>
    {/if}
    <div class="flex-1 min-h-0 overflow-y-auto overscroll-contain px-4 pb-[max(env(safe-area-inset-bottom),1rem)]">
      {@render children?.()}
    </div>
  </div>
{/if}
