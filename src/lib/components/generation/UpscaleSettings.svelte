<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { downloadModel } from "../../utils/api.js";
  import { ipcListen } from "../../utils/ipc.js";
  import { onMount } from "svelte";
  import InfoTip from "../ui/InfoTip.svelte";
  import EditableValue from "../ui/EditableValue.svelte";
  import { scrollCapture } from "../../utils/scrollCapture.js";

  interface RecommendedModel {
    label: string;
    filename: string;
    url: string;
    description: string;
  }

  const HF_BASE = "https://huggingface.co/AshtakaOOf/safetensored-upscalers/resolve/main";

  const recommendedModels: RecommendedModel[] = [
    // SPAN — fast, sharp, excellent general-purpose upscaler
    {
      label: "SPAN 2x — Spanimation",
      filename: "2x_ModernSpanimationV1.safetensors",
      url: `${HF_BASE}/span/2x_ModernSpanimationV1.safetensors`,
      description: "Fast 2x with clean lines and vivid colours. Great for anime and illustration.",
    },
    {
      label: "SPAN 4x — NomosUni",
      filename: "4xNomosUni_span_multijpg.safetensors",
      url: `${HF_BASE}/span/4xNomosUni_span_multijpg.safetensors`,
      description: "Fast 4x all-rounder. Handles photos, art, and JPEG artifacts well.",
    },
    // OmniSR — lightweight, reliable, good balance of speed and quality
    {
      label: "OmniSR 2x",
      filename: "OmniSR_X2_DIV2K.safetensors",
      url: `${HF_BASE}/omnisr/OmniSR_X2_DIV2K.safetensors`,
      description: "Tiny model (~1.6 MB). Quick and artifact-free 2x upscale.",
    },
    {
      label: "OmniSR 3x",
      filename: "OmniSR_X3_DIV2K.safetensors",
      url: `${HF_BASE}/omnisr/OmniSR_X3_DIV2K.safetensors`,
      description: "Tiny model (~1.7 MB). Balanced 3x upscale when 2x isn't enough and 4x is too much.",
    },
    {
      label: "OmniSR 4x",
      filename: "OmniSR_X4_DIV2K.safetensors",
      url: `${HF_BASE}/omnisr/OmniSR_X4_DIV2K.safetensors`,
      description: "Tiny model (~1.7 MB). Quick 4x upscale with solid detail for its size.",
    },
    // DAT — slow but highest quality, best for final output
    {
      label: "DAT 4x — IllustrationJaNai",
      filename: "4x_IllustrationJaNai_V1_DAT2_190k.safetensors",
      url: `${HF_BASE}/dat/4x_IllustrationJaNai_V1_DAT2_190k.safetensors`,
      description: "Slow but excellent quality. Best for illustrations and anime final prints (~140 MB).",
    },
  ];

  let downloading = $state<string | null>(null);
  let downloadError = $state<string | null>(null);

  // Download progress tracking
  let dlBytes = $state(0);
  let dlTotal = $state(0);


  /** Extract scale factor from upscaler model names (e.g., "OmniSR_X4_DIV2K" → 4, "2x_Modern..." → 2) */
  function extractScaleFromModel(filename: string): number | null {
    const match =
      filename.match(/_X(\d+)[_.]/i) ||
      filename.match(/[_-](\d+)x[_.]/i) ||
      filename.match(/^(\d+)x[_A-Z]/i);
    return match ? parseInt(match[1], 10) : null;
  }

  const dlPercent = $derived(dlTotal > 0 ? Math.round((dlBytes / dlTotal) * 100) : 0);

  onMount(async () => {
    await ipcListen("download:progress", (event: any) => {
      const data = event.payload as {
        filename: string;
        downloaded: number;
        total: number;
        done: boolean;
      };
      if (data.done) {
        dlBytes = 0;
        dlTotal = 0;
      } else {
        dlBytes = data.downloaded;
        dlTotal = data.total;
      }
    });
  });

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
    
    // Auto-update scale based on model name (e.g., "OmniSR_X4_DIV2K" → 4x)
    const detectedScale = extractScaleFromModel(filename);
    if (detectedScale !== null) {
      generation.upscaleScale = detectedScale;
    }
  }
</script>

<div class="space-y-3">
  <!-- Enable toggle -->
  <div class="flex items-center justify-between">
    <label class="text-xs text-neutral-400">{locale.t('generation.upscale.title')}<InfoTip text={locale.t('generation.upscale.tip')} /></label>
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
    <!-- Method -->
    <div>
      <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.upscale.method')}<InfoTip text={locale.t('generation.upscale.method_tip')} /></label>
      <select
        bind:value={generation.upscaleMethod}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        <option value="model">{locale.t('generation.upscale.method_model_option')}</option>
        <option value="algorithmic">{locale.t('generation.upscale.method_algorithmic_option')}</option>
      </select>
    </div>

    {#if generation.upscaleMethod === "algorithmic"}
      <!-- Scale -->
      <div use:scrollCapture>
        <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          <span>{locale.t('generation.upscale.scale')}<InfoTip text={locale.t('generation.upscale.scale_tip')} /></span>
          <EditableValue value={generation.upscaleScale} min={1} max={4} step={0.5} decimals={1} suffix="x" onchange={(v) => generation.upscaleScale = v} />
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

    <!-- Upscale Model (only for model method) -->
    {#if generation.upscaleMethod === "model"}
      <div>
        <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.upscale.model')}<InfoTip text={locale.t('generation.upscale.model_tip')} /></label>
        <select
          value={generation.upscaleModel ?? ""}
          onchange={(e) => handleModelSelect((e.target as HTMLSelectElement).value)}
          disabled={downloading !== null}
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors disabled:opacity-50"
        >
          <option value="">{locale.t('generation.upscale.select_model')}</option>
          {#each getModelOptions() as opt}
            <option value={opt.value}>{opt.label}</option>
          {/each}
        </select>
        {#if generation.upscaleModel && recommendedModels.find(r => r.filename === generation.upscaleModel)?.description}
          <p class="text-[11px] text-neutral-500 mt-1">{recommendedModels.find(r => r.filename === generation.upscaleModel)!.description}</p>
        {/if}
        {#if downloading}
          <div class="mt-2 bg-neutral-800/80 rounded-lg px-3 py-2">
            <div class="flex items-center justify-between text-[11px] text-neutral-400 mb-1">
              <span class="truncate mr-2">{locale.t('generation.upscale.downloading', { model: downloading || '' })}</span>
              {#if dlTotal > 0}
                <span class="shrink-0 tabular-nums">{locale.formatBytes(dlBytes)} / {locale.formatBytes(dlTotal)} ({dlPercent}%)</span>
              {/if}
            </div>
            {#if dlTotal > 0}
              <div class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden">
                <div
                  class="bg-indigo-400 h-full rounded-full transition-[width] duration-300 ease-out"
                  style="width: {dlPercent}%"
                ></div>
              </div>
            {:else}
              <div class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden">
                <div class="bg-indigo-400 h-full rounded-full w-1/3 animate-pulse"></div>
              </div>
            {/if}
          </div>
        {/if}
        {#if downloadError}
          <p class="text-xs text-red-400 mt-1">{downloadError}</p>
        {/if}
      </div>
    {/if}

    <div class="grid grid-cols-2 gap-3">
      <!-- Denoise -->
      <div use:scrollCapture>
        <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          <span>{locale.t('generation.upscale.denoise')}<InfoTip text={locale.t('generation.upscale.denoise_tip')} /></span>
          <EditableValue value={generation.upscaleDenoise} min={0} max={1} step={0.05} decimals={2} onchange={(v) => generation.upscaleDenoise = v} />
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
      <div use:scrollCapture>
        <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          <span>{locale.t('generation.upscale.steps')}<InfoTip text={locale.t('generation.upscale.steps_tip')} /></span>
          <EditableValue value={generation.upscaleSteps} min={1} max={50} step={1} onchange={(v) => generation.upscaleSteps = v} />
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
    {#if generation.isAnima}
      <div class="flex items-center gap-2">
        <input
          type="checkbox"
          checked={true}
          disabled
          class="w-4 h-4 accent-indigo-500 rounded opacity-50"
        />
        <label class="text-xs text-neutral-400">
          {locale.t('generation.upscale.tiling_forced_label')}<InfoTip text={locale.t('generation.upscale.tiling_forced_tip')} />
        </label>
      </div>
    {:else}
      <div class="flex items-center gap-2">
        <input
          type="checkbox"
          id="upscale-tiling"
          bind:checked={generation.upscaleTiling}
          class="w-4 h-4 accent-indigo-500 rounded"
        />
        <label for="upscale-tiling" class="text-xs text-neutral-400">
          {locale.t('generation.upscale.tiling_label')}<InfoTip text={locale.t('generation.upscale.tiling_tip')} />
        </label>
      </div>
    {/if}

    <!-- Tile Size (shown when tiling enabled or forced for Anima) -->
    {#if generation.upscaleTiling || generation.isAnima}
    <div use:scrollCapture>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        <span>{locale.t('generation.upscale.tile_size')}<InfoTip text={locale.t('generation.upscale.tile_size_tip')} /></span>
        <EditableValue value={generation.upscaleTileSize} min={256} max={2048} step={64} suffix="px" onchange={(v) => generation.upscaleTileSize = v} />
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

    <!-- Soft Guidance toggle -->
    <div class="flex items-center gap-2">
      <input
        type="checkbox"
        id="upscale-soft-guidance"
        bind:checked={generation.upscaleSoftGuidance}
        class="w-4 h-4 accent-indigo-500 rounded"
      />
      <label for="upscale-soft-guidance" class="text-xs text-neutral-400">
        {locale.t('generation.upscale.soft_guidance_label')}<InfoTip text={locale.t('generation.upscale.soft_guidance_tip')} />
      </label>
    </div>

    <!-- Soft Guidance Multiplier (shown when soft guidance enabled) -->
    {#if generation.upscaleSoftGuidance}
    <div use:scrollCapture>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        <span>{locale.t('generation.upscale.soft_guidance_multiplier')}<InfoTip text={locale.t('generation.upscale.soft_guidance_multiplier_tip')} /></span>
        <EditableValue value={generation.upscaleSoftGuidanceMultiplier} min={0} max={1} step={0.05} decimals={2} onchange={(v) => generation.upscaleSoftGuidanceMultiplier = v} />
      </label>
      <input
        type="range"
        bind:value={generation.upscaleSoftGuidanceMultiplier}
        min="0"
        max="1"
        step="0.05"
        class="w-full accent-indigo-500"
      />
    </div>
    {/if}

  {/if}
</div>
