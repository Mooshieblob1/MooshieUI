<script lang="ts">
  import { compare } from "../../stores/compare.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
</script>

<div class="flex flex-col h-full">
  {#if !compare.enabled}
    <div class="flex items-center justify-center h-full">
      <button
        onclick={() => compare.enable()}
        class="flex items-center gap-2 px-4 py-2 text-xs font-medium rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white transition-colors"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/></svg>
        {locale.t("compare.enable")}
      </button>
    </div>
  {:else}
    <div class="flex flex-col h-full p-2 overflow-auto">
      <div class="flex gap-2 items-start">
        <!-- Grid -->
        <div class="grid gap-2" style="grid-template-columns: repeat({compare.cols}, 96px)">
          {#each compare.cells as _, i}
            {@const isActive = i === compare.activeIndex}
            {@const color = compare.cellColor(i)}
            {@const summary = compare.cellSummary(i)}
            <button
              onclick={() => compare.selectCell(i)}
              class="group relative w-24 h-24 rounded-lg p-2 text-left flex flex-col transition-all {isActive
                ? 'bg-neutral-800/80'
                : 'bg-neutral-900/60 border border-neutral-800 hover:border-neutral-600'}"
              style={isActive ? `box-shadow: 0 0 0 2px ${color}` : ''}
            >
              {#if compare.cellCount > 1}
                <span
                  role="button"
                  tabindex="-1"
                  class="absolute -top-1.5 -right-1.5 w-4 h-4 rounded-full bg-neutral-800 border border-neutral-600 text-neutral-400 hover:bg-red-900 hover:border-red-500 hover:text-red-300 flex items-center justify-center text-[10px] leading-none opacity-0 group-hover:opacity-100 transition-opacity z-10"
                  onkeydown={(e) => { if (e.key === 'Enter') { e.stopPropagation(); compare.cols > 1 ? compare.removeColumn(i % compare.cols) : compare.removeRow(Math.floor(i / compare.cols)); } }}
                  onclick={(e) => { e.stopPropagation(); compare.cols > 1 ? compare.removeColumn(i % compare.cols) : compare.removeRow(Math.floor(i / compare.cols)); }}
                >×</span>
              {/if}
              <div class="flex items-center gap-1.5 mb-1">
                <span class="w-3 h-3 rounded-sm shrink-0" style="background: {color}"></span>
                <span class="text-[11px] font-medium text-neutral-200">{compare.cellLabel(i)}</span>
              </div>
              <p class="text-[10px] text-neutral-500 leading-tight line-clamp-3 flex-1">{summary}</p>
            </button>
          {/each}
        </div>

        <!-- Add column button (right of grid) -->
        {#if compare.canAddColumn}
          <button
            onclick={() => compare.addColumn()}
            class="w-24 h-24 rounded-lg border border-dashed border-neutral-700 hover:border-indigo-500/60 flex items-center justify-center text-neutral-500 hover:text-indigo-400 transition-colors"
            title={locale.t("compare.add_column")}
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
          </button>
        {/if}
      </div>

      <!-- Add row button (below grid) -->
      {#if compare.canAddRow}
        <div class="pt-2">
          <button
            onclick={() => compare.addRow()}
            class="w-24 h-24 rounded-lg border border-dashed border-neutral-700 hover:border-indigo-500/60 flex items-center justify-center text-neutral-500 hover:text-indigo-400 transition-colors"
            title={locale.t("compare.add_row")}
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
          </button>
        </div>
      {/if}

      <!-- Footer -->
      <div class="flex items-center justify-between mt-auto pt-1 border-t border-neutral-800/50">
        <span class="text-[10px] text-neutral-500">
          {compare.cellCount} {locale.t("compare.images")}
        </span>
        <button
          onclick={() => compare.toggle()}
          class="text-[10px] px-2 py-0.5 rounded border border-neutral-700 text-neutral-400 hover:text-red-300 hover:border-red-500/50 transition-colors"
        >
          {locale.t("compare.disable")}
        </button>
      </div>
    </div>
  {/if}
</div>
