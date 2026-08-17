<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import InfoTip from "../ui/InfoTip.svelte";
  import type { ModelFamily } from "../../utils/modelFamily.js";

  interface Props {
    suggestedAspect?: { w: number; h: number } | null;
  }
  let { suggestedAspect = null }: Props = $props();

  const AR_COLLAPSE_KEY = "mooshieui.generation.aspectRatioCollapsed.v1";
  let arOpen = $state(localStorage.getItem(AR_COLLAPSE_KEY) !== "true");
  let arSaveTimer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    const collapsed = String(!arOpen);
    if (arSaveTimer) clearTimeout(arSaveTimer);
    arSaveTimer = setTimeout(() => {
      try { localStorage.setItem(AR_COLLAPSE_KEY, collapsed); } catch {}
    }, 300);
  });

  let aspectW = $state(1);
  let aspectH = $state(1);
  let sideLength = $state(1024);
  let aspectWInput = $state("1");
  let aspectHInput = $state("1");
  let lastSyncedDimensions = "";

  /** Try to match persisted width/height back to a preset or simplified ratio. */
  /** Compute dimensions for a given aspect ratio using the area-faithful formula. */
  function dimsForAspect(aw: number, ah: number, side: number): { w: number; h: number } {
    const area = side * side;
    const wA = Math.round(Math.sqrt(area * (aw / ah)) / 8) * 8;
    const hA = Math.max(8, Math.round(area / wA / 8) * 8);
    const hB = Math.round(Math.sqrt(area * (ah / aw)) / 8) * 8;
    const wB = Math.max(8, Math.round(area / hB / 8) * 8);
    return Math.abs(wA * hA - area) <= Math.abs(wB * hB - area)
      ? { w: wA, h: hA }
      : { w: wB, h: hB };
  }

  function inferAspectFromDimensions(w: number, h: number) {
    // Check presets first (exact match on resulting dimensions)
    for (const p of presets) {
      const dims = dimsForAspect(p.w, p.h, sideLength);
      if (dims.w === w && dims.h === h) {
        return { w: p.w, h: p.h };
      }
    }
    // Fallback: reduce to simplest ratio via GCD
    const gcd = (a: number, b: number): number => (b === 0 ? a : gcd(b, a % b));
    const d = gcd(w, h);
    return { w: w / d, h: h / d };
  }

  // Sync aspect ratio UI from generation dimensions (including async settings load)
  $effect(() => {
    const w = generation.width;
    const h = generation.height;
    if (w && h) {
      const key = `${w}x${h}`;
      if (key === lastSyncedDimensions) return;
      lastSyncedDimensions = key;

      const inferred = inferAspectFromDimensions(w, h);
      aspectW = inferred.w;
      aspectH = inferred.h;
      aspectWInput = String(inferred.w);
      aspectHInput = String(inferred.h);

      // Keep side-length control aligned with the current generated area.
      sideLength = Math.max(64, Math.round(Math.sqrt(w * h) / 8) * 8);
    }
  });

  // When an input image is loaded, adopt its aspect ratio
  let lastAppliedKey = "";
  $effect(() => {
    if (suggestedAspect) {
      const key = `${suggestedAspect.w}:${suggestedAspect.h}`;
      if (key !== lastAppliedKey) {
        lastAppliedKey = key;
        aspectW = suggestedAspect.w;
        aspectH = suggestedAspect.h;
        aspectWInput = String(suggestedAspect.w);
        aspectHInput = String(suggestedAspect.h);
      }
    }
  });

  const presets = [
    { label: "1:1", w: 1, h: 1 },
    { label: "4:3", w: 4, h: 3 },
    { label: "3:2", w: 3, h: 2 },
    { label: "16:9", w: 16, h: 9 },
    { label: "21:9", w: 21, h: 9 },
    { label: "3:4", w: 3, h: 4 },
    { label: "2:3", w: 2, h: 3 },
    { label: "9:16", w: 9, h: 16 },
  ];

  function recalc() {
    const dims = dimsForAspect(
      Math.max(0.01, aspectW),
      Math.max(0.01, aspectH),
      Math.max(64, sideLength),
    );
    generation.width = dims.w;
    generation.height = dims.h;
    // Mark these dimensions as already synced so the $effect doesn't
    // re-infer the aspect ratio and overwrite the user's input.
    lastSyncedDimensions = `${dims.w}x${dims.h}`;
  }

  function applyPreset(w: number, h: number) {
    aspectW = w;
    aspectH = h;
    aspectWInput = String(w);
    aspectHInput = String(h);
    recalc();
  }

  function swapAspect() {
    const tmp = aspectW;
    aspectW = aspectH;
    aspectH = tmp;
    aspectWInput = String(aspectW);
    aspectHInput = String(aspectH);
    recalc();
  }

  function onAspectInput(kind: "w" | "h", value: string) {
    if (kind === "w") {
      aspectWInput = value;
      const parsed = Number.parseFloat(value);
      if (!Number.isNaN(parsed) && Number.isFinite(parsed) && parsed > 0) {
        aspectW = parsed;
        recalc();
      }
      return;
    }

    aspectHInput = value;
    const parsed = Number.parseFloat(value);
    if (!Number.isNaN(parsed) && Number.isFinite(parsed) && parsed > 0) {
      aspectH = parsed;
      recalc();
    }
  }

  const activePreset = $derived(
    presets.find((p) => p.w === aspectW && p.h === aspectH)?.label ?? ""
  );

  const DEFAULT_SIDE = 1024;
  const sidePresets = [512, 768, 1024, 1536, 2048];

  function applySideLength(side: number) {
    sideLength = side;
    recalc();
  }

  const FAMILY_LABELS: Partial<Record<ModelFamily, string>> = {
    anima: "Anima",
    sdxl: "SDXL",
    illustrious: "Illustrious",
    pony: "Pony",
    sd15: "SD 1.5",
    sd3: "SD3",
    flux: "Flux",
    flux1d: "Flux.1 Dev",
    flux1s: "Flux.1 Schnell",
    flux1krea: "Flux.1 Krea",
    flux2d: "Flux.2 Dev",
    flux2klein9b: "Flux.2 Klein 9B",
    flux2klein9bbase: "Flux.2 Klein 9B Base",
    flux2klein4b: "Flux.2 Klein 4B",
    flux2klein4bbase: "Flux.2 Klein 4B Base",
    chroma: "Chroma",
    zib: "Z-Image Base",
    zit: "Z-Image Turbo",
    wan: "Wan",
    qwen: "Qwen",
    ideogram4: "Ideogram 4.0",
    krea2: "Krea 2",
    auraflow: "AuraFlow",
    pixart: "PixArt",
    hunyuandit: "HunyuanDiT",
    cascade: "Stable Cascade",
    kolors: "Kolors",
    mugen: "Mugen",
    nanosaur: "Nanosaur",
  };

  /** Recommended initial-generation side-length range per model family. */
  function recommendedRange(family: ModelFamily): { min: number; max: number } | null {
    switch (family) {
      case "unknown":
        return null;
      case "sd15":
        return { min: 512, max: 768 };
      case "flux2d":
      case "flux2klein9b":
      case "flux2klein9bbase":
      case "flux2klein4b":
      case "flux2klein4bbase":
        return { min: 1024, max: 2048 };
      case "qwen":
        return { min: 1024, max: 1536 };
      default:
        return { min: 1024, max: 1024 };
    }
  }

  const recommended = $derived(recommendedRange(generation.modelFamily));
  const recommendedLabel = $derived(
    recommended
      ? recommended.min === recommended.max
        ? String(recommended.min)
        : `${recommended.min}–${recommended.max}`
      : ""
  );
  const familyLabel = $derived(FAMILY_LABELS[generation.modelFamily] ?? "");

  /** Fit a w:h ratio into a max bounding box for preset preview chips. */
  function aspectPreviewSize(w: number, h: number, boxPx = 12): { w: number; h: number } {
    const ratio = w / h;
    if (ratio >= 1) {
      return { w: boxPx, h: Math.max(3, Math.round(boxPx / ratio)) };
    }
    return { w: Math.max(3, Math.round(boxPx * ratio)), h: boxPx };
  }
</script>

<div class="space-y-3">
  <!-- Aspect Ratio -->
  <div>
    <div class="flex items-center mb-1.5">
      <button
        class="flex items-center text-xs text-neutral-400 hover:text-neutral-200 focus:outline-none"
        onclick={() => (arOpen = !arOpen)}
        title={arOpen ? locale.t('common.collapse', { section: locale.t('generation.dimensions.aspect_ratio') }) : locale.t('common.expand', { section: locale.t('generation.dimensions.aspect_ratio') })}
      >{locale.t('generation.dimensions.aspect_ratio')}</button>
      <InfoTip text={locale.t('generation.dimensions.aspect_ratio_tip')} />
      <button
        class="ml-auto text-neutral-400 hover:text-neutral-200 focus:outline-none"
        onclick={() => (arOpen = !arOpen)}
        title={arOpen ? locale.t('common.collapse', { section: locale.t('generation.dimensions.aspect_ratio') }) : locale.t('common.expand', { section: locale.t('generation.dimensions.aspect_ratio') })}
        aria-label={arOpen ? locale.t('common.collapse', { section: locale.t('generation.dimensions.aspect_ratio') }) : locale.t('common.expand', { section: locale.t('generation.dimensions.aspect_ratio') })}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {arOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
      </button>
    </div>
    {#if arOpen}
    <div class="flex items-center gap-1 flex-wrap mb-2">
      {#each presets as preset (preset.label)}
        {@const preview = aspectPreviewSize(preset.w, preset.h)}
        <button
          onclick={() => applyPreset(preset.w, preset.h)}
          class="inline-flex items-center gap-1.5 text-xs px-2 py-1 rounded transition-colors {activePreset === preset.label
            ? 'bg-indigo-600 text-white'
            : 'bg-neutral-800 border border-neutral-700 text-neutral-400 hover:bg-neutral-700'}"
          title={preset.label}
        >
          <span
            class="inline-flex h-4 w-4 shrink-0 items-center justify-center overflow-visible"
            aria-hidden="true"
          >
            <span
              class="box-border rounded-sm border {activePreset === preset.label
                ? 'border-white/70 bg-white/25'
                : 'border-neutral-500 bg-neutral-600/50'}"
              style="width: {preview.w}px; height: {preview.h}px"
            ></span>
          </span>
          {preset.label}
        </button>
      {/each}
    </div>
    <div class="flex items-center gap-1.5">
      <div class="flex-1">
        <span class="block text-[10px] text-neutral-500 mb-0.5">{locale.t('generation.dimensions.width')}</span>
        <input
          type="text"
          inputmode="decimal"
          value={aspectWInput}
          oninput={(e) => onAspectInput("w", (e.target as HTMLInputElement).value)}
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-sm text-neutral-100 text-center focus:outline-none focus:border-indigo-500 transition-colors"
        />
      </div>
      <span class="text-neutral-500 text-sm mt-4">:</span>
      <div class="flex-1">
        <span class="block text-[10px] text-neutral-500 mb-0.5">{locale.t('generation.dimensions.height')}</span>
        <input
          type="text"
          inputmode="decimal"
          value={aspectHInput}
          oninput={(e) => onAspectInput("h", (e.target as HTMLInputElement).value)}
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-sm text-neutral-100 text-center focus:outline-none focus:border-indigo-500 transition-colors"
        />
      </div>
      <button
        onclick={swapAspect}
        class="text-neutral-400 hover:text-neutral-200 transition-colors shrink-0 mt-4"
        title={locale.t('generation.dimensions.swap')}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M7 16V4m0 0L3 8m4-4l4 4M17 8v12m0 0l4-4m-4 4l-4-4"/>
        </svg>
      </button>
    </div>
    <p class="text-[10px] text-neutral-500 mt-1">{locale.t('generation.dimensions.ratio_hint')}</p>
    {/if}
  </div>

  <!-- Side Length -->
  <div>
    <div class="flex items-center justify-between mb-1.5">
      <label class="text-xs text-neutral-400">{locale.t('generation.dimensions.resolution')}<InfoTip text={locale.t('generation.dimensions.resolution_tip')} /></label>
      <div class="flex items-center gap-2">
        <button
          onclick={() => { generation.resolutionLocked = !generation.resolutionLocked; generation.saveSettings(); }}
          class="inline-flex items-center gap-1 text-[10px] transition-colors {generation.resolutionLocked ? 'text-indigo-400 hover:text-indigo-300' : 'text-neutral-400 hover:text-neutral-200'}"
          title={locale.t('generation.dimensions.lock_tip')}
          aria-pressed={generation.resolutionLocked}
        >
          {#if generation.resolutionLocked}
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <rect x="3" y="11" width="18" height="11" rx="2"/>
              <path d="M7 11V7a5 5 0 0 1 10 0v4"/>
            </svg>
          {:else}
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <rect x="3" y="11" width="18" height="11" rx="2"/>
              <path d="M7 11V7a5 5 0 0 1 9.9-1"/>
            </svg>
          {/if}
          {locale.t('generation.dimensions.lock')}
        </button>
        <button
          onclick={() => applySideLength(DEFAULT_SIDE)}
          class="inline-flex items-center gap-1 text-[10px] text-neutral-400 hover:text-neutral-200 transition-colors"
          title={locale.t('generation.dimensions.reset', { res: DEFAULT_SIDE })}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/>
            <path d="M3 3v5h5"/>
          </svg>
          {locale.t('generation.dimensions.reset', { res: DEFAULT_SIDE })}
        </button>
      </div>
    </div>
    <div class="flex items-center gap-1 flex-wrap mb-2">
      {#each sidePresets as side (side)}
        {@const isRecommended = recommended !== null && side >= recommended.min && side <= recommended.max}
        <button
          onclick={() => applySideLength(side)}
          class="relative text-xs px-2 py-1 rounded transition-colors {sideLength === side
            ? 'bg-indigo-600 text-white'
            : 'bg-neutral-800 border border-neutral-700 text-neutral-400 hover:bg-neutral-700'}"
          title={isRecommended && familyLabel
            ? locale.t('generation.dimensions.recommended', { model: familyLabel, res: recommendedLabel })
            : String(side)}
        >
          {side}
          {#if isRecommended}
            <span class="absolute -top-0.5 -right-0.5 h-1.5 w-1.5 rounded-full bg-emerald-400" aria-hidden="true"></span>
          {/if}
        </button>
      {/each}
    </div>
    <input
      type="number"
      bind:value={sideLength}
      oninput={recalc}
      min="64"
      max="2048"
      step="8"
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-1.5 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
    />
    {#if recommended && familyLabel}
      <p class="text-[10px] text-neutral-500 mt-1">
        <span class="inline-block h-1.5 w-1.5 rounded-full bg-emerald-400 mr-1 align-middle" aria-hidden="true"></span>{locale.t('generation.dimensions.recommended', { model: familyLabel, res: recommendedLabel })}
      </p>
    {/if}
  </div>

  <!-- Resulting dimensions -->
  <div class="flex items-center justify-between text-xs text-neutral-400">
    <span>{locale.t('generation.dimensions.result')}</span>
    <span class="text-neutral-200">{generation.width} &times; {generation.height}</span>
  </div>
</div>
