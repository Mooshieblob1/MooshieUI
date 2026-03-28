<script lang="ts">
  import { canvas } from "../../../stores/canvas.svelte.js";
  import { locale } from "../../../stores/locale.svelte.js";
</script>

<div class="flex items-center gap-2">
  <label class="flex items-center gap-2 text-xs text-neutral-400">
    {locale.t('canvas.brush_size')}
    <input
      type="range"
      bind:value={canvas.brushSettings.size}
      min="1"
      max="200"
      step="1"
      class="w-20 accent-indigo-500"
    />
    <span class="text-neutral-300 tabular-nums w-6 text-right text-[11px]">{canvas.brushSettings.size}</span>
  </label>

  <div class="w-px h-6 bg-neutral-700"></div>

  <label class="flex items-center gap-2 text-xs text-neutral-400">
    {locale.t('canvas.opacity')}
    <input
      type="range"
      bind:value={canvas.brushSettings.opacity}
      min="0.05"
      max="1"
      step="0.05"
      class="w-16 accent-indigo-500"
    />
    <span class="text-neutral-300 tabular-nums w-8 text-right text-[11px]">{Math.round(canvas.brushSettings.opacity * 100)}%</span>
  </label>
</div>
