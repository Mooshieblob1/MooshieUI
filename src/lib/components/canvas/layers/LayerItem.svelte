<script lang="ts">
  import { canvas, type CanvasLayer } from "../../../stores/canvas.svelte.js";
  import { locale } from "../../../stores/locale.svelte.js";

  interface Props {
    layer: CanvasLayer;
  }

  let { layer }: Props = $props();
  let isRenaming = $state(false);
  let renameValue = $state("");

  function startRename() {
    renameValue = layer.name;
    isRenaming = true;
  }

  function commitRename() {
    if (renameValue.trim()) {
      canvas.renameLayer(layer.id, renameValue.trim());
    }
    isRenaming = false;
  }

  function handleRenameKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") commitRename();
    if (e.key === "Escape") isRenaming = false;
  }

  const isActive = $derived(canvas.activeLayerId === layer.id);
  const thumb = $derived(canvas.layerThumbnails[layer.id]);
  const canDelete = $derived(canvas.layers.length > 1);
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="group rounded-md transition-colors cursor-pointer
    {isActive
      ? 'bg-neutral-800/60 ring-1 ring-inset ring-indigo-500/50'
      : 'hover:bg-neutral-800/40 ring-1 ring-inset ring-transparent'}"
  onclick={() => canvas.setActiveLayer(layer.id)}
  ondblclick={startRename}
>
  <div class="flex items-center gap-2 px-1.5 py-1.5">
    <!-- Visibility toggle (filled circle when visible, hollow when hidden) -->
    <button
      onclick={(e) => { e.stopPropagation(); canvas.toggleLayerVisibility(layer.id); }}
      class="shrink-0 w-5 h-5 flex items-center justify-center rounded hover:bg-neutral-700 {layer.visible ? 'text-indigo-400' : 'text-neutral-600'}"
      title={layer.visible ? locale.t('canvas.hide_layer') : locale.t('canvas.show_layer')}
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        {#if layer.visible}
          <circle cx="12" cy="12" r="6" fill="currentColor" stroke="none" />
        {:else}
          <circle cx="12" cy="12" r="6" fill="none" />
        {/if}
      </svg>
    </button>

    <!-- Pixel thumbnail on a checkerboard backdrop -->
    <div
      class="relative shrink-0 w-9 h-9 rounded overflow-hidden border border-neutral-700/60"
      style="background-color:#262626;background-image:linear-gradient(45deg,#333 25%,transparent 25%),linear-gradient(-45deg,#333 25%,transparent 25%),linear-gradient(45deg,transparent 75%,#333 75%),linear-gradient(-45deg,transparent 75%,#333 75%);background-size:8px 8px;background-position:0 0,0 4px,4px -4px,-4px 0;"
    >
      {#if thumb}
        <img src={thumb} alt="" class="absolute inset-0 w-full h-full object-contain" />
      {/if}
    </div>

    <!-- Name -->
    <div class="flex-1 min-w-0">
      {#if isRenaming}
        <!-- svelte-ignore a11y_autofocus -->
        <input
          type="text"
          bind:value={renameValue}
          onblur={commitRename}
          onkeydown={handleRenameKeydown}
          onclick={(e) => e.stopPropagation()}
          class="w-full bg-neutral-800 border border-neutral-600 rounded px-1 py-0.5 text-xs text-neutral-200 outline-none focus:border-indigo-500"
          autofocus
        />
      {:else}
        <span class="block truncate text-xs {isActive ? 'text-neutral-200' : 'text-neutral-400'}">{layer.name}</span>
      {/if}
    </div>

    <!-- Per-row actions (shown on hover, always on the active row) -->
    <div class="{isActive ? 'flex' : 'hidden group-hover:flex'} items-center gap-0.5 shrink-0">
      <button
        onclick={(e) => { e.stopPropagation(); canvas.reorderLayer(layer.id, 'up'); }}
        class="w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-700"
        title={locale.t('canvas.move_up_title')}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="18 15 12 9 6 15"/></svg>
      </button>
      <button
        onclick={(e) => { e.stopPropagation(); canvas.reorderLayer(layer.id, 'down'); }}
        class="w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-700"
        title={locale.t('canvas.move_down_title')}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
      </button>
      <button
        onclick={(e) => { e.stopPropagation(); canvas.duplicateLayer(layer.id); }}
        class="w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-700"
        title={locale.t('canvas.duplicate')}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
      </button>
      <button
        onclick={(e) => { e.stopPropagation(); if (canDelete) canvas.removeLayer(layer.id); }}
        class="w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-red-400 hover:bg-neutral-700 {canDelete ? '' : 'opacity-40 cursor-not-allowed'}"
        title={locale.t('canvas.delete_layer')}
        disabled={!canDelete}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
      </button>
    </div>

    <!-- Lock toggle -->
    <button
      onclick={(e) => { e.stopPropagation(); canvas.toggleLayerLock(layer.id); }}
      class="shrink-0 w-5 h-5 flex items-center justify-center rounded hover:bg-neutral-700 {layer.locked ? 'text-indigo-400' : 'text-neutral-600'}"
      title={layer.locked ? locale.t('canvas.unlock_layer') : locale.t('canvas.lock_layer')}
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        {#if layer.locked}
          <rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>
        {:else}
          <rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 9.9-1"/>
        {/if}
      </svg>
    </button>
  </div>

  <!-- Opacity sub-row (active layer only) -->
  {#if isActive}
    <div class="flex items-center gap-2 px-2 py-1.5 border-t border-neutral-700/60 bg-neutral-900/40">
      <span class="text-[10px] text-neutral-500 shrink-0">{locale.t('canvas.opacity')}</span>
      <input
        type="range"
        value={layer.opacity}
        oninput={(e) => canvas.setLayerOpacity(layer.id, parseFloat((e.target as HTMLInputElement).value))}
        onclick={(e) => e.stopPropagation()}
        min="0"
        max="1"
        step="0.05"
        class="flex-1 accent-indigo-500"
      />
      <span class="text-[10px] text-neutral-400 tabular-nums shrink-0 w-8 text-right">{Math.round(layer.opacity * 100)}%</span>
    </div>
  {/if}
</div>
