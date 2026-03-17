<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { generate, interruptGeneration } from "../../utils/api.js";

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
      const params = generation.toParams();
      const promptId = await generate(params);
      progress.startGeneration(promptId, params.upscale_enabled);
      generation.saveSettings();
    } catch (e) {
      console.error("Generation failed:", e);
      errorMsg = String(e);
      progress.reset();
    }
  }

  const canGenerate = $derived(!!generation.checkpoint);
</script>

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
