<script lang="ts">
  import { progress } from "../../stores/progress.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { generate } from "../../utils/api.js";

  async function upscaleImage() {
    generation.upscaleEnabled = true;
    if (progress.lastOutputImage) {
      generation.inputImage = progress.lastOutputImage;
    }
    const params = generation.toParams();
    params.mode = "img2img";
    try {
      const promptId = await generate(params);
      progress.startGeneration(promptId);
    } catch (e) {
      console.error("Upscale failed:", e);
    }
  }
</script>

<div class="relative w-full aspect-square bg-neutral-900 rounded-xl border border-neutral-800 flex items-center justify-center overflow-hidden group">
  {#if progress.displayImage}
    <img
      src={progress.displayImage}
      alt="Preview"
      class="w-full h-full object-contain"
    />
    {#if !progress.isGenerating && progress.lastOutputImage}
      <div class="absolute top-3 right-3 opacity-0 group-hover:opacity-100 transition-opacity">
        <button
          onclick={upscaleImage}
          class="flex items-center gap-1.5 bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-medium px-3 py-1.5 rounded-lg shadow-lg transition-colors"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M3 4a1 1 0 011-1h4a1 1 0 010 2H6.414l2.293 2.293a1 1 0 01-1.414 1.414L5 6.414V8a1 1 0 01-2 0V4zm9 1a1 1 0 110-2h4a1 1 0 011 1v4a1 1 0 11-2 0V6.414l-2.293 2.293a1 1 0 11-1.414-1.414L13.586 5H12zm-9 7a1 1 0 112 0v1.586l2.293-2.293a1 1 0 011.414 1.414L6.414 15H8a1 1 0 110 2H4a1 1 0 01-1-1v-4zm13 3a1 1 0 01-1 1h-4a1 1 0 110-2h1.586l-2.293-2.293a1 1 0 011.414-1.414L15 13.586V12a1 1 0 112 0v4z" clip-rule="evenodd"/>
          </svg>
          Upscale
        </button>
      </div>
    {/if}
  {:else if progress.isGenerating}
    <div class="text-neutral-600 text-sm">Generating...</div>
  {:else}
    <div class="text-neutral-700 text-sm">Output will appear here</div>
  {/if}
</div>
