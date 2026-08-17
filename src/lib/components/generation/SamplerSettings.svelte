<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import InfoTip from "../ui/InfoTip.svelte";
  import EditableValue from "../ui/EditableValue.svelte";
  import { scrollCapture } from "../../utils/scrollCapture.js";
  import { downloadModel } from "../../utils/api.js";
  import { ipcListen } from "../../utils/ipc.js";
  import { onMount } from "svelte";

  const RDBT_ANIMA_FILENAME = "rdbt_v1.0_anima_b1_16-step.safetensors";
  const RDBT_ANIMA_URL =
    "https://huggingface.co/Kutches/Anim4/resolve/main/rdbt_v1.0_anima_b1_16-step.safetensors";

  let rdbtDownloading = $state(false);
  let rdbtDownloadError = $state<string | null>(null);
  let rdbtDlBytes = $state(0);
  let rdbtDlTotal = $state(0);
  const rdbtDlPercent = $derived(rdbtDlTotal > 0 ? Math.round((rdbtDlBytes / rdbtDlTotal) * 100) : 0);
  const rdbtEnabled = $derived(
    generation.loras.some((l) => l.name === RDBT_ANIMA_FILENAME && l.enabled)
  );

  onMount(async () => {
    await ipcListen("download:progress", (event: any) => {
      const data = event.payload as { filename: string; downloaded: number; total: number; done: boolean };
      if (data.filename !== RDBT_ANIMA_FILENAME) return;
      if (data.done) {
        rdbtDlBytes = 0;
        rdbtDlTotal = 0;
      } else {
        rdbtDlBytes = data.downloaded;
        rdbtDlTotal = data.total;
      }
    });
  });

  async function toggleRdbtAnima() {
    const idx = generation.loras.findIndex((l) => l.name === RDBT_ANIMA_FILENAME);

    if (rdbtEnabled) {
      if (idx >= 0) generation.toggleLora(idx);
      return;
    }

    if (!models.loras.includes(RDBT_ANIMA_FILENAME)) {
      rdbtDownloading = true;
      rdbtDownloadError = null;
      try {
        await downloadModel(RDBT_ANIMA_URL, "loras", RDBT_ANIMA_FILENAME);
        await models.refresh();
      } catch (e) {
        rdbtDownloadError = `${e}`;
        rdbtDownloading = false;
        return;
      }
      rdbtDownloading = false;
    }

    if (idx >= 0) {
      generation.toggleLora(idx);
    } else {
      generation.loras = [
        ...generation.loras,
        { name: RDBT_ANIMA_FILENAME, strength_model: 1.0, strength_clip: 1.0, enabled: true },
      ];
    }
  }

  let randomSeed = $derived(generation.seed === "-1");
  const activeModelName = $derived((generation.diffusionModel || generation.checkpoint || "").toLowerCase());
  const hasAnimaRecommendation = $derived(generation.isAnima || activeModelName.includes("anima"));
  const hasJuiceRecommendation = $derived(
    activeModelName.includes("juice") ||
      activeModelName.includes("seele") ||
      activeModelName.includes("sih") ||
      activeModelName.includes("σih"),
  );
  const hasNanosaurRecommendation = $derived(generation.isNanosaur || activeModelName.includes("nanosaur"));

  let animaRecOpen = $state(true);
  let juiceRecOpen = $state(true);
  let nanosaurRecOpen = $state(true);

  function recommendedStepRange() {
    const sampler = generation.samplerName.toLowerCase();
    if (sampler.includes("euler")) return { min: 18, max: 28 };
    if (sampler.includes("dpmpp")) return { min: 24, max: 36 };
    return { min: 20, max: 30 };
  }

  function recommendedCfgRange() {
    if (isCfgPpSampler(generation.samplerName)) return { min: 1.5, max: 2.2, target: 1.8 };
    return { min: 4.0, max: 8.0, target: 6.0 };
  }

  const stepsOutOfRange = $derived(
    generation.steps < recommendedStepRange().min || generation.steps > recommendedStepRange().max
  );

  const cfgOutOfRange = $derived(
    generation.cfg < recommendedCfgRange().min || generation.cfg > recommendedCfgRange().max
  );

  const metadataUpgradedToBoth = $derived(
    generation.outputBitDepth === "16bit" && generation.metadataMode === "stealth"
  );

  const effectiveMetadataMode = $derived(
    metadataUpgradedToBoth ? "both" : generation.metadataMode
  );

  /** CFG++ samplers use an alternative guidance method that works best at low CFG (~1-2). */
  function isCfgPpSampler(name: string): boolean {
    return name.includes("cfg_pp");
  }

  function onSamplerChange() {
    if (isCfgPpSampler(generation.samplerName) && generation.cfg > 5) {
      generation.cfg = recommendedCfgRange().target;
    }
  }

  function applyRecommendedSamplerTuning() {
    const stepRange = recommendedStepRange();
    const cfgRange = recommendedCfgRange();
    generation.steps = Math.round((stepRange.min + stepRange.max) / 2);
    generation.cfg = cfgRange.target;
  }

  function applyAnimaRecommendation() {
    generation.steps = 30;
    generation.cfg = 4.0;
    generation.samplerName = "er_sde";
    generation.scheduler = "sgm_uniform";
    // Face fix and upscale steps are 1/3 of main steps
    generation.facefixSteps = Math.ceil(30 / 3);
    generation.upscaleSteps = Math.ceil(30 / 3);
  }

  function applyJuiceRecommendation() {
    generation.steps = 20;
    generation.cfg = 1.4;
    generation.samplerName = "euler_cfg_pp";
    generation.scheduler = "sgm_uniform";
    // Face fix and upscale steps are 1/3 of main steps
    generation.facefixSteps = Math.ceil(20 / 3);
    generation.upscaleSteps = Math.ceil(20 / 3);
  }

  function applyNanosaurRecommendation() {
    generation.steps = 40;
    generation.cfg = 7;
    generation.samplerName = "euler";
    generation.scheduler = "simple";
    generation.upscaleSteps = 20;
    generation.upscaleDenoise = 0.5;
  }
</script>

<div class="space-y-3">
  {#if hasAnimaRecommendation}
    <div class="rounded-lg border border-indigo-700/50 bg-indigo-900/15 overflow-hidden">
      <button
        class="w-full flex items-center justify-between px-2.5 py-2 text-left"
        onclick={() => (animaRecOpen = !animaRecOpen)}
      >
        <p class="text-xs text-indigo-300 font-medium">{locale.t('generation.sampler.anima_recommended')}</p>
        <svg class="w-3 h-3 text-indigo-400 shrink-0 transition-transform {animaRecOpen ? '' : '-rotate-90'}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
      </button>
      {#if animaRecOpen}
        <div class="flex items-start justify-between gap-2 px-2.5 pb-2.5">
          <p class="text-[11px] text-neutral-300">{locale.t('generation.sampler.anima_hint')}</p>
          <button
            class="shrink-0 px-2 py-1 text-[10px] rounded border border-indigo-500/70 text-indigo-200 hover:border-indigo-400 hover:text-indigo-100 transition-colors"
            onclick={applyAnimaRecommendation}
          >
            {locale.t('common.apply')}
          </button>
        </div>
        <div class="flex items-center justify-between gap-2 px-2.5 pb-2.5">
          <label class="text-[11px] text-neutral-300">
            {locale.t('generation.sampler.anima_rdbt_toggle')}<InfoTip text={locale.t('generation.sampler.anima_rdbt_tip')} />
          </label>
          <button
            class="relative w-10 h-5 rounded-full transition-colors shrink-0 {rdbtEnabled
              ? 'bg-indigo-600'
              : 'bg-neutral-700'}"
            onclick={toggleRdbtAnima}
            disabled={rdbtDownloading}
            role="switch"
            aria-checked={rdbtEnabled}
            aria-label={locale.t('generation.sampler.anima_rdbt_toggle')}
          >
            <span
              class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform {rdbtEnabled
                ? 'translate-x-5'
                : ''}"
            ></span>
          </button>
        </div>
        {#if rdbtDownloading}
          <div class="mx-2.5 mb-2.5 bg-neutral-900/60 rounded-lg px-3 py-2">
            <div class="flex items-center justify-between text-[11px] text-neutral-400 mb-1">
              <span class="truncate mr-2">{locale.t('generation.sampler.anima_rdbt_downloading')}</span>
              {#if rdbtDlTotal > 0}
                <span class="shrink-0 tabular-nums">{locale.formatBytes(rdbtDlBytes)} / {locale.formatBytes(rdbtDlTotal)} ({rdbtDlPercent}%)</span>
              {/if}
            </div>
            <div class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden">
              {#if rdbtDlTotal > 0}
                <div class="bg-indigo-400 h-full rounded-full transition-[width] duration-300 ease-out" style="width: {rdbtDlPercent}%"></div>
              {:else}
                <div class="bg-indigo-400 h-full rounded-full w-1/3 animate-pulse"></div>
              {/if}
            </div>
          </div>
        {/if}
        {#if rdbtDownloadError}
          <p class="text-xs text-red-400 mx-2.5 mb-2.5">{rdbtDownloadError}</p>
        {/if}
      {/if}
    </div>
  {/if}

  {#if hasJuiceRecommendation}
    <div class="rounded-lg border border-neutral-700 bg-neutral-900/60 overflow-hidden">
      <button
        class="w-full flex items-center justify-between px-2.5 py-2 text-left"
        onclick={() => (juiceRecOpen = !juiceRecOpen)}
      >
        <p class="text-xs text-neutral-300 font-medium">{locale.t('generation.sampler.juice_recommended')}</p>
        <svg class="w-3 h-3 text-neutral-500 shrink-0 transition-transform {juiceRecOpen ? '' : '-rotate-90'}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
      </button>
      {#if juiceRecOpen}
        <div class="flex items-start justify-between gap-2 px-2.5 pb-2.5">
          <p class="text-[11px] text-neutral-400">{locale.t('generation.sampler.juice_hint')}</p>
          <button
            class="shrink-0 px-2 py-1 text-[10px] rounded border border-neutral-600 text-neutral-300 hover:border-neutral-500 hover:text-neutral-200 transition-colors"
            onclick={applyJuiceRecommendation}
          >
            {locale.t('common.apply')}
          </button>
        </div>
      {/if}
    </div>
  {/if}

  {#if hasNanosaurRecommendation}
    <div class="rounded-lg border border-emerald-700/50 bg-emerald-900/15 overflow-hidden">
      <button
        class="w-full flex items-center justify-between px-2.5 py-2 text-left"
        onclick={() => (nanosaurRecOpen = !nanosaurRecOpen)}
      >
        <p class="text-xs text-emerald-300 font-medium">{locale.t('generation.sampler.nanosaur_recommended')}</p>
        <svg class="w-3 h-3 text-emerald-500 shrink-0 transition-transform {nanosaurRecOpen ? '' : '-rotate-90'}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
      </button>
      {#if nanosaurRecOpen}
        <div class="flex items-start justify-between gap-2 px-2.5 pb-2.5">
          <p class="text-[11px] text-neutral-300">{locale.t('generation.sampler.nanosaur_hint')}</p>
          <button
            class="shrink-0 px-2 py-1 text-[10px] rounded border border-emerald-500/70 text-emerald-200 hover:border-emerald-400 hover:text-emerald-100 transition-colors"
            onclick={applyNanosaurRecommendation}
          >
            {locale.t('common.apply')}
          </button>
        </div>
      {/if}
    </div>
  {/if}

  <!-- Sampler + Scheduler -->
  <div class="grid grid-cols-2 gap-2">
    <div>
      <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.sampler.label')}<InfoTip text={locale.t('generation.sampler.label_tip')} /></label>
      <select
        bind:value={generation.samplerName}
        onchange={onSamplerChange}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        {#each models.samplers as s}
          <option value={s}>{s}</option>
        {/each}
      </select>
    </div>
    <div>
      <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.sampler.scheduler')}<InfoTip text={locale.t('generation.sampler.scheduler_tip')} /></label>
      <select
        bind:value={generation.scheduler}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        {#each models.schedulers as s}
          <option value={s}>{s}</option>
        {/each}
      </select>
    </div>
  </div>

  <!-- Steps + CFG side-by-side -->
  <div class="grid grid-cols-2 gap-2">
    <div use:scrollCapture>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        <span>{locale.t('generation.sampler.steps')}<InfoTip text={locale.t('generation.sampler.steps_tip')} /></span>
        <EditableValue value={generation.steps} min={1} max={150} step={1} onchange={(v) => generation.steps = v} />
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
    <div use:scrollCapture>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        <span>{locale.t('generation.sampler.cfg')}<InfoTip text={locale.t('generation.sampler.cfg_tip')} /></span>
        <EditableValue value={generation.cfg} min={0} max={30} step={0.1} decimals={1} onchange={(v) => generation.cfg = v} />
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
  <!-- Recommendations row -->
  <div class="flex items-center justify-between gap-2 -mt-1">
    <span class="text-[10px] {stepsOutOfRange ? 'text-amber-400' : 'text-neutral-500'} truncate">
      Steps: {recommendedStepRange().min}-{recommendedStepRange().max}
    </span>
    <span class="text-[10px] {cfgOutOfRange ? 'text-amber-400' : 'text-neutral-500'} truncate">
      CFG: {locale.formatDecimal(recommendedCfgRange().min, 1)}-{locale.formatDecimal(recommendedCfgRange().max, 1)}
    </span>
    {#if cfgOutOfRange || stepsOutOfRange}
      <button
        class="text-[10px] px-2 py-0.5 rounded border border-amber-700/70 text-amber-300 hover:border-amber-500 hover:text-amber-200 transition-colors shrink-0"
        onclick={applyRecommendedSamplerTuning}
      >
        {locale.t('generation.sampler.fix')}
      </button>
    {/if}
  </div>

  {#if generation.cfg <= 1.0}
    <div class="flex items-start gap-2 rounded-lg border border-amber-600/60 bg-amber-950/30 px-3 py-2 mt-1">
      <svg xmlns="http://www.w3.org/2000/svg" aria-hidden="true" class="w-4 h-4 mt-0.5 shrink-0 text-amber-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z" />
        <path d="M12 9v4" />
        <path d="M12 17h.01" />
      </svg>
      <div>
        <p class="text-xs font-semibold text-amber-200">{locale.t('generation.sampler.cfg1_warning_title')}</p>
        <p class="text-[10px] text-amber-300/80 mt-0.5">{locale.t('generation.sampler.cfg1_warning_body')}</p>
      </div>
    </div>
  {/if}

  <!-- Seed + Batch Size -->
  <div class="grid grid-cols-2 gap-2">
    <div>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        <span>{locale.t('generation.sampler.seed')}<InfoTip text={locale.t('generation.sampler.seed_tip')} /></span>
        <div class="flex items-center gap-1">
          {#if progress.lastCompletedSeed != null}
            <button
              class="text-[10px] px-1.5 py-0.5 rounded bg-neutral-700 text-neutral-300 hover:bg-neutral-600 transition-colors"
              onclick={() => (generation.seed = progress.lastCompletedSeed!)}
              title={locale.t('generation.sampler.seed_use_last_tip')}
            >
              {locale.t('generation.sampler.seed_use_last')}
            </button>
          {/if}
          <button
            class="text-[10px] px-1.5 py-0.5 rounded {randomSeed
              ? 'bg-indigo-600 text-white'
              : 'bg-neutral-700 text-neutral-300'} transition-colors"
            onclick={() => (generation.seed = randomSeed ? (progress.lastCompletedSeed ?? "0") : "-1")}
          >
            {locale.t('generation.sampler.seed_random')}
          </button>
        </div>
      </label>
      <!-- Single editable box: shows "Random" as a placeholder when RNG is on, and
           typing a value switches off RNG automatically — no need to toggle first (#394).
           type="text": a number input would round 63-bit seeds past 2^53. -->
      <input
        type="text"
        inputmode="numeric"
        value={randomSeed ? '' : generation.seed}
        placeholder={locale.t('generation.sampler.random_display')}
        oninput={(e) => {
          const digits = e.currentTarget.value.replace(/\D/g, '');
          if (e.currentTarget.value !== digits) e.currentTarget.value = digits;
          generation.seed = digits === '' ? "-1" : digits;
        }}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      />
    </div>
    <div use:scrollCapture>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        <span>{locale.t('generation.sampler.batch')}<InfoTip text={locale.t('generation.sampler.batch_tip')} /></span>
        <EditableValue value={generation.batchSize} min={1} max={8} step={1} onchange={(v) => generation.batchSize = v} />
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

  <!-- Bit Depth + Metadata -->
  <div class="grid grid-cols-2 gap-2">
    <div>
      <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.sampler.bit_depth')}<InfoTip text={locale.t('generation.sampler.bit_depth_tip')} /></label>
      <div class="flex gap-1">
        {#each ["8bit", "16bit"] as depth}
          {@const depthLocked = depth === "16bit" && generation.outputFormat === "webp"}
          <button
            disabled={depthLocked}
            title={depthLocked ? locale.t('generation.sampler.bit_16_webp_disabled') : undefined}
            class="flex-1 py-1 text-[11px] rounded-lg border transition-colors {generation.outputBitDepth === depth
              ? 'bg-indigo-600/30 border-indigo-500 text-indigo-300'
              : 'bg-neutral-800/50 border-neutral-700 text-neutral-400 hover:border-neutral-600'} {depthLocked ? 'opacity-40 cursor-not-allowed hover:border-neutral-700' : ''}"
            onclick={() => generation.outputBitDepth = depth as "8bit" | "16bit"}
          >
            {depth === "8bit" ? locale.t('generation.sampler.bit_8') : locale.t('generation.sampler.bit_16')}
          </button>
        {/each}
      </div>
    </div>
    <div>
      <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.sampler.output_format')}<InfoTip text={locale.t('generation.sampler.output_format_tip')} /></label>
      <div class="flex gap-1">
        {#each ["png", "jxl", "webp"] as fmt}
          <button
            class="flex-1 py-1 text-[11px] rounded-lg border transition-colors {generation.outputFormat === fmt
              ? 'bg-indigo-600/30 border-indigo-500 text-indigo-300'
              : 'bg-neutral-800/50 border-neutral-700 text-neutral-400 hover:border-neutral-600'}"
            onclick={() => {
              generation.outputFormat = fmt as "png" | "jxl" | "webp";
              // Lossless WebP (VP8L) has no 16-bit variant, so keep the pair valid.
              if (fmt === "webp") generation.outputBitDepth = "8bit";
            }}
          >
            {fmt === "png"
              ? locale.t('generation.sampler.format_png')
              : fmt === "jxl"
                ? locale.t('generation.sampler.format_jxl')
                : locale.t('generation.sampler.format_webp')}
          </button>
        {/each}
      </div>
    </div>
  </div>
  <div class="grid grid-cols-1 gap-2">
    <div>
      <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.sampler.metadata')}<InfoTip text={locale.t('generation.sampler.metadata_tip')} /></label>
      <div class="flex gap-1">
        {#each [["text_chunk", locale.t('generation.sampler.metadata_text')], ["stealth", locale.t('generation.sampler.metadata_stealth')], ["both", locale.t('generation.sampler.metadata_both')]] as [value, label]}
          <button
            class="flex-1 py-1 text-[11px] rounded-lg border transition-colors {effectiveMetadataMode === value
              ? 'bg-indigo-600/30 border-indigo-500 text-indigo-300'
              : 'bg-neutral-800/50 border-neutral-700 text-neutral-400 hover:border-neutral-600'}"
            onclick={() => generation.metadataMode = value as "text_chunk" | "stealth" | "both"}
          >
            {label}
          </button>
        {/each}
      </div>
    </div>
  </div>
  {#if metadataUpgradedToBoth}
    <p class="text-[10px] text-indigo-300 -mt-1">
      {locale.t('generation.sampler.metadata_upgraded')}
    </p>
  {/if}

  <!-- Smart Guidance toggle (hidden for Flux models which use FluxGuidance instead) -->
  {#if generation.isFlux}
    <div use:scrollCapture>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        <span>{locale.t('generation.sampler.flux_guidance_label')}<InfoTip text={locale.t('generation.sampler.flux_guidance_tip')} /></span>
        <EditableValue value={generation.fluxGuidance} min={0} max={10} step={0.1} decimals={1} onchange={(v) => generation.fluxGuidance = v} />
      </label>
      <input
        type="range"
        bind:value={generation.fluxGuidance}
        min="0"
        max="10"
        step="0.1"
        class="w-full accent-indigo-500"
      />
    </div>
  {:else}
    <div class="flex items-center gap-2">
      <input
        type="checkbox"
        id="smart-guidance"
        bind:checked={generation.smartGuidance}
        class="w-4 h-4 accent-indigo-500 rounded"
      />
      <label for="smart-guidance" class="text-xs text-neutral-400">
        {locale.t('generation.sampler.smart_guidance_label')}<InfoTip text={locale.t('generation.sampler.smart_guidance_tip')} />
      </label>
    </div>
  {/if}

  {#if generation.isAnima}
    <div class="flex items-center gap-2">
      <input
        type="checkbox"
        id="anima-teacache"
        bind:checked={generation.animaTeacacheEnabled}
        onchange={() => generation.saveSettings()}
        class="w-4 h-4 accent-indigo-500 rounded"
      />
      <label for="anima-teacache" class="text-xs text-neutral-400">
        {locale.t('generation.sampler.anima_teacache_label')}<InfoTip text={locale.t('generation.sampler.anima_teacache_tip')} />
      </label>
    </div>
  {/if}

</div>
