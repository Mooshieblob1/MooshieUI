<script lang="ts">
  import { canvas, type CanvasLayer } from "../../../stores/canvas.svelte.js";

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
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="flex items-center gap-1.5 px-2 py-1.5 rounded-md text-xs transition-colors cursor-pointer
    {isActive ? 'bg-indigo-600/20 border border-indigo-500/40' : 'hover:bg-neutral-800 border border-transparent'}"
  onclick={() => canvas.setActiveLayer(layer.id)}
  ondblclick={startRename}
>
  <!-- Layer type icon -->
  <div class="shrink-0 w-4 h-4 flex items-center justify-center text-neutral-500">
    {#if layer.type === "mask"}
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/></svg>
    {:else}
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/></svg>
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
        class="w-full bg-neutral-800 border border-neutral-600 rounded px-1 py-0.5 text-xs text-neutral-200 outline-none focus:border-indigo-500"
        autofocus
      />
    {:else}
      <span class="block truncate {isActive ? 'text-neutral-200' : 'text-neutral-400'}">{layer.name}</span>
    {/if}
  </div>

  <!-- Visibility toggle -->
  <button
    onclick={(e) => { e.stopPropagation(); canvas.toggleLayerVisibility(layer.id); }}
    class="shrink-0 w-5 h-5 flex items-center justify-center rounded hover:bg-neutral-700 {layer.visible ? 'text-neutral-400' : 'text-neutral-600'}"
    title={layer.visible ? "Hide layer" : "Show layer"}
  >
    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      {#if layer.visible}
        <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>
      {:else}
        <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/>
      {/if}
    </svg>
  </button>

  <!-- Lock toggle -->
  <button
    onclick={(e) => { e.stopPropagation(); canvas.toggleLayerLock(layer.id); }}
    class="shrink-0 w-5 h-5 flex items-center justify-center rounded hover:bg-neutral-700 {layer.locked ? 'text-amber-400' : 'text-neutral-600'}"
    title={layer.locked ? "Unlock layer" : "Lock layer"}
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
