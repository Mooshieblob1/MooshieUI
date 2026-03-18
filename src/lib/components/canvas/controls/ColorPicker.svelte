<script lang="ts">
  import { canvas } from "../../../stores/canvas.svelte.js";
</script>

<div class="flex items-center gap-1.5">
  <div class="relative w-8 h-8">
    <label class="absolute bottom-0 right-0 w-5 h-5 rounded border border-neutral-600 cursor-pointer overflow-hidden" title="Background color">
      <input
        type="color"
        bind:value={canvas.backgroundColor}
        class="absolute inset-0 w-full h-full opacity-0 cursor-pointer"
      />
      <div class="w-full h-full" style="background: {canvas.backgroundColor}"></div>
    </label>
    <label class="absolute top-0 left-0 w-5 h-5 rounded border border-neutral-600 cursor-pointer overflow-hidden z-10" title="Foreground color">
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
