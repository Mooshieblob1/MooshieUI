<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";

  let checkpointSearch = $state("");
  let loraSearch = $state("");
  let showCheckpointDropdown = $state(false);
  let showLoraDropdown = $state<number | null>(null);

  const filteredCheckpoints = $derived(
    models.checkpoints.filter((m) =>
      m.toLowerCase().includes(checkpointSearch.toLowerCase())
    )
  );

  function filteredLoras(search: string) {
    return models.loras.filter((l) =>
      l.toLowerCase().includes(search.toLowerCase())
    );
  }

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
    <div class="flex items-center justify-between mb-1">
      <label class="text-xs text-neutral-400">LoRAs</label>
      <button
        onclick={() => generation.addLora()}
        class="text-xs text-indigo-400 hover:text-indigo-300 transition-colors"
      >
        + Add LoRA
      </button>
    </div>
    {#each generation.loras as lora, i}
      <div
        class="flex items-center gap-2 mb-2 bg-neutral-800 border border-neutral-700 rounded-lg p-2"
      >
        <div class="flex-1 min-w-0">
          <select
            bind:value={lora.name}
            class="w-full bg-neutral-750 border border-neutral-600 rounded px-2 py-1 text-xs text-neutral-100 focus:outline-none"
          >
            <option value="">Select LoRA...</option>
            {#each models.loras as l}
              <option value={l}>{l}</option>
            {/each}
          </select>
          <div class="flex gap-2 mt-1">
            <label class="flex items-center gap-1 text-xs text-neutral-500">
              Model
              <input
                type="range"
                bind:value={lora.strength_model}
                min="0"
                max="2"
                step="0.05"
                class="w-16 accent-indigo-500"
              />
              <span class="w-8 text-neutral-300"
                >{lora.strength_model.toFixed(2)}</span
              >
            </label>
            <label class="flex items-center gap-1 text-xs text-neutral-500">
              CLIP
              <input
                type="range"
                bind:value={lora.strength_clip}
                min="0"
                max="2"
                step="0.05"
                class="w-16 accent-indigo-500"
              />
              <span class="w-8 text-neutral-300"
                >{lora.strength_clip.toFixed(2)}</span
              >
            </label>
          </div>
        </div>
        <button
          onclick={() => generation.removeLora(i)}
          class="text-neutral-500 hover:text-red-400 transition-colors text-lg leading-none"
          title="Remove"
        >
          &times;
        </button>
      </div>
    {/each}
  </div>
</div>
