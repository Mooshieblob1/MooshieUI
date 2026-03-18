<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { canvas } from "../../stores/canvas.svelte.js";
  import { generate, interruptGeneration } from "../../utils/api.js";

  interface Props {
    canvasEditorRef?: { getRasterComposite: () => HTMLCanvasElement | null; getMaskCanvas: () => HTMLCanvasElement | null };
  }

  let { canvasEditorRef }: Props = $props();
  let errorMsg = $state<string | null>(null);

  async function handleGenerate() {
    errorMsg = null;
    if (progress.isGenerating) {
      await interruptGeneration();
      progress.reset();
      return;
    }

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

      const params = generation.toParams();
      const promptId = await generate(params);
      progress.startGeneration(promptId, params.upscale_enabled, params.mode);
      generation.saveSettings();
    } catch (e) {
      console.error("Generation failed:", e);
      errorMsg = String(e);
      progress.reset();
    }
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

<button
  onclick={handleGenerate}
  disabled={!canGenerate && !progress.isGenerating}
  class="w-full py-3 rounded-xl font-semibold text-sm transition-all {progress.isGenerating
    ? 'bg-red-600 hover:bg-red-500 text-white'
    : canGenerate
      ? 'bg-indigo-600 hover:bg-indigo-500 text-white shadow-lg shadow-indigo-600/20'
      : 'bg-neutral-800 text-neutral-500 cursor-not-allowed'}"
>
  {#if progress.isGenerating}
    Cancel
  {:else}
    Generate
  {/if}
</button>

{#if errorMsg}
  <p class="text-xs text-red-400 text-center mt-1">{errorMsg}</p>
{/if}
