<script lang="ts">
  import { progress } from "../../stores/progress.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { generate } from "../../utils/api.js";
  import type { GenerationParams } from "../../types/index.js";

  async function upscaleImage() {
    generation.upscaleEnabled = true;
    if (progress.lastOutputImage) {
      generation.inputImage = progress.lastOutputImage;
    }
    const params = generation.toParams() as GenerationParams;
    params.mode = "img2img";
    try {
      const promptId = await generate(params);
      progress.startGeneration(promptId, true, "img2img");
    } catch (e) {
      console.error("Upscale failed:", e);
    }
  }

  $effect(() => {
    progress.setActiveMode(generation.mode);
  });

  function handleSave() {
    if (progress.lastOutputImage) {
      gallery.saveBlobAs(progress.lastOutputImage, "output.png");
    }
  }

  function handleCopy() {
    if (progress.lastOutputImage) {
      gallery.copyBlobToClipboard(progress.lastOutputImage);
    }
  }

  function inpaintImage() {
    if (!progress.lastOutputImage) return;
    generation.mode = "inpainting";
    generation.inputImage = progress.lastOutputImage;
    generation.maskImage = null;
  }

  async function handleDelete() {
    if (!progress.lastOutputImage) return;
    const currentUrl = progress.lastOutputImage;
    const image = gallery.sessionImages.find((i) => i.url === currentUrl)
      ?? gallery.images.find((i) => i.url === currentUrl);

    if (image) {
      await gallery.deleteImage(image);
    }

    if (progress.lastOutputImage === currentUrl) {
      progress.setLastOutputForMode(progress.currentMode, null);
      progress.previewImage = null;
    }
  }
</script>

<div class="relative w-full aspect-square bg-neutral-900 rounded-xl border border-neutral-800 flex items-center justify-center overflow-hidden group">
  {#if progress.displayImage}
    <button
      class="w-full h-full cursor-pointer"
      onclick={() => {
        if (progress.displayImage) gallery.openLightboxUrl(progress.displayImage);
      }}
    >
      <img
        src={progress.displayImage}
        alt="Preview"
        class="w-full h-full object-contain"
      />
    </button>
    {#if !progress.isGenerating && progress.lastOutputImage}
      <div class="absolute top-3 right-3 flex gap-2 opacity-0 group-hover:opacity-100 transition-opacity">
        {#if !progress.wasUpscaled}
          <button
            onclick={upscaleImage}
            class="flex items-center gap-1.5 bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-medium px-3 py-1.5 rounded-lg shadow-lg transition-colors"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 20 20" fill="currentColor">
              <path fill-rule="evenodd" d="M3 4a1 1 0 011-1h4a1 1 0 010 2H6.414l2.293 2.293a1 1 0 01-1.414 1.414L5 6.414V8a1 1 0 01-2 0V4zm9 1a1 1 0 110-2h4a1 1 0 011 1v4a1 1 0 11-2 0V6.414l-2.293 2.293a1 1 0 11-1.414-1.414L13.586 5H12zm-9 7a1 1 0 112 0v1.586l2.293-2.293a1 1 0 011.414 1.414L6.414 15H8a1 1 0 110 2H4a1 1 0 01-1-1v-4zm13 3a1 1 0 01-1 1h-4a1 1 0 110-2h1.586l-2.293-2.293a1 1 0 011.414-1.414L15 13.586V12a1 1 0 112 0v4z" clip-rule="evenodd"/>
            </svg>
            Upscale
          </button>
        {/if}
        <button
          onclick={inpaintImage}
          class="flex items-center gap-1.5 bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-medium px-3 py-1.5 rounded-lg shadow-lg transition-colors"
          title="Use as inpaint input"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/></svg>
          Inpaint
        </button>
        <button
          onclick={handleSave}
          class="flex items-center gap-1.5 bg-neutral-700 hover:bg-neutral-600 text-white text-xs font-medium px-3 py-1.5 rounded-lg shadow-lg transition-colors"
          title="Save As"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
          Save
        </button>
        <button
          onclick={handleCopy}
          class="flex items-center gap-1.5 bg-neutral-700 hover:bg-neutral-600 text-white text-xs font-medium px-3 py-1.5 rounded-lg shadow-lg transition-colors"
          title="Copy to clipboard"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
          Copy
        </button>
        <button
          onclick={handleDelete}
          class="flex items-center gap-1.5 bg-red-700 hover:bg-red-600 text-white text-xs font-medium px-3 py-1.5 rounded-lg shadow-lg transition-colors"
          title="Delete image"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
          Delete
        </button>
      </div>
    {/if}
  {:else if progress.isGenerating}
    <div class="text-neutral-600 text-sm">Generating...</div>
  {:else}
    <div class="text-neutral-700 text-sm">Output will appear here</div>
  {/if}
</div>
