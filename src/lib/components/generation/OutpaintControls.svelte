<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { canvas } from "../../stores/canvas.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { scrollCapture } from "../../utils/scrollCapture.js";
  import {
    clampOutpaintFeather,
    clampOutpaintPadding,
    computeOutpaintDimensions,
    OUTPAINT_MAX_FEATHER,
    OUTPAINT_MAX_PAD,
  } from "../../utils/outpaintCanvas.js";
  import EditableValue from "../ui/EditableValue.svelte";
  import InfoTip from "../ui/InfoTip.svelte";

  interface Props {
    imagePreviewUrl: string | null;
    imageAspect: { w: number; h: number } | null;
  }

  let { imagePreviewUrl, imageAspect }: Props = $props();

  const resolvedPreviewUrl = $derived(imagePreviewUrl ?? canvas.referenceImageUrl);
  const srcWidth = $derived(imageAspect?.w ?? generation.width);
  const srcHeight = $derived(imageAspect?.h ?? generation.height);

  const geom = $derived(
    computeOutpaintDimensions(srcWidth, srcHeight, {
      top: generation.outpaintTop,
      bottom: generation.outpaintBottom,
      left: generation.outpaintLeft,
      right: generation.outpaintRight,
      feather: generation.outpaintFeather,
    }),
  );

  // Percentage placement of the original image inside the expanded canvas preview
  const srcLeftPct = $derived((geom.srcX / geom.width) * 100);
  const srcTopPct = $derived((geom.srcY / geom.height) * 100);
  const srcWidthPct = $derived((geom.srcWidth / geom.width) * 100);
  const srcHeightPct = $derived((geom.srcHeight / geom.height) * 100);

  function setSide(side: "top" | "bottom" | "left" | "right", val: number) {
    const clamped = clampOutpaintPadding(val);
    if (side === "top") generation.outpaintTop = clamped;
    else if (side === "bottom") generation.outpaintBottom = clamped;
    else if (side === "left") generation.outpaintLeft = clamped;
    else generation.outpaintRight = clamped;
  }

  function nudgeSide(side: "top" | "bottom" | "left" | "right", delta: number) {
    const current =
      side === "top"
        ? generation.outpaintTop
        : side === "bottom"
          ? generation.outpaintBottom
          : side === "left"
            ? generation.outpaintLeft
            : generation.outpaintRight;
    setSide(side, current + delta);
  }

  function applyPreset(top: number, bottom: number, left: number, right: number) {
    generation.outpaintTop = top;
    generation.outpaintBottom = bottom;
    generation.outpaintLeft = left;
    generation.outpaintRight = right;
  }
</script>

<div class="space-y-2.5 pt-1">
  <div class="flex items-center justify-between gap-2">
    <span class="text-xs font-medium text-neutral-300 flex items-center">
      {locale.t("generation.outpaint.directions")}
      <InfoTip text={locale.t("generation.outpaint.directions_tip")} />
    </span>
    <div class="flex items-center gap-1">
      <button
        type="button"
        onclick={() => applyPreset(0, 0, 256, 256)}
        class="px-1.5 py-0.5 text-[10px] rounded border border-neutral-700 text-neutral-300 hover:border-indigo-500/60 hover:text-indigo-200 transition-colors"
      >
        {locale.t("generation.outpaint.preset_sides")}
      </button>
      <button
        type="button"
        onclick={() => applyPreset(256, 256, 0, 0)}
        class="px-1.5 py-0.5 text-[10px] rounded border border-neutral-700 text-neutral-300 hover:border-indigo-500/60 hover:text-indigo-200 transition-colors"
      >
        {locale.t("generation.outpaint.preset_vertical")}
      </button>
      <button
        type="button"
        onclick={() => applyPreset(256, 256, 256, 256)}
        class="px-1.5 py-0.5 text-[10px] rounded border border-neutral-700 text-neutral-300 hover:border-indigo-500/60 hover:text-indigo-200 transition-colors"
      >
        {locale.t("generation.outpaint.preset_all")}
      </button>
      <button
        type="button"
        onclick={() => applyPreset(0, 0, 0, 0)}
        class="px-1.5 py-0.5 text-[10px] rounded border border-neutral-800 text-neutral-400 hover:border-neutral-600 hover:text-neutral-200 transition-colors"
      >
        {locale.t("generation.outpaint.preset_reset")}
      </button>
    </div>
  </div>

  <!-- Directional pad layout: Top / Left + Live Preview + Right / Bottom -->
  <div class="rounded-lg border border-neutral-800 bg-neutral-950/60 p-2.5 space-y-2">
    <!-- Top -->
    <div class="flex items-center justify-center gap-1.5">
      <span class="text-[11px] text-neutral-400 w-12 text-right">{locale.t("generation.outpaint.top")}</span>
      <button
        type="button"
        onclick={() => nudgeSide("top", -64)}
        class="w-5 h-6 rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-300 text-xs"
        aria-label="{locale.t('generation.outpaint.top')} -64"
      >-</button>
      <input
        type="number"
        min="0"
        max={OUTPAINT_MAX_PAD}
        step="8"
        value={generation.outpaintTop}
        oninput={(e) => setSide("top", Number((e.currentTarget as HTMLInputElement).value))}
        class="w-16 h-6 px-1.5 rounded bg-neutral-900 border border-neutral-700 text-xs text-neutral-200 text-center tabular-nums"
      />
      <button
        type="button"
        onclick={() => nudgeSide("top", 64)}
        class="w-5 h-6 rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-300 text-xs"
        aria-label="{locale.t('generation.outpaint.top')} +64"
      >+</button>
    </div>

    <!-- Left + Live Expanded Frame Preview + Right -->
    <div class="grid grid-cols-[1fr_minmax(110px,1.2fr)_1fr] items-center gap-2">
      <!-- Left -->
      <div class="flex flex-col items-center gap-1">
        <span class="text-[11px] text-neutral-400">{locale.t("generation.outpaint.left")}</span>
        <div class="flex items-center gap-1">
          <button
            type="button"
            onclick={() => nudgeSide("left", -64)}
            class="w-5 h-6 rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-300 text-xs"
            aria-label="{locale.t('generation.outpaint.left')} -64"
          >-</button>
          <input
            type="number"
            min="0"
            max={OUTPAINT_MAX_PAD}
            step="8"
            value={generation.outpaintLeft}
            oninput={(e) => setSide("left", Number((e.currentTarget as HTMLInputElement).value))}
            class="w-14 h-6 px-1 rounded bg-neutral-900 border border-neutral-700 text-xs text-neutral-200 text-center tabular-nums"
          />
          <button
            type="button"
            onclick={() => nudgeSide("left", 64)}
            class="w-5 h-6 rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-300 text-xs"
            aria-label="{locale.t('generation.outpaint.left')} +64"
          >+</button>
        </div>
      </div>

      <!-- Expanded frame preview -->
      <div class="flex flex-col items-center gap-1">
        <div
          class="relative w-full max-h-28 overflow-hidden rounded border border-indigo-500/40 bg-[#1a1a1f] bg-[conic-gradient(#26262e_90deg,#1a1a1f_90deg_180deg,#26262e_180deg_270deg,#1a1a1f_270deg)] bg-[length:12px_12px]"
          style="aspect-ratio: {geom.width} / {geom.height};"
        >
          <div
            class="absolute overflow-hidden border border-dashed border-amber-400/70 bg-neutral-900/80"
            style="left: {srcLeftPct}%; top: {srcTopPct}%; width: {srcWidthPct}%; height: {srcHeightPct}%;"
          >
            {#if resolvedPreviewUrl}
              <img
                src={resolvedPreviewUrl}
                alt={locale.t("generation.image.input")}
                class="w-full h-full object-fill"
              />
            {/if}
          </div>
        </div>
        <span class="text-[10px] text-neutral-400 tabular-nums">
          {srcWidth}&times;{srcHeight} &rarr; <span class="text-indigo-300 font-medium">{geom.width}&times;{geom.height}</span>
        </span>
      </div>

      <!-- Right -->
      <div class="flex flex-col items-center gap-1">
        <span class="text-[11px] text-neutral-400">{locale.t("generation.outpaint.right")}</span>
        <div class="flex items-center gap-1">
          <button
            type="button"
            onclick={() => nudgeSide("right", -64)}
            class="w-5 h-6 rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-300 text-xs"
            aria-label="{locale.t('generation.outpaint.right')} -64"
          >-</button>
          <input
            type="number"
            min="0"
            max={OUTPAINT_MAX_PAD}
            step="8"
            value={generation.outpaintRight}
            oninput={(e) => setSide("right", Number((e.currentTarget as HTMLInputElement).value))}
            class="w-14 h-6 px-1 rounded bg-neutral-900 border border-neutral-700 text-xs text-neutral-200 text-center tabular-nums"
          />
          <button
            type="button"
            onclick={() => nudgeSide("right", 64)}
            class="w-5 h-6 rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-300 text-xs"
            aria-label="{locale.t('generation.outpaint.right')} +64"
          >+</button>
        </div>
      </div>
    </div>

    <!-- Bottom -->
    <div class="flex items-center justify-center gap-1.5">
      <span class="text-[11px] text-neutral-400 w-12 text-right">{locale.t("generation.outpaint.bottom")}</span>
      <button
        type="button"
        onclick={() => nudgeSide("bottom", -64)}
        class="w-5 h-6 rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-300 text-xs"
        aria-label="{locale.t('generation.outpaint.bottom')} -64"
      >-</button>
      <input
        type="number"
        min="0"
        max={OUTPAINT_MAX_PAD}
        step="8"
        value={generation.outpaintBottom}
        oninput={(e) => setSide("bottom", Number((e.currentTarget as HTMLInputElement).value))}
        class="w-16 h-6 px-1.5 rounded bg-neutral-900 border border-neutral-700 text-xs text-neutral-200 text-center tabular-nums"
      />
      <button
        type="button"
        onclick={() => nudgeSide("bottom", 64)}
        class="w-5 h-6 rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-300 text-xs"
        aria-label="{locale.t('generation.outpaint.bottom')} +64"
      >+</button>
    </div>
  </div>

  <!-- Feather width -->
  <div use:scrollCapture>
    <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
      <span class="flex items-center">
        {locale.t("generation.outpaint.feather")}
        <InfoTip text={locale.t("generation.outpaint.feather_tip")} />
      </span>
      <EditableValue
        value={generation.outpaintFeather}
        min={0}
        max={OUTPAINT_MAX_FEATHER}
        step={4}
        decimals={0}
        onchange={(v) => (generation.outpaintFeather = clampOutpaintFeather(v))}
      />
    </label>
    <input
      type="range"
      bind:value={generation.outpaintFeather}
      min="0"
      max={OUTPAINT_MAX_FEATHER}
      step="4"
      class="w-full accent-indigo-500"
    />
  </div>
</div>
