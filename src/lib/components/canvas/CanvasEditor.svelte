<script lang="ts">
  import { onMount } from "svelte";
  import { canvas } from "../../stores/canvas.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import CanvasToolbar from "./CanvasToolbar.svelte";
  import CanvasStage from "./CanvasStage.svelte";
  import CanvasStatusBar from "./CanvasStatusBar.svelte";
  import CanvasStagingStrip from "./staging/CanvasStagingStrip.svelte";

  interface Props {
    showInpaintPreviewOverlay?: boolean;
  }

  let { showInpaintPreviewOverlay = true }: Props = $props();

  let stageRef: CanvasStage | undefined = $state();

  onMount(() => {
    // Initialize canvas with generation dimensions if not already set
    if (canvas.layers.length === 0) {
      canvas.initCanvas(generation.width, generation.height);
    }
  });

  $effect(() => {
    if (!canvas.isCanvasMode) return;

    const width = generation.width;
    const height = generation.height;

    if (canvas.layers.length === 0) {
      canvas.initCanvas(width, height);
      return;
    }

    if (canvas.canvasWidth !== width || canvas.canvasHeight !== height) {
      canvas.initCanvas(width, height);
    }
  });

  // Expose export functions for generation integration
  export function getRasterComposite(): HTMLCanvasElement | null {
    return stageRef?.getRasterComposite() ?? null;
  }

  export function getMaskCanvas(): HTMLCanvasElement | null {
    return stageRef?.getMaskCanvas() ?? null;
  }
</script>

<div class="flex flex-col h-full rounded-xl border border-neutral-800 overflow-hidden">
  <CanvasToolbar />
  <div class="flex-1 min-h-0 relative">
    <CanvasStage bind:this={stageRef} />

    {#if showInpaintPreviewOverlay && generation.mode === "inpainting" && progress.isGenerating}
      <div class="absolute inset-0 z-20 pointer-events-none">
        <div class="absolute inset-0 bg-black/15"></div>

        <div class="absolute right-4 top-4 w-full max-w-md rounded-xl border border-neutral-700/80 bg-neutral-950/88 shadow-2xl backdrop-blur-sm overflow-hidden">
          <div class="px-4 py-3 border-b border-neutral-800 flex items-center justify-between gap-3">
            <div class="text-sm font-medium text-neutral-100">{locale.t('canvas.inpainting_preview')}</div>
            <div class="text-xs text-neutral-400 text-right">{progress.phaseLabel || locale.t("progress.generating")}</div>
          </div>

          <div class="p-3">
            <div class="aspect-video rounded-lg border border-neutral-800 bg-neutral-900 flex items-center justify-center overflow-hidden">
              {#if progress.displayImage}
                <img
                  src={progress.displayImage}
                  alt={locale.t("canvas.inpaint_preview_alt")}
                  class="w-full h-full object-contain"
                />
              {:else}
                <div class="flex flex-col items-center gap-2 text-neutral-400">
                  <div class="w-5 h-5 border-2 border-indigo-500 border-t-transparent rounded-full animate-spin"></div>
                  <span class="text-xs">{locale.t('canvas.waiting_preview')}</span>
                </div>
              {/if}
            </div>

            <div class="mt-3">
              <div class="h-2 rounded-full bg-neutral-800 overflow-hidden">
                <div
                  class="h-full bg-indigo-500 transition-[width] duration-200"
                  style="width: {Math.max(2, progress.percentage)}%"
                ></div>
              </div>
              <div class="mt-1 text-[11px] text-neutral-500 text-right">
                {progress.currentStep} / {progress.totalSteps || "?"} {locale.t("progress.steps_suffix")}
              </div>
            </div>
          </div>
        </div>
      </div>
    {/if}

    {#if canvas.canApplyInpaintResult && !progress.isGenerating}
      <div class="absolute inset-x-0 bottom-4 z-20 flex justify-center px-4 pointer-events-none">
        <div class="pointer-events-auto flex items-center gap-3 rounded-full border border-neutral-700/80 bg-neutral-950/90 py-1.5 pl-4 pr-1.5 shadow-2xl backdrop-blur-sm">
          <span class="text-xs text-neutral-300">{locale.t('canvas.inpaint_result_ready')}</span>
          <button
            onclick={() => canvas.applyInpaintResult()}
            class="text-xs font-medium px-4 py-1.5 rounded-full border border-emerald-500 bg-emerald-600/25 text-emerald-100 hover:border-emerald-400 hover:bg-emerald-600/40"
            title={locale.t('canvas.apply_inpaint_title')}
          >
            {locale.t('canvas.accept')}
          </button>
        </div>
      </div>
    {/if}
  </div>
  <CanvasStagingStrip />
  <CanvasStatusBar />
</div>
