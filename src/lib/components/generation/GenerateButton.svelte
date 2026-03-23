<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { canvas } from "../../stores/canvas.svelte.js";
  import { generate, interruptGeneration, deleteQueueItem, installPipPackage, downloadModel } from "../../utils/api.js";
  import { models } from "../../stores/models.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";

  interface Props {
    canvasEditorRef?: { getRasterComposite: () => HTMLCanvasElement | null; getMaskCanvas: () => HTMLCanvasElement | null };
  }

  let { canvasEditorRef }: Props = $props();
  let errorMsg = $state<string | null>(null);

  async function handleGenerate() {
    errorMsg = null;

    if (!generation.checkpoint) {
      errorMsg = "Select a checkpoint first";
      return;
    }

    try {
      // If canvas mode is active, export canvas content before generating
      if (canvas.isCanvasMode) {
        if (!canvasEditorRef) {
          throw new Error("Canvas editor is not ready yet. Please try again.");
        }
        await canvas.syncToGeneration(
          () => canvasEditorRef.getRasterComposite(),
          () => canvasEditorRef.getMaskCanvas()
        );
      }

      if (generation.mode === "inpainting") {
        if (!generation.inputImage) {
          errorMsg = "Inpainting needs an input image. Upload one or use a staged image.";
          return;
        }
        if (!generation.maskImage) {
          errorMsg = "Inpainting needs a mask. Paint a mask in Canvas Editor or upload one.";
          return;
        }
      }

      // Ensure face fix dependencies are ready when enabled
      if (generation.facefixEnabled) {
        const detector = generation.facefixDetector || "face_yolov8m.pt";
        if (!models.ultralyticsModels.includes(detector)) {
          gallery.showToast("Downloading face fix model...", "info");
          await downloadModel(
            `https://huggingface.co/Bingsu/adetailer/resolve/main/${detector}`,
            "ultralytics",
            detector,
          );
          generation.facefixDetector = detector;
          await models.refresh();
        }
        await installPipPackage("ultralytics");
      }

      const params = generation.toParams();
      generation.saveCurrentPromptToHistory();
      const result = await generate(params);
      params.seed = result.seed;
      progress.enqueue(result.prompt_id, params.upscale_enabled, params.mode, params);
      generation.saveSettings();
    } catch (e) {
      console.error("Generation failed:", e);
      errorMsg = String(e);
    }
  }

  /** Left-click: cancel the current generation only, let the queue continue. */
  async function handleCancelCurrent() {
    await interruptGeneration();
    // The executing:null / execution_error handler will remove this prompt from the queue
  }

  /** Right-click: cancel current + clear the entire queue. */
  async function handleCancelAll(e: MouseEvent) {
    e.preventDefault();
    const promptIds = progress.pendingPrompts.map((p) => p.promptId);
    await interruptGeneration();
    for (const pid of promptIds) {
      try { await deleteQueueItem(pid); } catch { /* already removed */ }
    }
    progress.cancelAll();
  }

  const canGenerate = $derived(!!generation.checkpoint);

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      handleGenerate();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="flex gap-2">
  <button
    onclick={handleGenerate}
    disabled={!canGenerate}
    class="flex-1 py-3 rounded-xl font-semibold text-sm transition-colors
      {canGenerate
        ? 'bg-indigo-600 hover:bg-indigo-500 text-white shadow-lg shadow-indigo-600/20'
        : 'bg-neutral-800 text-neutral-500 cursor-not-allowed'}"
  >
    {#if progress.queueCount > 0}
      Generate (+{progress.queueCount})
    {:else}
      Generate
    {/if}
  </button>

  {#if progress.isGenerating}
    <button
      onclick={handleCancelCurrent}
      oncontextmenu={handleCancelAll}
      class="px-4 py-3 rounded-xl font-semibold text-sm bg-red-700 hover:bg-red-600 text-white transition-colors"
      title="Click: skip current — Right-click: cancel all"
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
        <line x1="18" y1="6" x2="6" y2="18"></line>
        <line x1="6" y1="6" x2="18" y2="18"></line>
      </svg>
    </button>
  {/if}
</div>

{#if errorMsg}
  <p class="text-xs text-red-800 text-center mt-1">{errorMsg}</p>
{/if}
