<script lang="ts">
  import { progress } from "../../stores/progress.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { generate } from "../../utils/api.js";

  let currentTipIndex = $state(0);
  let progressPercent = $state(0);
  let autoPlayInterval: ReturnType<typeof setInterval> | null = null;
  let suppressOpenUntil = 0;

  const TIP_DISPLAY_TIME = 6500; // 6.5 seconds per tip
  const TIP_UPDATE_INTERVAL = 50; // Update progress bar every 50ms

  const baseTips = [
    // Prompt tips
    { category: "Prompts", text: "Clear, specific prompts work better than long ones. Models understand context without repetition." },
    { category: "Prompts", text: "Try reusing prompts from successful images metadata. Consistency beats reinvention." },
    { category: "Prompts", text: "When results disappoint, refine your prompt first before adjusting parameters." },

    // Parameter tips
    { category: "Parameters", text: "Most models work best with CFG 7-10. Higher doesn't mean better - it can degrade quality." },
    { category: "Parameters", text: "The sampler matters: DDIM is fast, Euler is stable, DPM++ is flexible. Experiment by model." },
    { category: "Parameters", text: "Seed lets you iterate. Try small CFG or step changes with the same seed for refinement." },
    { category: "Parameters", text: "Hover over any setting for explanations. No need to memorize what each does." },

    // Workflow tips
    { category: "Workflow", text: "Generate at lower resolution first, then upscale. Saves time and lets you refine results." },
    { category: "Workflow", text: "Your generation settings are saved. They'll be here next time you return." },
    { category: "Workflow", text: "If confused, start simple: one good prompt + default settings beats complexity." },
    { category: "Workflow", text: "Drag an image with metadata onto any section to import its settings, or drop it here to apply all parameters. Ctrl+V works too!" },
  ];

  let tips = $derived(
    generation.autoQualityTags
      ? baseTips
      : [...baseTips, { category: "Quality", text: "Getting blurry or low-quality results? Try re-enabling auto quality tags in Settings > Performance." }]
  );

  function startAutoPlay() {
    progressPercent = 0;
    let elapsedTime = 0;
    autoPlayInterval = setInterval(() => {
      elapsedTime += TIP_UPDATE_INTERVAL;
      progressPercent = (elapsedTime / TIP_DISPLAY_TIME) * 100;
      
      if (elapsedTime >= TIP_DISPLAY_TIME) {
        nextTip();
        elapsedTime = 0;
        progressPercent = 0;
      }
    }, TIP_UPDATE_INTERVAL);
  }

  function stopAutoPlay() {
    if (autoPlayInterval) {
      clearInterval(autoPlayInterval);
      autoPlayInterval = null;
    }
  }

  function resetAutoPlay() {
    stopAutoPlay();
    progressPercent = 0;
    startAutoPlay();
  }

  function nextTip() {
    currentTipIndex = (currentTipIndex + 1) % tips.length;
    resetAutoPlay();
  }

  function prevTip() {
    currentTipIndex = (currentTipIndex - 1 + tips.length) % tips.length;
    resetAutoPlay();
  }

  function handleWheel(e: WheelEvent) {
    e.preventDefault();
    if (e.deltaY > 0) {
      nextTip();
    } else {
      prevTip();
    }
  }

  $effect(() => {
    // Start auto-play when component mounts or when not generating
    if (!progress.isGenerating && !progress.displayImage) {
      startAutoPlay();
    } else {
      stopAutoPlay();
    }
    return () => stopAutoPlay();
  });

  async function upscaleImage() {
    generation.upscaleEnabled = true;
    if (progress.lastOutputImage) {
      generation.inputImage = progress.lastOutputImage;
    }
    const params = generation.toParams();
    params.mode = "img2img";
    try {
      const result = await generate(params);
      params.seed = result.seed;
      progress.enqueue(result.prompt_id, true, "img2img", params);
    } catch (e) {
      console.error("Upscale failed:", e);
    }
  }

  $effect(() => {
    progress.setActiveMode(generation.mode);
  });

  function getSavedImageForUrl(url: string | null) {
    if (!url || url.startsWith("data:image/")) return null;
    return gallery.sessionImages.find((image) => image.url === url) ?? null;
  }

  function getActiveSavedImage() {
    return getSavedImageForUrl(progress.lastOutputImage);
  }

  function openPreviewLightbox() {
    if (Date.now() < suppressOpenUntil) return;

    const url = progress.displayImage;
    if (!url) return;

    const savedImage = getSavedImageForUrl(url);
    if (savedImage) {
      gallery.openLightbox(savedImage);
      return;
    }

    gallery.openLightboxUrl(url);
  }

  function handleSave() {
    const savedImage = getActiveSavedImage();
    if (!savedImage) {
      gallery.showToast("Saved image not available yet", "info");
      return;
    }
    void gallery.saveImageAs(savedImage);
  }

  function handleCopy() {
    const savedImage = getActiveSavedImage();
    if (!savedImage) {
      gallery.showToast("Saved image not available yet", "info");
      return;
    }
    void gallery.copyToClipboard(savedImage);
  }

  function hasFilePayload(e: DragEvent): boolean {
    const dt = e.dataTransfer;
    if (!dt) return false;
    if (dt.files && dt.files.length > 0) return true;
    if (dt.items && Array.from(dt.items).some((item) => item.kind === "file")) return true;
    return false;
  }

  function suppressPreviewOpenOnFileDrop(e: DragEvent) {
    if (!hasFilePayload(e)) return;
    suppressOpenUntil = Date.now() + 500;
  }
</script>

<div class="relative w-full aspect-square bg-white dark:bg-neutral-900 rounded-xl border border-neutral-200 dark:border-neutral-800 flex items-center justify-center overflow-hidden group">
  {#if progress.displayImage}
    <button
      class="w-full h-full cursor-pointer"
      onclick={openPreviewLightbox}
      ondragenter={suppressPreviewOpenOnFileDrop}
      ondragover={suppressPreviewOpenOnFileDrop}
      ondrop={suppressPreviewOpenOnFileDrop}
    >
      <img
        src={progress.displayImage}
        alt="Preview"
        class="w-full h-full object-contain"
      />
    </button>
    {#if !progress.isGenerating && progress.lastOutputImage}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="absolute top-3 right-3 flex gap-2 opacity-0 group-hover:opacity-100 transition-opacity"
        onmousedown={(e) => e.stopPropagation()}
        onclick={(e) => e.stopPropagation()}
      >
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
      </div>
    {/if}
  {:else if progress.isGenerating}
    <div class="text-neutral-400 dark:text-neutral-600 text-sm">Generating...</div>
  {:else}
    <!-- Tips Carousel -->
    <div class="flex flex-col items-center justify-center w-full h-full p-8 gap-6" onwheel={handleWheel}>
      <div class="flex flex-col items-center gap-3 max-w-md w-full">
        <span class="text-xs font-semibold text-indigo-600 dark:text-indigo-500 uppercase tracking-wide">
          {tips[currentTipIndex].category}
        </span>
        <p class="text-neutral-700 dark:text-neutral-300 text-sm text-center leading-relaxed">
          {tips[currentTipIndex].text}
        </p>
        
        <!-- Progress Bar -->
        <div class="w-full h-0.5 bg-neutral-200 dark:bg-neutral-700 rounded-full overflow-hidden mt-2">
          <div 
            class="h-full bg-indigo-600 dark:bg-indigo-500 transition-all ease-linear"
            style="width: {progressPercent}%"
          />
        </div>
      </div>
      
      <!-- Navigation Controls -->
      <div class="flex items-center gap-3">
        <button
          onclick={prevTip}
          class="p-2 rounded-lg bg-neutral-100 dark:bg-neutral-800 hover:bg-neutral-200 dark:hover:bg-neutral-700 text-neutral-600 dark:text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 transition-colors"
          title="Previous tip"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M12.707 5.293a1 1 0 010 1.414L9.414 10l3.293 3.293a1 1 0 01-1.414 1.414l-4-4a1 1 0 010-1.414l4-4a1 1 0 011.414 0z" clip-rule="evenodd"/>
          </svg>
        </button>
        
        <div class="flex gap-1.5">
          {#each tips as _, index}
            <div
              class="w-1.5 h-1.5 rounded-full transition-colors {index === currentTipIndex ? 'bg-indigo-600 dark:bg-indigo-500' : 'bg-neutral-300 dark:bg-neutral-700'}"
            />
          {/each}
        </div>
        
        <button
          onclick={nextTip}
          class="p-2 rounded-lg bg-neutral-100 dark:bg-neutral-800 hover:bg-neutral-200 dark:hover:bg-neutral-700 text-neutral-600 dark:text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 transition-colors"
          title="Next tip"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M7.293 14.707a1 1 0 010-1.414L10.586 10 7.293 6.707a1 1 0 011.414-1.414l4 4a1 1 0 010 1.414l-4 4a1 1 0 01-1.414 0z" clip-rule="evenodd"/>
          </svg>
        </button>
      </div>
      
      <span class="text-xs text-neutral-500 dark:text-neutral-600">
        {currentTipIndex + 1} / {tips.length}
      </span>
    </div>
  {/if}
</div>
