<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { downloadModel } from "../../utils/api.js";

  interface RecommendedModel {
    label: string;
    filename: string;
    url: string;
  }

  const recommendedModels: RecommendedModel[] = [
    {
      label: "Omni 2x (Recommended)",
      filename: "OmniSR_X2_DIV2K.safetensors",
      url: "https://huggingface.co/Acly/Omni-SR/resolve/main/OmniSR_X2_DIV2K.safetensors",
    },
    {
      label: "Omni 4x (Recommended)",
      filename: "OmniSR_X4_DIV2K.safetensors",
      url: "https://huggingface.co/Acly/Omni-SR/resolve/main/OmniSR_X4_DIV2K.safetensors",
    },
  ];

  let downloading = $state<string | null>(null);
  let downloadError = $state<string | null>(null);

  // All options: installed models + recommended that aren't installed yet
  function getModelOptions() {
    const installed = models.upscaleModels;
    const options: { value: string; label: string; needsDownload: boolean }[] = [];

    // Add recommended models first
    for (const rec of recommendedModels) {
      const isInstalled = installed.includes(rec.filename);
      options.push({
        value: rec.filename,
        label: isInstalled ? rec.label : `⬇ ${rec.label}`,
        needsDownload: !isInstalled,
      });
    }

    // Add other installed models
    for (const m of installed) {
      if (!recommendedModels.some((r) => r.filename === m)) {
        options.push({ value: m, label: m, needsDownload: false });
      }
    }

    return options;
  }

  async function handleModelSelect(filename: string) {
    const rec = recommendedModels.find((r) => r.filename === filename);
    const isInstalled = models.upscaleModels.includes(filename);

    if (rec && !isInstalled) {
      downloading = filename;
      downloadError = null;
      try {
        await downloadModel(rec.url, "upscale_models", rec.filename);
        // Refresh models list so it shows as installed
        await models.refresh();
      } catch (e) {
        downloadError = `Download failed: ${e}`;
        generation.upscaleModel = null;
        return;
      } finally {
        downloading = null;
      }
    }

    generation.upscaleModel = filename;
  }
</script>

<div class="space-y-3">
  <!-- Enable toggle -->
  <div class="flex items-center justify-between">
    <label class="text-xs text-neutral-400">Upscale</label>
    <button
      class="relative w-10 h-5 rounded-full transition-colors {generation.upscaleEnabled
        ? 'bg-indigo-600'
        : 'bg-neutral-700'}"
      onclick={() => (generation.upscaleEnabled = !generation.upscaleEnabled)}
      role="switch"
      aria-checked={generation.upscaleEnabled}
    >
      <span
        class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform {generation.upscaleEnabled
          ? 'translate-x-5'
          : ''}"
      ></span>
    </button>
  </div>

  {#if generation.upscaleEnabled}
    <!-- Method + Scale (scale only for algorithmic) -->
    <div class="grid grid-cols-2 gap-3">
      <div>
        <label class="block text-xs text-neutral-400 mb-1">Method</label>
        <select
          bind:value={generation.upscaleMethod}
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
        >
          <option value="model">Model (ESRGAN)</option>
          <option value="algorithmic">Algorithmic</option>
        </select>
      </div>

      {#if generation.upscaleMethod === "algorithmic"}
        <div>
          <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
            Scale
            <span class="text-neutral-300">{generation.upscaleScale}x</span>
          </label>
          <input
            type="range"
            bind:value={generation.upscaleScale}
            min="1"
            max="4"
            step="0.5"
            class="w-full accent-indigo-500"
          />
        </div>
      {/if}
    </div>

    <!-- Upscale Model (only for model method) -->
    {#if generation.upscaleMethod === "model"}
      <div>
        <label class="block text-xs text-neutral-400 mb-1">Upscale Model</label>
        <select
          value={generation.upscaleModel ?? ""}
          onchange={(e) => handleModelSelect((e.target as HTMLSelectElement).value)}
          disabled={downloading !== null}
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors disabled:opacity-50"
        >
          <option value="">Select model...</option>
          {#each getModelOptions() as opt}
            <option value={opt.value}>{opt.label}</option>
          {/each}
        </select>
        {#if downloading}
          <p class="text-xs text-indigo-400 mt-1 animate-pulse">Downloading {downloading}...</p>
        {/if}
        {#if downloadError}
          <p class="text-xs text-red-400 mt-1">{downloadError}</p>
        {/if}
      </div>
    {/if}

    <div class="grid grid-cols-2 gap-3">
      <!-- Denoise -->
      <div>
        <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          Denoise
          <span class="text-neutral-300">{generation.upscaleDenoise.toFixed(2)}</span>
        </label>
        <input
          type="range"
          bind:value={generation.upscaleDenoise}
          min="0"
          max="1"
          step="0.05"
          class="w-full accent-indigo-500"
        />
      </div>

      <!-- Steps -->
      <div>
        <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          Steps
          <span class="text-neutral-300">{generation.upscaleSteps}</span>
        </label>
        <input
          type="range"
          bind:value={generation.upscaleSteps}
          min="1"
          max="50"
          step="1"
          class="w-full accent-indigo-500"
        />
      </div>
    </div>

    <!-- Tiling toggle -->
    <div class="flex items-center gap-2">
      <input
        type="checkbox"
        id="upscale-tiling"
        bind:checked={generation.upscaleTiling}
        class="w-4 h-4 accent-indigo-500 rounded"
      />
      <label for="upscale-tiling" class="text-xs text-neutral-400">
        Tiled diffusion (recommended for large images)
      </label>
    </div>

    <!-- Tile Size (only when tiling enabled) -->
    {#if generation.upscaleTiling}
    <div>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        Tile Size
        <span class="text-neutral-300">{generation.upscaleTileSize}px</span>
      </label>
      <input
        type="range"
        bind:value={generation.upscaleTileSize}
        min="256"
        max="2048"
        step="64"
        class="w-full accent-indigo-500"
      />
    </div>
    {/if}
  {/if}
</div>
