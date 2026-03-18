<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import InfoTip from "../ui/InfoTip.svelte";

  let randomSeed = $derived(generation.seed === -1);

  /** CFG++ samplers use an alternative guidance method that works best at low CFG (~1-2). */
  function isCfgPpSampler(name: string): boolean {
    return name.includes("cfg_pp");
  }

  function onSamplerChange() {
    if (isCfgPpSampler(generation.samplerName) && generation.cfg > 5) {
      generation.cfg = 1.4;
    }
  }
</script>

<div class="space-y-3">
  <!-- Sampler -->
  <div>
    <label class="block text-xs text-neutral-400 mb-1">Sampler<InfoTip text="The algorithm used to progressively remove noise from the image. Different samplers produce different results — 'euler' is fast and reliable, 'dpmpp' variants offer higher quality, 'ancestral' ones add randomness for variety." /></label>
    <select
      bind:value={generation.samplerName}
      onchange={onSamplerChange}
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
    >
      {#each models.samplers as s}
        <option value={s}>{s}</option>
      {/each}
    </select>
  </div>

  <!-- Scheduler -->
  <div>
    <label class="block text-xs text-neutral-400 mb-1">Scheduler<InfoTip text="Controls how noise is distributed across steps. 'normal' is standard, 'karras' front-loads detail work for sharper results, 'sgm_uniform' spaces steps evenly." /></label>
    <select
      bind:value={generation.scheduler}
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
    >
      {#each models.schedulers as s}
        <option value={s}>{s}</option>
      {/each}
    </select>
  </div>

  <!-- Steps -->
  <div>
    <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
      Steps<InfoTip text="How many denoising iterations to run. More steps = finer detail but slower. 20-30 is a good balance for most samplers. Some (like 'euler') converge fast and don't benefit much beyond 25." />
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
    <label class="block text-xs text-neutral-400 mb-1">CFG Scale<InfoTip text="Classifier-Free Guidance — how closely the AI follows your prompt. Higher = more literal but can look artificial. Lower = more creative but may ignore parts of your prompt. CFG++ samplers work best around 1-2." /></label>
    <input
      type="number"
      bind:value={generation.cfg}
      min="0"
      max="30"
      step="0.1"
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors mb-1"
    />
    <input
      type="range"
      bind:value={generation.cfg}
      min="0"
      max="30"
      step="0.1"
      class="w-full accent-indigo-500"
    />
  </div>

  <!-- Seed -->
  <div>
    <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
      Seed<InfoTip text="A number that determines the 'randomness' of your image. Same seed + same settings = same image. Use 'Random' for variety, or set a specific seed to reproduce or iterate on a result." />
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
      Batch Size<InfoTip text="How many images to generate at once. Higher values use more VRAM but let you compare results quickly. Each image uses the same prompt but a different seed." />
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

</div>
