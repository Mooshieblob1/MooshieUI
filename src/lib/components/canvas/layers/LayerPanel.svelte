<script lang="ts">
  import { canvas } from "../../../stores/canvas.svelte.js";
  import LayerItem from "./LayerItem.svelte";

  function handleAddRaster() {
    canvas.addLayer("raster");
  }

  function handleAddMask() {
    canvas.addLayer("mask");
  }

  function handleRemove() {
    if (canvas.activeLayerId) {
      canvas.removeLayer(canvas.activeLayerId);
    }
  }

  function handleDuplicate() {
    if (canvas.activeLayerId) {
      canvas.duplicateLayer(canvas.activeLayerId);
    }
  }

  function handleMoveUp() {
    if (canvas.activeLayerId) {
      canvas.reorderLayer(canvas.activeLayerId, "up");
    }
  }

  function handleMoveDown() {
    if (canvas.activeLayerId) {
      canvas.reorderLayer(canvas.activeLayerId, "down");
    }
  }
</script>

<div class="space-y-2">
  <div class="flex items-center justify-between">
    <h3 class="text-xs text-neutral-400 font-medium">Layers</h3>
    <div class="flex items-center gap-0.5">
      <button
        onclick={handleAddRaster}
        class="w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800"
        title="Add raster layer"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
      </button>
      <button
        onclick={handleAddMask}
        class="w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800"
        title="Add mask layer"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="16"/><line x1="8" y1="12" x2="16" y2="12"/></svg>
      </button>
      <button
        onclick={handleDuplicate}
        class="w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800"
        title="Duplicate layer"
        disabled={!canvas.activeLayerId}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
      </button>
      <button
        onclick={handleRemove}
        class="w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-red-400 hover:bg-neutral-800"
        title="Delete layer"
        disabled={!canvas.activeLayerId || canvas.layers.length <= 1}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
      </button>
    </div>
  </div>

  <!-- Layer list (top = highest order) -->
  <div class="space-y-0.5">
    {#each canvas.sortedLayers as layer (layer.id)}
      <LayerItem {layer} />
    {/each}
  </div>

  <!-- Layer reorder buttons -->
  {#if canvas.activeLayerId}
    <div class="flex items-center gap-1 pt-1">
      <button
        onclick={handleMoveUp}
        class="flex-1 py-1 text-[10px] text-neutral-500 hover:text-neutral-300 hover:bg-neutral-800 rounded transition-colors"
        title="Move layer up"
      >
        Move Up
      </button>
      <button
        onclick={handleMoveDown}
        class="flex-1 py-1 text-[10px] text-neutral-500 hover:text-neutral-300 hover:bg-neutral-800 rounded transition-colors"
        title="Move layer down"
      >
        Move Down
      </button>
    </div>
  {/if}

  <!-- Active layer opacity -->
  {#if canvas.activeLayer}
    <div>
      <label class="flex items-center justify-between text-[10px] text-neutral-500 mb-0.5">
        Opacity
        <span class="text-neutral-400 tabular-nums">{Math.round(canvas.activeLayer.opacity * 100)}%</span>
      </label>
      <input
        type="range"
        value={canvas.activeLayer.opacity}
        oninput={(e) => {
          if (canvas.activeLayerId) {
            canvas.setLayerOpacity(canvas.activeLayerId, parseFloat((e.target as HTMLInputElement).value));
          }
        }}
        min="0"
        max="1"
        step="0.05"
        class="w-full accent-indigo-500"
      />
    </div>
  {/if}
</div>
