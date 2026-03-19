<script lang="ts">
  import { canvas } from "../../../stores/canvas.svelte.js";
  import ColorTooltip from "../../ui/ColorTooltip.svelte";

  let showFgTooltip = $state(false);
  let showBgTooltip = $state(false);
  let tooltipPos = $state({ x: 0, y: 0 });

  function onEnter(e: MouseEvent, isFg: boolean) {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    tooltipPos = { x: rect.left, y: rect.bottom + 4 };
    if (isFg) {
      showFgTooltip = true;
    } else {
      showBgTooltip = true;
    }
  }
</script>

<div class="flex items-center gap-1.5">
  <div class="relative w-8 h-8">
    <label 
      class="absolute bottom-0 right-0 w-5 h-5 rounded border border-neutral-600 cursor-pointer overflow-hidden" 
      title="Background color"
      onmouseenter={(e) => onEnter(e, false)}
      onmouseleave={() => showBgTooltip = false}
    >
      <input
        type="color"
        bind:value={canvas.backgroundColor}
        class="absolute inset-0 w-full h-full opacity-0 cursor-pointer"
      />
      <div class="w-full h-full" style="background: {canvas.backgroundColor}"></div>
    </label>
    <label 
      class="absolute top-0 left-0 w-5 h-5 rounded border border-neutral-600 cursor-pointer overflow-hidden z-10" 
      title="Foreground color"
      onmouseenter={(e) => onEnter(e, true)}
      onmouseleave={() => showFgTooltip = false}
    >
      <input
        type="color"
        bind:value={canvas.foregroundColor}
        class="absolute inset-0 w-full h-full opacity-0 cursor-pointer"
      />
      <div class="w-full h-full" style="background: {canvas.foregroundColor}"></div>
    </label>
  </div>

  <button
    onclick={() => canvas.swapColors()}
    class="text-neutral-500 hover:text-neutral-300 text-[10px] font-bold"
    title="Swap colors (X)"
  >
    X
  </button>

  <button
    onclick={() => canvas.resetColors()}
    class="text-neutral-500 hover:text-neutral-300 text-[10px] font-bold"
    title="Reset colors (D)"
  >
    D
  </button>
</div>

{#if showFgTooltip}
  <div class="fixed" style="left: {tooltipPos.x}px; top: {tooltipPos.y}px; z-index: 100;">
    <ColorTooltip color={canvas.foregroundColor} />
  </div>
{/if}
{#if showBgTooltip}
  <div class="fixed" style="left: {tooltipPos.x}px; top: {tooltipPos.y}px; z-index: 100;">
    <ColorTooltip color={canvas.backgroundColor} />
  </div>
{/if}
