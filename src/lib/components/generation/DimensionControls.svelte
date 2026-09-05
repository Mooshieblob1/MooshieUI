<script lang="ts">
  import { untrack } from "svelte";
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import InfoTip from "../ui/InfoTip.svelte";
  import type { ModelFamily } from "../../utils/modelFamily.js";
  import {
    NOVELAI_ASPECT_RATIOS,
    NOVELAI_DIMENSION_STEP,
    naiV5Variant,
    nearestNovelAiAspect,
  } from "../../utils/novelaiModels.js";
  import { novelai } from "../../stores/novelai.svelte.js";
  import { novelAiOpusCovers } from "../../utils/novelaiCost.js";

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

  /**
   * The pixel grid every dimension has to land on.
   *
   * NovelAI rejects anything that is not a multiple of 64, and its own UI steps
   * the side length 1024 -> 1088 -> 1152. Local backends accept 8, so the
   * coarser grid applies in NovelAI mode only.
   */
  const quantum = $derived(generation.isNovelAi ? NOVELAI_DIMENSION_STEP : 8);

  /** Try to match persisted width/height back to a preset or simplified ratio. */
  /**
   * Largest pair on the pixel grid whose area does not go over `area`.
   *
   * NovelAI prices by pixel count and Opus only covers a generation while it
   * stays at or under one megapixel, so rounding a ratio up past the requested
   * area is what quietly turns a free 1024 generation into a paid one. That is
   * what 2:3 did: the area-faithful formula picked 832x1280, 1.06 MP, while
   * 3:4 happened to round down and stayed free. Walking the width across a few
   * grid steps and taking the tallest height that still fits lands on
   * 832x1216, which is NovelAI's own portrait preset.
   */
  function dimsUnderArea(aw: number, ah: number, area: number, q: number): { w: number; h: number } {
    const target = aw / ah;
    let best: { w: number; h: number; err: number } | null = null;
    const mid = Math.round(Math.sqrt(area * target) / q);
    for (let k = Math.max(1, mid - 2); k <= mid + 2; k++) {
      const w = k * q;
      const h = Math.floor(area / w / q) * q;
      if (h < q) continue;
      // Scored in log space so a result that is too wide and one that is too
      // tall by the same factor are treated as equally far off.
      const err = Math.abs(Math.log(w / h / target));
      if (!best || err < best.err) best = { w, h, err };
    }
    return best ? { w: best.w, h: best.h } : { w: q, h: q };
  }

  /** Compute dimensions for a given aspect ratio using the area-faithful formula. */
  function dimsForAspect(aw: number, ah: number, side: number, q = quantum): { w: number; h: number } {
    const area = side * side;
    if (generation.isNovelAi) return dimsUnderArea(aw, ah, area, q);
    const wA = Math.max(q, Math.round(Math.sqrt(area * (aw / ah)) / q) * q);
    const hA = Math.max(q, Math.round(area / wA / q) * q);
    const hB = Math.max(q, Math.round(Math.sqrt(area * (ah / aw)) / q) * q);
    const wB = Math.max(q, Math.round(area / hB / q) * q);
    return Math.abs(wA * hA - area) <= Math.abs(wB * hB - area)
      ? { w: wA, h: hA }
      : { w: wB, h: hB };
  }

  /**
   * How far (in log space) a pixel ratio may sit from a preset and still be
   * shown as that preset. 0.04 comfortably covers grid snapping (NovelAI's
   * 1216x832 is 2.6% off a true 3:2) while staying well inside half the gap
   * between the closest presets, 4:3 and 3:2 (0.059).
   */
  const PRESET_RATIO_TOLERANCE = 0.04;

  function inferAspectFromDimensions(w: number, h: number) {
    // NovelAI only ever gets one of its own shapes, whatever the pixels say.
    if (generation.isNovelAi) return nearestNovelAiAspect(w, h);
    // Check presets first (exact match on resulting dimensions)
    for (const p of presets) {
      const dims = dimsForAspect(p.w, p.h, sideLength);
      if (dims.w === w && dims.h === h) {
        return { w: p.w, h: p.h };
      }
    }
    // Then the nearest preset by ratio. Imported images land on the pixel
    // grid, not on an exact ratio, so 1216x832 should still read as 3:2.
    const ratio = w / h;
    let nearest: { w: number; h: number; err: number } | null = null;
    for (const p of presets) {
      const err = Math.abs(Math.log(ratio / (p.w / p.h)));
      if (!nearest || err < nearest.err) nearest = { w: p.w, h: p.h, err };
    }
    if (nearest && nearest.err <= PRESET_RATIO_TOLERANCE) {
      return { w: nearest.w, h: nearest.h };
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

      syncControlsFromDimensions(w, h);
    }
  });

  // Point the aspect and side-length readouts at the given dimensions without
  // writing anything back to the store.
  function syncControlsFromDimensions(w: number, h: number) {
    const inferred = inferAspectFromDimensions(w, h);
    aspectW = inferred.w;
    aspectH = inferred.h;
    aspectWInput = String(inferred.w);
    aspectHInput = String(inferred.h);

    // Keep side-length control aligned with the current generated area.
    sideLength = Math.max(quantum, Math.round(Math.sqrt(w * h) / quantum) * quantum);
  }

  // When an input image is loaded, adopt its aspect ratio. The suggestion
  // arrives as raw pixel dimensions, so reduce it to a ratio first; otherwise
  // the inputs would read "1024 : 1536" instead of "2 : 3".
  let lastAppliedKey = "";
  $effect(() => {
    if (suggestedAspect && suggestedAspect.w > 0 && suggestedAspect.h > 0) {
      const key = `${suggestedAspect.w}:${suggestedAspect.h}`;
      if (key !== lastAppliedKey) {
        lastAppliedKey = key;
        const inferred = inferAspectFromDimensions(suggestedAspect.w, suggestedAspect.h);
        aspectW = inferred.w;
        aspectH = inferred.h;
        aspectWInput = String(inferred.w);
        aspectHInput = String(inferred.h);
      }
    }
  });

  const LOCAL_PRESETS: ReadonlyArray<{ label: string; w: number; h: number }> = [
    { label: "1:1", w: 1, h: 1 },
    { label: "4:3", w: 4, h: 3 },
    { label: "3:2", w: 3, h: 2 },
    { label: "16:9", w: 16, h: 9 },
    { label: "21:9", w: 21, h: 9 },
    { label: "3:4", w: 3, h: 4 },
    { label: "2:3", w: 2, h: 3 },
    { label: "9:16", w: 9, h: 16 },
  ];

  // NovelAI mode offers only the shapes novelai.net itself does.
  const presets = $derived(generation.isNovelAi ? NOVELAI_ASPECT_RATIOS : LOCAL_PRESETS);

  function recalc() {
    const dims = dimsForAspect(
      Math.max(0.01, aspectW),
      Math.max(0.01, aspectH),
      Math.max(quantum, sideLength),
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

  /**
   * A typed ratio is free-form while it is being typed, so "16" can pass
   * through "1" without the field snapping underneath the cursor. Once the
   * field is committed, NovelAI mode pulls it onto the nearest NovelAI shape.
   */
  function onAspectCommit() {
    if (!generation.isNovelAi) return;
    const snapped = nearestNovelAiAspect(aspectW, aspectH);
    if (snapped.w === aspectW && snapped.h === aspectH) return;
    applyPreset(snapped.w, snapped.h);
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

  /**
   * Which presets Opus covers at the current side length and step count.
   *
   * Only ever populated on an Opus account in NovelAI mode, generating one
   * image at a time: on any other plan, or in a batch, every generation costs
   * Anlas and marking them all would say nothing. The dimensions come from the
   * same `dimsForAspect` the buttons apply, so a green border cannot disagree
   * with what clicking one produces.
   */
  const freePresets = $derived.by(() => {
    const free = new Set<string>();
    if (!generation.isNovelAi || !novelai.isOpus || generation.batchSize !== 1)
      return free;
    // V5 is outside Opus unlimited: with the allowance drained nothing is free.
    if (naiV5Variant(generation.checkpoint) !== null && novelai.opusAllowanceEmpty)
      return free;
    for (const p of presets) {
      const dims = dimsForAspect(p.w, p.h, sideLength);
      if (novelAiOpusCovers(dims.w, dims.h, generation.steps, true))
        free.add(p.label);
    }
    return free;
  });

  const DEFAULT_SIDE = 1024;
  const sidePresets = [512, 768, 1024, 1536, 2048];

  function applySideLength(side: number) {
    sideLength = side;
    recalc();
  }

  // Switching backends changes the legal grid, so dimensions carried over from
  // the other one can be illegal. Re-run the aspect maths whenever it changes.
  // The equality guard is what stops the self-write on `sideLength` from
  // re-triggering this effect.
  // Switching backends changes the legal grid, but never the resolution: the
  // store already snaps width and height to 64 when a NovelAI model is
  // selected (1024 stays 1024), and nothing here writes them. Only the
  // readouts are re-derived, so a 4:3 arriving in NovelAI mode reads as its
  // nearest NovelAI shape, 3:2, while the dimensions stay where they were.
  let lastQuantum = 0;
  $effect(() => {
    const q = quantum;
    if (q === lastQuantum) return;
    const first = lastQuantum === 0;
    lastQuantum = q;
    if (first) return;
    untrack(() => {
      const w = generation.width;
      const h = generation.height;
      if (w && h) syncControlsFromDimensions(w, h);
    });
  });

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
      {#if freePresets.size > 0}
        <span
          class="ml-auto mr-1 inline-flex items-center gap-1 rounded-full border border-green-500/40 bg-green-500/10 px-1.5 py-0.5 text-[10px] text-green-400"
          title={locale.t('generation.dimensions.free_opus_tip')}
        >
          <span class="h-1.5 w-1.5 rounded-full bg-green-400"></span>
          {locale.t('generation.dimensions.free_opus')}
        </span>
      {/if}
      <button
        class="{freePresets.size > 0 ? '' : 'ml-auto '}text-neutral-400 hover:text-neutral-200 focus:outline-none"
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
        {@const isFree = freePresets.has(preset.label)}
        <button
          onclick={() => applyPreset(preset.w, preset.h)}
          class="inline-flex items-center gap-1.5 text-xs px-2 py-1 rounded border transition-colors {activePreset === preset.label
            ? isFree
              ? 'bg-indigo-600 border-green-400 text-white'
              : 'bg-indigo-600 border-indigo-600 text-white'
            : isFree
              ? 'bg-neutral-800 border-green-500/60 text-neutral-400 hover:bg-neutral-700'
              : 'bg-neutral-800 border-neutral-700 text-neutral-400 hover:bg-neutral-700'}"
          title={isFree
            ? `${preset.label} - ${locale.t('generation.dimensions.free_opus_tip')}`
            : preset.label}
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
          onchange={onAspectCommit}
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
          onchange={onAspectCommit}
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
      step={quantum}
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
