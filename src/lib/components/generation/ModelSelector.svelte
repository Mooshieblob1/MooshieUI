<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";

  let checkpointSearch = $state("");
  let showCheckpointDropdown = $state(false);
  let showLoraDropdown = $state<number | null>(null);
  let loraSearches = $state<Record<number, string>>({});

  const activeLoraCount = $derived(
    generation.loras.filter((l) => l.enabled && l.name).length
  );

  function filteredLorasForIndex(index: number) {
    const search = loraSearches[index] ?? "";
    return models.loras.filter((l) =>
      l.toLowerCase().includes(search.toLowerCase())
    );
  }

  function selectLora(index: number, name: string) {
    generation.loras = generation.loras.map((l, i) =>
      i === index ? { ...l, name } : l
    );
    showLoraDropdown = null;
    loraSearches = { ...loraSearches, [index]: "" };
  }

  function displayLoraName(fullPath: string): string {
    if (!fullPath) return "Select LoRA...";
    const parts = fullPath.replace(/\\/g, "/").split("/");
    return parts[parts.length - 1];
  }

  const filteredCheckpoints = $derived(
    models.checkpoints.filter((m) =>
      m.toLowerCase().includes(checkpointSearch.toLowerCase())
    )
  );

  function selectCheckpoint(name: string) {
    generation.checkpoint = name;
    checkpointSearch = "";
    showCheckpointDropdown = false;
  }
</script>

<div class="space-y-3">
  <!-- Checkpoint -->
  <div class="relative">
    <label class="block text-xs text-neutral-400 mb-1">Checkpoint</label>
    <button
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-left text-neutral-100 hover:border-neutral-600 focus:outline-none focus:border-indigo-500 transition-colors truncate"
      onclick={() => (showCheckpointDropdown = !showCheckpointDropdown)}
    >
      {generation.checkpoint || "Select checkpoint..."}
    </button>
    {#if showCheckpointDropdown}
      <div
        class="absolute z-50 mt-1 w-full bg-neutral-800 border border-neutral-700 rounded-lg shadow-xl max-h-60 overflow-hidden"
      >
        <input
          type="text"
          bind:value={checkpointSearch}
          placeholder="Search..."
          class="w-full bg-neutral-750 border-b border-neutral-700 px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none"
        />
        <div class="overflow-y-auto max-h-48">
          {#each filteredCheckpoints as ckpt}
            <button
              class="w-full text-left px-3 py-1.5 text-sm text-neutral-200 hover:bg-neutral-700 truncate"
              onclick={() => selectCheckpoint(ckpt)}
            >
              {ckpt}
            </button>
          {/each}
        </div>
      </div>
    {/if}
  </div>

  <!-- VAE -->
  <div>
    <label class="block text-xs text-neutral-400 mb-1">VAE</label>
    <select
      bind:value={generation.vae}
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
    >
      <option value="">Automatic (from checkpoint)</option>
      {#each models.vaes as vae}
        <option value={vae}>{vae}</option>
      {/each}
    </select>
  </div>

  <!-- LoRAs -->
  <div>
    <div class="flex items-center justify-between mb-1.5">
      <div class="flex items-center gap-2">
        <label class="text-xs text-neutral-400">LoRAs</label>
        {#if activeLoraCount > 0}
          <span class="text-[10px] px-1.5 py-0.5 rounded-full bg-indigo-600/20 text-indigo-400">
            {activeLoraCount} active
          </span>
        {/if}
      </div>
      <button
        onclick={() => generation.addLora()}
        class="text-xs text-indigo-400 hover:text-indigo-300 transition-colors"
      >
        + Add LoRA
      </button>
    </div>
    {#each generation.loras as lora, i}
      <div
        class="mb-2 rounded-lg border p-2.5 transition-opacity {lora.enabled
          ? 'bg-neutral-800 border-neutral-700'
          : 'bg-neutral-800/50 border-neutral-700/50 opacity-50'}"
      >
        <!-- Header row: toggle + name + remove -->
        <div class="flex items-center gap-2 mb-2">
          <button
            class="relative w-8 h-4 rounded-full transition-colors shrink-0 {lora.enabled
              ? 'bg-indigo-600'
              : 'bg-neutral-700'}"
            onclick={() => generation.toggleLora(i)}
            role="switch"
            aria-checked={lora.enabled}
            title={lora.enabled ? "Disable" : "Enable"}
          >
            <span
              class="absolute top-0.5 left-0.5 w-3 h-3 rounded-full bg-white transition-transform {lora.enabled
                ? 'translate-x-4'
                : ''}"
            ></span>
          </button>

          <!-- Searchable LoRA selector -->
          <div class="relative flex-1 min-w-0">
            <button
              class="w-full bg-neutral-750 border border-neutral-600 rounded px-2 py-1 text-xs text-left truncate transition-colors {lora.enabled
                ? 'text-neutral-100 hover:border-neutral-500'
                : 'text-neutral-500'}"
              onclick={() =>
                (showLoraDropdown = showLoraDropdown === i ? null : i)}
            >
              {displayLoraName(lora.name)}
            </button>
            {#if showLoraDropdown === i}
              <div
                class="absolute z-50 mt-1 w-full bg-neutral-800 border border-neutral-700 rounded-lg shadow-xl max-h-48 overflow-hidden"
              >
                <input
                  type="text"
                  bind:value={loraSearches[i]}
                  placeholder="Search LoRAs..."
                  class="w-full bg-neutral-750 border-b border-neutral-700 px-2 py-1.5 text-xs text-neutral-100 placeholder-neutral-500 focus:outline-none"
                />
                <div class="overflow-y-auto max-h-36">
                  {#each filteredLorasForIndex(i) as l}
                    <button
                      class="w-full text-left px-2 py-1 text-xs text-neutral-200 hover:bg-neutral-700 truncate"
                      onclick={() => selectLora(i, l)}
                    >
                      {l}
                    </button>
                  {/each}
                </div>
              </div>
            {/if}
          </div>

          <button
            onclick={() => generation.removeLora(i)}
            class="text-neutral-500 hover:text-red-400 transition-colors text-sm leading-none shrink-0"
            title="Remove"
          >
            &times;
          </button>
        </div>

        <!-- Strength sliders -->
        {#if lora.name}
          <div class="space-y-1.5">
            <div>
              <div class="flex items-center justify-between text-xs mb-0.5">
                <span class="text-neutral-500">Model</span>
                <span class="text-neutral-300 tabular-nums">{lora.strength_model.toFixed(2)}</span>
              </div>
              <input
                type="range"
                bind:value={lora.strength_model}
                min="0"
                max="2"
                step="0.05"
                class="w-full accent-indigo-500"
              />
            </div>
            <div>
              <div class="flex items-center justify-between text-xs mb-0.5">
                <span class="text-neutral-500">CLIP</span>
                <span class="text-neutral-300 tabular-nums">{lora.strength_clip.toFixed(2)}</span>
              </div>
              <input
                type="range"
                bind:value={lora.strength_clip}
                min="0"
                max="2"
                step="0.05"
                class="w-full accent-indigo-500"
              />
            </div>
          </div>
        {/if}
      </div>
    {/each}
  </div>
</div>
