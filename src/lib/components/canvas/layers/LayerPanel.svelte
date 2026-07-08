<script lang="ts">
  import { canvas } from "../../../stores/canvas.svelte.js";
  import { locale } from "../../../stores/locale.svelte.js";
  import LayerItem from "./LayerItem.svelte";

  type GroupKey = "mask" | "raster";

  let collapsed = $state<Record<GroupKey, boolean>>({ mask: false, raster: false });

  const maskLayers = $derived(canvas.sortedLayers.filter((l) => l.type === "mask"));
  const rasterLayers = $derived(canvas.sortedLayers.filter((l) => l.type === "raster"));
  const activeType = $derived(canvas.activeLayer?.type ?? null);

  const groups = $derived([
    { key: "mask" as GroupKey, titleKey: "canvas.mask_layers", addKey: "canvas.add_mask", layers: maskLayers },
    { key: "raster" as GroupKey, titleKey: "canvas.raster_layers", addKey: "canvas.add_raster", layers: rasterLayers },
  ]);

  function toggleGroup(key: GroupKey) {
    collapsed = { ...collapsed, [key]: !collapsed[key] };
  }

  function addToGroup(key: GroupKey) {
    canvas.addLayer(key);
    // Adding a layer should reveal it, so make sure the group is open.
    collapsed = { ...collapsed, [key]: false };
  }
</script>

<div class="space-y-2">
  {#each groups as group (group.key)}
    <div>
      <!-- Group header -->
      <div class="flex items-center gap-1">
        <button
          onclick={() => toggleGroup(group.key)}
          class="flex-1 flex items-center gap-1.5 py-1 text-left"
        >
          <svg
            xmlns="http://www.w3.org/2000/svg"
            class="w-3 h-3 shrink-0 text-neutral-500 transition-transform {collapsed[group.key] ? '' : 'rotate-90'}"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2.5"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <polyline points="9 18 15 12 9 6" />
          </svg>
          <span class="text-xs font-medium {activeType === group.key ? 'text-neutral-200' : 'text-neutral-500'}">
            {locale.t(group.titleKey)}
          </span>
          <span class="px-1.5 rounded-full bg-neutral-800 text-[10px] text-neutral-400 tabular-nums">
            {group.layers.length}
          </span>
        </button>
        <button
          onclick={() => addToGroup(group.key)}
          class="w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800"
          title={locale.t(group.addKey)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
        </button>
      </div>

      <!-- Group body -->
      {#if !collapsed[group.key]}
        <div class="space-y-0.5 mt-1">
          {#if group.layers.length === 0}
            <p class="px-2 py-1.5 text-[10px] text-neutral-600 italic">{locale.t('canvas.no_layers')}</p>
          {:else}
            {#each group.layers as layer (layer.id)}
              <LayerItem {layer} />
            {/each}
          {/if}
        </div>
      {/if}
    </div>
  {/each}
</div>
