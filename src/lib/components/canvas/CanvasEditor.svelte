<script lang="ts">
  import { onMount } from "svelte";
  import { canvas } from "../../stores/canvas.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import CanvasToolbar from "./CanvasToolbar.svelte";
  import CanvasStage from "./CanvasStage.svelte";
  import CanvasStatusBar from "./CanvasStatusBar.svelte";
  import CanvasStagingStrip from "./staging/CanvasStagingStrip.svelte";

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

    {#if generation.mode === "inpainting" && progress.isGenerating}
      <div class="absolute inset-0 z-20 bg-black/50 backdrop-blur-[1px] flex items-center justify-center p-4">
        <div class="w-full max-w-xl rounded-xl border border-neutral-700 bg-neutral-950/95 shadow-2xl overflow-hidden">
          <div class="px-4 py-3 border-b border-neutral-800 flex items-center justify-between">
            <div class="text-sm font-medium text-neutral-100">Inpainting Preview</div>
            <div class="text-xs text-neutral-400">{progress.phaseLabel || "Generating..."}</div>
          </div>

          <div class="p-4">
            <div class="aspect-video rounded-lg border border-neutral-800 bg-neutral-900 flex items-center justify-center overflow-hidden">
              {#if progress.displayImage}
                <img
                  src={progress.displayImage}
                  alt="Inpainting generation preview"
                  class="w-full h-full object-contain"
                />
              {:else}
                <div class="flex flex-col items-center gap-2 text-neutral-400">
                  <div class="w-5 h-5 border-2 border-indigo-500 border-t-transparent rounded-full animate-spin"></div>
                  <span class="text-xs">Waiting for preview frames...</span>
                </div>
              {/if}
            </div>

            <div class="mt-3">
              <div class="h-2 rounded-full bg-neutral-800 overflow-hidden">
                <div
                  class="h-full bg-indigo-500 transition-all duration-200"
                  style="width: {Math.max(2, progress.percentage)}%"
                ></div>
              </div>
              <div class="mt-1 text-[11px] text-neutral-500 text-right">
                {progress.currentStep} / {progress.totalSteps || "?"} steps
              </div>
            </div>
          </div>
        </div>
      </div>
    {/if}
  </div>
  <CanvasStagingStrip />
  <CanvasStatusBar />
</div>
