<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";

  let searchQuery = $state("");

  const filteredCheckpoints = $derived(() => {
    const q = searchQuery.toLowerCase().trim();
    let list = models.checkpoints;
    if (q) {
      list = list.filter((name) => {
        const display = displayName(name).toLowerCase();
        return display.includes(q) || name.toLowerCase().includes(q);
      });
    }
    // Active checkpoint first, then alphabetical
    return [...list].sort((a, b) => {
      const aActive = a === generation.checkpoint;
      const bActive = b === generation.checkpoint;
      if (aActive !== bActive) return aActive ? -1 : 1;
      return displayName(a).localeCompare(displayName(b));
    });
  });

  function displayName(filename: string): string {
    const base = filename.split(/[\\/]/).pop() ?? filename;
    return base.replace(/\.(safetensors|ckpt|pt|bin)$/i, "");
  }

  function selectCheckpoint(filename: string) {
    generation.checkpoint = filename;
    generation.saveSettings();
  }
</script>

<div class="flex flex-col h-full">
  <!-- Search bar -->
  <div class="px-2 pt-1.5 pb-1 shrink-0">
    <input
      type="text"
      bind:value={searchQuery}
      placeholder={locale.t('checkpoint.search_placeholder')}
      class="w-full bg-neutral-800 border border-neutral-700 rounded px-2.5 py-1 text-xs text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
    />
  </div>

  {#if filteredCheckpoints().length === 0}
    <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs">
      <p>{locale.t('checkpoint.no_results')}</p>
    </div>
  {:else}
    <div class="flex gap-2.5 flex-1 min-h-0 overflow-x-auto px-2 py-1.5">
      {#each filteredCheckpoints() as name (name)}
        {@const isActive = name === generation.checkpoint}
        {@const label = displayName(name)}
        <button
          onclick={() => selectCheckpoint(name)}
          class="shrink-0 h-full aspect-[3/4] flex flex-col rounded-lg border bg-neutral-900/60 overflow-hidden transition-all text-left {isActive
            ? 'border-indigo-500/60 ring-1 ring-indigo-500/20'
            : 'border-neutral-800 hover:border-neutral-600'}"
          title={name}
        >
          <!-- Visual placeholder area -->
          <div class="relative flex-1 min-h-0 bg-neutral-950 flex items-center justify-center">
            {#if isActive}
              <div class="absolute top-1.5 left-1.5 z-10 px-1.5 py-0.5 rounded text-[9px] font-medium bg-indigo-600 text-white">
                {locale.t('checkpoint.active')}
              </div>
            {/if}
            <svg
              xmlns="http://www.w3.org/2000/svg"
              class="w-8 h-8 {isActive ? 'text-indigo-700' : 'text-neutral-700'}"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
              stroke-linecap="round"
              stroke-linejoin="round"
            >
              <path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/>
              <polyline points="3.27 6.96 12 12.01 20.73 6.96"/>
              <line x1="12" y1="22.08" x2="12" y2="12"/>
            </svg>
          </div>
          <!-- Name label -->
          <div class="px-1.5 py-1.5 shrink-0 border-t border-neutral-800">
            <p class="text-[10px] text-neutral-300 truncate leading-tight" title={name}>{label}</p>
          </div>
        </button>
      {/each}
    </div>
  {/if}
</div>
