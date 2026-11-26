<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { novelai } from "../../stores/novelai.svelte.js";
  import type { NovelAiCharacter, NovelAiDirectorReference } from "../../types/index.js";
  import {
    chooseNovelAiSheetCanvas,
    computeSheetItemRect,
    resolveMultiCharSheetLayout,
  } from "../../utils/characterPreciseRefLogic.js";
  import { novelAiBase64ToSrc } from "../../utils/novelaiImage.js";
  import { naiV5Variant, type NovelAiModelInfo } from "../../utils/novelaiModels.js";
  import { scrollCapture } from "../../utils/scrollCapture.js";
  import { CHECKERBOARD_CLASS } from "../../utils/transparentBackdrop.js";
  import EditableValue from "../ui/EditableValue.svelte";
  import InfoTip from "../ui/InfoTip.svelte";

  interface Props {
    model: NovelAiModelInfo | null | undefined;
    references: readonly NovelAiDirectorReference[];
    resolvedTargets: readonly (number | null)[];
    characters: readonly NovelAiCharacter[];
    useCoords: boolean;
    width: number;
    height: number;
    sheetCutout: boolean;
    cuttingOut?: boolean;
    onUpdateRef: (refIndex: number, patch: Partial<NovelAiDirectorReference>) => void;
    onResetLayout: () => void;
    onCutoutAll: () => void;
    onToggleSheetCutout: (value: boolean) => void;
  }

  let {
    model,
    references,
    resolvedTargets,
    characters,
    useCoords,
    width,
    height,
    sheetCutout,
    cuttingOut = false,
    onUpdateRef,
    onResetLayout,
    onCutoutAll,
    onToggleSheetCutout,
  }: Props = $props();

  let canvasEl = $state<HTMLDivElement | null>(null);
  let selectedRefIndex = $state<number | null>(null);
  let naturalSizes = $state<Record<string, { w: number; h: number }>>({});
  let dragState = $state<{
    refIndex: number;
    kind: "move" | "resize";
    startClientX: number;
    startClientY: number;
    startX: number;
    startY: number;
    startScale: number;
    currentX: number;
    currentY: number;
    currentScale: number;
  } | null>(null);

  const isV5 = $derived(naiV5Variant(model?.id) !== null);
  const canvasSize = $derived(chooseNovelAiSheetCanvas(width, height, isV5));
  const canvasW = $derived(canvasSize[0]);
  const canvasH = $derived(canvasSize[1]);

  const layoutItems = $derived(
    resolveMultiCharSheetLayout(
      references,
      resolvedTargets,
      characters,
      useCoords,
      canvasW,
      canvasH,
    ),
  );

  const activeSelectedIndex = $derived.by(() => {
    if (selectedRefIndex !== null && layoutItems.some((item) => item.refIndex === selectedRefIndex)) {
      return selectedRefIndex;
    }
    return layoutItems[0]?.refIndex ?? null;
  });

  const selectedItem = $derived(
    layoutItems.find((item) => item.refIndex === activeSelectedIndex) ?? null,
  );

  function clamp(val: number, min: number, max: number): number {
    return Math.min(max, Math.max(min, val));
  }

  function displayBase64(raw: string): string {
    if (sheetCutout) {
      return novelai.getSheetCutout(raw) ?? raw;
    }
    return raw;
  }

  function itemValues(item: (typeof layoutItems)[number]): { x: number; y: number; scale: number } {
    if (dragState && dragState.refIndex === item.refIndex) {
      return {
        x: dragState.currentX,
        y: dragState.currentY,
        scale: dragState.currentScale,
      };
    }
    return { x: item.x, y: item.y, scale: item.scale };
  }

  function recordNaturalSize(key: string, img: HTMLImageElement) {
    const w = img.naturalWidth;
    const h = img.naturalHeight;
    if (w <= 0 || h <= 0) return;
    const prev = naturalSizes[key];
    if (prev && prev.w === w && prev.h === h) return;
    naturalSizes = { ...naturalSizes, [key]: { w, h } };
  }

  function startPointerInteraction(
    e: PointerEvent,
    item: (typeof layoutItems)[number],
    kind: "move" | "resize",
  ) {
    e.preventDefault();
    e.stopPropagation();
    selectedRefIndex = item.refIndex;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const cur = itemValues(item);
    dragState = {
      refIndex: item.refIndex,
      kind,
      startClientX: e.clientX,
      startClientY: e.clientY,
      startX: cur.x,
      startY: cur.y,
      startScale: cur.scale,
      currentX: cur.x,
      currentY: cur.y,
      currentScale: cur.scale,
    };
  }

  function movePointerInteraction(e: PointerEvent) {
    if (!dragState || !canvasEl) return;
    const rect = canvasEl.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return;
    const dx = (e.clientX - dragState.startClientX) / rect.width;
    const dy = (e.clientY - dragState.startClientY) / rect.height;
    if (dragState.kind === "move") {
      dragState = {
        ...dragState,
        currentX: clamp(dragState.startX + dx, 0.05, 0.95),
        currentY: clamp(dragState.startY + dy, 0.05, 0.95),
      };
    } else {
      const delta = (dx + dy) * 0.75;
      dragState = {
        ...dragState,
        currentScale: clamp(Math.round((dragState.startScale + delta) * 100) / 100, 0.2, 1.35),
      };
    }
  }

  function finishPointerInteraction() {
    if (!dragState) return;
    const finished = dragState;
    dragState = null;
    if (finished.kind === "move") {
      onUpdateRef(finished.refIndex, {
        sheet_x: Number(finished.currentX.toFixed(4)),
        sheet_y: Number(finished.currentY.toFixed(4)),
      });
    } else {
      onUpdateRef(finished.refIndex, {
        sheet_scale: Number(finished.currentScale.toFixed(3)),
      });
    }
  }

  function handleItemKeyDown(e: KeyboardEvent, item: (typeof layoutItems)[number]) {
    const step = e.shiftKey ? 0.05 : 0.02;
    const cur = itemValues(item);
    if (e.key === "ArrowLeft") {
      e.preventDefault();
      onUpdateRef(item.refIndex, { sheet_x: Number(clamp(cur.x - step, 0.05, 0.95).toFixed(4)) });
    } else if (e.key === "ArrowRight") {
      e.preventDefault();
      onUpdateRef(item.refIndex, { sheet_x: Number(clamp(cur.x + step, 0.05, 0.95).toFixed(4)) });
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      onUpdateRef(item.refIndex, { sheet_y: Number(clamp(cur.y - step, 0.05, 0.95).toFixed(4)) });
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      onUpdateRef(item.refIndex, { sheet_y: Number(clamp(cur.y + step, 0.05, 0.95).toFixed(4)) });
    } else if (e.key === "+" || e.key === "=") {
      e.preventDefault();
      onUpdateRef(item.refIndex, {
        sheet_scale: Number(clamp(cur.scale + 0.05, 0.2, 1.35).toFixed(3)),
      });
    } else if (e.key === "-") {
      e.preventDefault();
      onUpdateRef(item.refIndex, {
        sheet_scale: Number(clamp(cur.scale - 0.05, 0.2, 1.35).toFixed(3)),
      });
    }
  }
</script>

<div class="space-y-2 pt-1 border-t border-neutral-800/80">
  <div class="flex items-center justify-between gap-2">
    <span class="flex items-center gap-1.5 text-[11px] text-neutral-400 min-w-0">
      <span class="truncate">{locale.t("generation.novelai.reference.isolate_sheet_preview")}</span>
      <span class="text-[10px] tabular-nums text-neutral-500">{canvasW}x{canvasH}</span>
      <InfoTip text={locale.t("generation.novelai.reference.isolate_sheet_preview_tip")} />
    </span>
    <div class="flex items-center gap-1 shrink-0">
      <button
        type="button"
        disabled={cuttingOut}
        onclick={onCutoutAll}
        class="px-2 py-0.5 rounded text-[10px] bg-indigo-500/15 hover:bg-indigo-500/25 border border-indigo-500/35 text-indigo-200 disabled:opacity-50 transition-colors"
      >
        {cuttingOut
          ? locale.t("generation.novelai.reference.isolate_sheet_cutout_running")
          : locale.t("generation.novelai.reference.isolate_sheet_cutout_btn")}
      </button>
      <button
        type="button"
        onclick={onResetLayout}
        class="px-2 py-0.5 rounded text-[10px] bg-neutral-800 hover:bg-neutral-700 text-neutral-300 transition-colors"
      >
        {locale.t("generation.novelai.reference.isolate_sheet_reset")}
      </button>
    </div>
  </div>

  <div class="flex justify-center">
    <div
      bind:this={canvasEl}
      class="relative overflow-hidden rounded-md border border-neutral-800 {isV5
        ? CHECKERBOARD_CLASS
        : 'bg-neutral-100'}"
      style="aspect-ratio: {canvasW} / {canvasH}; width: min(100%, calc(200px * {canvasW} / {canvasH}));"
    >
      <div class="pointer-events-none absolute inset-0 grid grid-cols-3 grid-rows-3 opacity-25">
        {#each Array(9) as _, cell (cell)}
          <div class="border border-neutral-500/30"></div>
        {/each}
      </div>

      {#if useCoords}
        {#each characters as ch, idx (idx)}
          {#if ch.enabled && ch.prompt.trim()}
            <div
              class="pointer-events-none absolute -translate-x-1/2 -translate-y-1/2 w-2.5 h-2.5 rounded-full border border-indigo-400/70 bg-indigo-500/20"
              style="left: {clamp(ch.center.x, 0.05, 0.95) * 100}%; top: {clamp(ch.center.y, 0.05, 0.95) * 100}%;"
            ></div>
          {/if}
        {/each}
      {/if}

      {#each layoutItems as item (item.refIndex)}
        {@const imgData = displayBase64(item.ref.image)}
        {@const dims = naturalSizes[imgData] ?? {
          w: Math.round(canvasW * 0.5),
          h: Math.round(canvasH * 0.8),
        }}
        {@const vals = itemValues(item)}
        {@const rect = computeSheetItemRect(
          canvasW,
          canvasH,
          dims.w,
          dims.h,
          vals.x,
          vals.y,
          vals.scale,
        )}
        {@const isSelected = activeSelectedIndex === item.refIndex}
        <div
          class="absolute"
          style="left: {(rect.left / canvasW) * 100}%; top: {(rect.top / canvasH) * 100}%; width: {(rect.width / canvasW) * 100}%; height: {(rect.height / canvasH) * 100}%;"
        >
          <button
            type="button"
            class="relative w-full h-full block cursor-move select-none rounded-sm border transition-colors focus:outline-none {isSelected
              ? 'border-indigo-400 ring-1 ring-indigo-400/60'
              : 'border-white/25 hover:border-white/50'}"
            aria-label={locale.t("generation.novelai.reference.isolate_target_character", {
              index: String(item.characterIndex + 1),
            })}
            onpointerdown={(e) => startPointerInteraction(e, item, "move")}
            onpointermove={movePointerInteraction}
            onpointerup={finishPointerInteraction}
            onpointercancel={finishPointerInteraction}
            onkeydown={(e) => handleItemKeyDown(e, item)}
          >
            <img
              src={novelAiBase64ToSrc(imgData)}
              alt=""
              draggable="false"
              onload={(e) => recordNaturalSize(imgData, e.currentTarget as HTMLImageElement)}
              class="w-full h-full object-fill pointer-events-none select-none"
            />
            <span
              class="pointer-events-none absolute top-0.5 left-0.5 px-1 py-px rounded text-[9px] font-semibold leading-none bg-neutral-950/85 text-amber-300 border border-amber-500/40"
            >
              C{item.characterIndex + 1}
            </span>
          </button>

          {#if isSelected}
            <button
              type="button"
              class="absolute -bottom-1.5 -right-1.5 w-3.5 h-3.5 rounded-full bg-indigo-500 border border-white/80 cursor-nwse-resize shadow focus:outline-none"
              aria-label={locale.t("generation.novelai.reference.isolate_sheet_scale")}
              onpointerdown={(e) => startPointerInteraction(e, item, "resize")}
              onpointermove={movePointerInteraction}
              onpointerup={finishPointerInteraction}
              onpointercancel={finishPointerInteraction}
            ></button>
          {/if}
        </div>
      {/each}
    </div>
  </div>

  {#if selectedItem}
    {@const cur = itemValues(selectedItem)}
    <div use:scrollCapture>
      <label class="flex items-center justify-between text-[11px] text-neutral-400 mb-1">
        <span class="flex items-center gap-1.5">
          <span class="text-amber-300 font-medium">C{selectedItem.characterIndex + 1}</span>
          <span>{locale.t("generation.novelai.reference.isolate_sheet_scale")}</span>
        </span>
        <EditableValue
          value={Math.round(cur.scale * 100)}
          min={20}
          max={135}
          step={5}
          suffix="%"
          onchange={(v) =>
            onUpdateRef(selectedItem.refIndex, {
              sheet_scale: Number(clamp(v / 100, 0.2, 1.35).toFixed(3)),
            })}
        />
      </label>
      <input
        type="range"
        min="0.2"
        max="1.35"
        step="0.05"
        value={cur.scale}
        oninput={(e) =>
          onUpdateRef(selectedItem.refIndex, {
            sheet_scale: Number(e.currentTarget.value),
          })}
        class="w-full accent-indigo-500"
      />
    </div>
  {/if}

  <label class="flex items-center gap-2 text-xs text-neutral-300">
    <input
      type="checkbox"
      class="accent-indigo-500"
      checked={sheetCutout}
      onchange={(e) => onToggleSheetCutout(e.currentTarget.checked)}
    />
    <span>{locale.t("generation.novelai.reference.isolate_sheet_cutout")}</span>
    <InfoTip text={locale.t("generation.novelai.reference.isolate_sheet_cutout_tip")} />
  </label>
</div>
