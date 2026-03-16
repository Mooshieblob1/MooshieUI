<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";

  let randomSeed = $derived(generation.seed === -1);
</script>

<div class="space-y-3">
  <div class="grid grid-cols-2 gap-3">
    <!-- Sampler -->
    <div>
      <label class="block text-xs text-neutral-400 mb-1">Sampler</label>
      <select
        bind:value={generation.samplerName}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        {#each models.samplers as s}
          <option value={s}>{s}</option>
        {/each}
      </select>
    </div>

    <!-- Scheduler -->
    <div>
      <label class="block text-xs text-neutral-400 mb-1">Scheduler</label>
      <select
        bind:value={generation.scheduler}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        {#each models.schedulers as s}
          <option value={s}>{s}</option>
        {/each}
      </select>
    </div>
  </div>

  <div class="grid grid-cols-2 gap-3">
    <!-- Steps -->
    <div>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        Steps
        <span class="text-neutral-300">{generation.steps}</span>
      </label>
      <input
        type="range"
        bind:value={generation.steps}
        min="1"
        max="150"
        step="1"
        class="w-full accent-indigo-500"
      />
    </div>

    <!-- CFG -->
    <div>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        CFG Scale
        <input
          type="number"
          bind:value={generation.cfg}
          min="0"
          max="30"
          step="0.1"
          class="w-16 bg-neutral-800 border border-neutral-700 rounded px-1.5 py-0.5 text-xs text-neutral-300 text-right focus:outline-none focus:border-indigo-500"
        />
      </label>
      <input
        type="range"
        bind:value={generation.cfg}
        min="0"
        max="30"
        step="0.1"
        class="w-full accent-indigo-500"
      />
    </div>
  </div>

  <!-- Seed -->
  <div>
    <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
      Seed
      <button
        class="text-xs px-2 py-0.5 rounded {randomSeed
          ? 'bg-indigo-600 text-white'
          : 'bg-neutral-700 text-neutral-300'} transition-colors"
        onclick={() => (generation.seed = randomSeed ? 0 : -1)}
      >
        Random
      </button>
    </label>
    {#if !randomSeed}
      <input
        type="number"
        bind:value={generation.seed}
        min="0"
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      />
    {:else}
      <div class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-500">
        Random each generation
      </div>
    {/if}
  </div>

  <!-- Batch Size -->
  <div>
    <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
      Batch Size
      <span class="text-neutral-300">{generation.batchSize}</span>
    </label>
    <input
      type="range"
      bind:value={generation.batchSize}
      min="1"
      max="8"
      step="1"
      class="w-full accent-indigo-500"
    />
  </div>

  <!-- Denoise (only for img2img/inpainting) -->
  {#if generation.mode !== "txt2img"}
    <div>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        Denoise Strength
        <span class="text-neutral-300">{generation.denoise.toFixed(2)}</span>
      </label>
      <input
        type="range"
        bind:value={generation.denoise}
        min="0"
        max="1"
        step="0.01"
        class="w-full accent-indigo-500"
      />
    </div>
  {/if}
</div>
