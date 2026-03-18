<script lang="ts">
  import { canvas } from "../../stores/canvas.svelte.js";
</script>

<div class="flex items-center justify-between px-3 py-1 bg-neutral-900 border-t border-neutral-800 text-[11px] text-neutral-500">
  <div class="flex items-center gap-4">
    <span>{canvas.canvasWidth} &times; {canvas.canvasHeight}</span>
    {#if canvas.cursorPos}
      <span class="tabular-nums">
        x: {Math.round(canvas.cursorPos.x)}, y: {Math.round(canvas.cursorPos.y)}
      </span>
    {/if}
  </div>
  <div class="flex items-center gap-4">
    {#if canvas.activeLayer}
      <span class="text-neutral-400">{canvas.activeLayer.name}</span>
    {/if}
    <span class="tabular-nums">{canvas.zoomPercent}%</span>
  </div>
</div>
