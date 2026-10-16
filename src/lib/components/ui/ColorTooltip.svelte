<script module lang="ts">
  import { getColorName, initColors, ORIGINAL_COLORS } from "ntc-ts";

  // ntc-ts starts with a palette of just Black; without this every color is named "Black".
  initColors(ORIGINAL_COLORS);
</script>

<script lang="ts">

  interface Props {
    color: string;
  }
  let { color }: Props = $props();

  let colorName = $derived(getColorName(color).name);
</script>

<div class="px-3 py-2 rounded-lg bg-neutral-800 border border-neutral-700 shadow-2xl text-[11px] text-neutral-300 leading-relaxed">
  <div class="flex items-center gap-2">
    <div class="w-4 h-4 rounded border border-neutral-600" style="background-color: {color};"></div>
    <span>{colorName} ({color})</span>
  </div>
</div>
