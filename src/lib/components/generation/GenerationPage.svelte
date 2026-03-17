<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import PromptInputs from "./PromptInputs.svelte";
  import ModelSelector from "./ModelSelector.svelte";
  import SamplerSettings from "./SamplerSettings.svelte";
  import DimensionControls from "./DimensionControls.svelte";
  import GenerateButton from "./GenerateButton.svelte";
  import UpscaleSettings from "./UpscaleSettings.svelte";
  import InfoTip from "../ui/InfoTip.svelte";
  import ProgressBar from "../progress/ProgressBar.svelte";
  import PreviewImage from "../progress/PreviewImage.svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { readFile } from "@tauri-apps/plugin-fs";
  import { uploadImage, uploadImageBytes, loadGalleryImage, getOutputImage } from "../../utils/api.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import type { OutputImage } from "../../types/index.js";

  const modes = [
    { id: "txt2img" as const, label: "Text to Image" },
    { id: "img2img" as const, label: "Image to Image" },
    { id: "inpainting" as const, label: "Inpainting" },
  ];

  let imagePreviewUrl = $state<string | null>(null);
  let maskPreviewUrl = $state<string | null>(null);
  let uploading = $state(false);
  let imageAspect = $state<{ w: number; h: number } | null>(null);

  function gcd(a: number, b: number): number {
    while (b) { [a, b] = [b, a % b]; }
    return a;
  }

  function simplifyRatio(w: number, h: number): { w: number; h: number } {
    const g = gcd(w, h);
    let sw = w / g;
    let sh = h / g;
    if (sw <= 50 && sh <= 50) return { w: sw, h: sh };
    // Approximate with a scaled-down ratio
    const ratio = w / h;
    sw = Math.round(ratio * 10);
    sh = 10;
    const g2 = gcd(sw, sh);
    return { w: sw / g2, h: sh / g2 };
  }

  function detectImageDimensions(blobUrl: string) {
    const img = new Image();
    img.onload = () => {
      imageAspect = simplifyRatio(img.naturalWidth, img.naturalHeight);
    };
    img.src = blobUrl;
  }

  async function browseImage() {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }],
    });
    if (!selected) return;

    uploading = true;
    try {
      // Read file locally for preview
      const bytes = await readFile(selected);
      const blob = new Blob([bytes], { type: "image/png" });
      if (imagePreviewUrl) URL.revokeObjectURL(imagePreviewUrl);
      imagePreviewUrl = URL.createObjectURL(blob);
      detectImageDimensions(imagePreviewUrl);

      // Upload to ComfyUI
      const response = await uploadImage(selected);
      generation.inputImage = response.name;
    } catch (e) {
      console.error("Failed to upload image:", e);
    } finally {
      uploading = false;
    }
  }

  async function browseMask() {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }],
    });
    if (!selected) return;

    uploading = true;
    try {
      const bytes = await readFile(selected);
      const blob = new Blob([bytes], { type: "image/png" });
      if (maskPreviewUrl) URL.revokeObjectURL(maskPreviewUrl);
      maskPreviewUrl = URL.createObjectURL(blob);

      const response = await uploadImage(selected);
      generation.maskImage = response.name;
    } catch (e) {
      console.error("Failed to upload mask:", e);
    } finally {
      uploading = false;
    }
  }

  function clearImage() {
    generation.inputImage = null;
    imageAspect = null;
    if (imagePreviewUrl) {
      URL.revokeObjectURL(imagePreviewUrl);
      imagePreviewUrl = null;
    }
  }

  function clearMask() {
    generation.maskImage = null;
    if (maskPreviewUrl) {
      URL.revokeObjectURL(maskPreviewUrl);
      maskPreviewUrl = null;
    }
  }

  async function upscaleImage(image: OutputImage) {
    try {
      let bytes: number[];
      if (image.gallery_filename) {
        bytes = await loadGalleryImage(image.gallery_filename);
      } else {
        bytes = await getOutputImage(image.filename, image.subfolder);
      }
      const response = await uploadImageBytes(bytes, image.filename);
      generation.inputImage = response.name;
      generation.mode = "img2img";
      generation.upscaleEnabled = true;
      gallery.showToast("Image loaded for upscaling");
    } catch (e) {
      console.error("Failed to set up upscale:", e);
      gallery.showToast("Failed to load image");
    }
  }

  // Panel widths (px)
  let leftWidth = $state(360);
  let rightWidth = $state(300);

  const LEFT_MIN = 280;
  const LEFT_MAX = 520;
  const RIGHT_MIN = 250;
  const RIGHT_MAX = 450;

  let dragging = $state<"left" | "right" | null>(null);
  let dragStartX = 0;
  let dragStartWidth = 0;

  function onDividerDown(side: "left" | "right", e: MouseEvent) {
    dragging = side;
    dragStartX = e.clientX;
    dragStartWidth = side === "left" ? leftWidth : rightWidth;
    e.preventDefault();
  }

  function onPointerMove(e: MouseEvent) {
    if (!dragging) return;
    const delta = e.clientX - dragStartX;
    if (dragging === "left") {
      leftWidth = Math.min(LEFT_MAX, Math.max(LEFT_MIN, dragStartWidth + delta));
    } else {
      rightWidth = Math.min(RIGHT_MAX, Math.max(RIGHT_MIN, dragStartWidth - delta));
    }
  }

  function onPointerUp() {
    dragging = null;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="flex h-full select-none"
  onmousemove={onPointerMove}
  onmouseup={onPointerUp}
  onmouseleave={onPointerUp}
>
  <!-- Left panel: Image Settings -->
  <div
    class="overflow-y-auto p-4 flex flex-col gap-4 shrink-0"
    style="width: {leftWidth}px"
  >
    <!-- Mode tabs -->
    <div class="flex gap-1 bg-neutral-900 rounded-lg p-1">
      {#each modes as mode}
        <button
          onclick={() => (generation.mode = mode.id)}
          class="flex-1 text-xs py-1.5 rounded-md transition-colors {generation.mode ===
          mode.id
            ? 'bg-neutral-700 text-white'
            : 'text-neutral-400 hover:text-neutral-200'}"
        >
          {mode.label}
        </button>
      {/each}
    </div>

    <PromptInputs />

    <div class="border-t border-neutral-800 pt-4">
      <DimensionControls suggestedAspect={imageAspect} />
    </div>

    {#if generation.mode !== "txt2img"}
      <div class="border-t border-neutral-800 pt-4 space-y-3">
        <!-- Input Image -->
        <div>
          <label class="block text-xs text-neutral-400 mb-1">Input Image</label>
          {#if imagePreviewUrl}
            <div class="relative group">
              <img
                src={imagePreviewUrl}
                alt="Input"
                class="w-full rounded-lg border border-neutral-700 object-contain max-h-40"
              />
              <button
                class="absolute top-1 right-1 w-6 h-6 flex items-center justify-center rounded bg-neutral-900/80 hover:bg-red-800 text-neutral-300 text-xs opacity-0 group-hover:opacity-100 transition-opacity"
                onclick={clearImage}
                title="Remove"
              >
                &times;
              </button>
            </div>
          {:else}
            <button
              class="w-full bg-neutral-800 border border-dashed border-neutral-600 rounded-lg p-4 text-sm text-neutral-400 hover:border-indigo-500 hover:text-indigo-400 transition-colors flex items-center justify-center gap-2"
              onclick={browseImage}
              disabled={uploading}
            >
              {#if uploading}
                <div class="w-4 h-4 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin"></div>
                Uploading...
              {:else}
                <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                Browse Image
              {/if}
            </button>
          {/if}
        </div>

        <!-- Denoise Strength -->
        <div>
          <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
            Denoise Strength<InfoTip text="How much the AI changes the input image. 0 = no change, 1 = completely new image ignoring the input. Lower values (0.3-0.5) keep the original composition, higher values (0.6-0.8) allow more creative freedom." />
            <span class="text-neutral-300">{generation.denoise.toFixed(2)}</span>
          </label>
          <input
            type="range"
            bind:value={generation.denoise}
            min="0"
            max="1"
            step="0.01"
            class="w-full accent-indigo-500"
          />
        </div>

        {#if generation.mode === "inpainting"}
          <!-- Mask Image -->
          <div>
            <label class="block text-xs text-neutral-400 mb-1">Mask Image</label>
            {#if maskPreviewUrl}
              <div class="relative group">
                <img
                  src={maskPreviewUrl}
                  alt="Mask"
                  class="w-full rounded-lg border border-neutral-700 object-contain max-h-40"
                />
                <button
                  class="absolute top-1 right-1 w-6 h-6 flex items-center justify-center rounded bg-neutral-900/80 hover:bg-red-800 text-neutral-300 text-xs opacity-0 group-hover:opacity-100 transition-opacity"
                  onclick={clearMask}
                  title="Remove"
                >
                  &times;
                </button>
              </div>
            {:else}
              <button
                class="w-full bg-neutral-800 border border-dashed border-neutral-600 rounded-lg p-4 text-sm text-neutral-400 hover:border-indigo-500 hover:text-indigo-400 transition-colors flex items-center justify-center gap-2"
                onclick={browseMask}
                disabled={uploading}
              >
                {#if uploading}
                  <div class="w-4 h-4 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin"></div>
                  Uploading...
                {:else}
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                  Browse Mask
                {/if}
              </button>
            {/if}
          </div>

          <!-- Grow Mask By -->
          <div>
            <div class="flex items-center justify-between text-xs mb-0.5">
              <span class="text-neutral-400">Grow Mask By<InfoTip text="Expands the masked area by this many pixels. Helps blend the inpainted region into the surrounding image for seamless results." /></span>
              <span class="text-neutral-300 tabular-nums">{generation.growMaskBy}px</span>
            </div>
            <input
              type="range"
              bind:value={generation.growMaskBy}
              min="0"
              max="64"
              step="1"
              class="w-full accent-indigo-500"
            />
          </div>
        {/if}
      </div>
    {/if}

    <div class="mt-auto pt-4">
      <GenerateButton />
    </div>
  </div>

  <!-- Left divider -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="w-1 shrink-0 cursor-col-resize hover:bg-indigo-500/40 transition-colors {dragging === 'left' ? 'bg-indigo-500/60' : 'bg-neutral-800'}"
    onmousedown={(e) => onDividerDown("left", e)}
  ></div>

  <!-- Center panel: Preview & Output -->
  <div class="flex-1 min-w-0 p-6 flex flex-col gap-4 overflow-y-auto">
    <ProgressBar />
    <PreviewImage />
  </div>

  <!-- Right divider -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="w-1 shrink-0 cursor-col-resize hover:bg-indigo-500/40 transition-colors {dragging === 'right' ? 'bg-indigo-500/60' : 'bg-neutral-800'}"
    onmousedown={(e) => onDividerDown("right", e)}
  ></div>

  <!-- Right panel: Model & Sampler Settings -->
  <div
    class="overflow-y-auto p-4 space-y-4 shrink-0"
    style="width: {rightWidth}px"
  >
    <ModelSelector />

    <div class="border-t border-neutral-800 pt-4">
      <SamplerSettings />
    </div>

    <div class="border-t border-neutral-800 pt-4">
      <UpscaleSettings />
    </div>

    <!-- Session Image History -->
    {#if gallery.sessionImages.length > 0}
      <div class="border-t border-neutral-800 pt-4">
        <h3 class="text-xs text-neutral-400 mb-2">Session History</h3>
        <div class="grid grid-cols-2 gap-2">
          {#each gallery.sessionImages as image}
            <div class="group relative aspect-square rounded-lg overflow-hidden border border-neutral-800 hover:border-indigo-500 transition-colors">
              <button
                class="w-full h-full"
                onclick={() => gallery.openLightbox(image)}
              >
                {#if image.url}
                  <img
                    src={image.url}
                    alt={image.filename}
                    class="w-full h-full object-cover"
                  />
                {/if}
              </button>
              <!-- Hover actions -->
              <div class="absolute top-1 right-1 flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                <button
                  class="w-6 h-6 flex items-center justify-center rounded bg-indigo-900/60 hover:bg-indigo-700 text-neutral-300 text-xs backdrop-blur-sm"
                  title="Upscale"
                  onclick={(e) => { e.stopPropagation(); upscaleImage(image); }}
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/><line x1="11" y1="8" x2="11" y2="14"/><line x1="8" y1="11" x2="14" y2="11"/></svg>
                </button>
                <button
                  class="w-6 h-6 flex items-center justify-center rounded bg-neutral-900/80 hover:bg-neutral-700 text-neutral-300 text-xs backdrop-blur-sm"
                  title="Save As"
                  onclick={(e) => { e.stopPropagation(); gallery.saveImageAs(image); }}
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                </button>
                <button
                  class="w-6 h-6 flex items-center justify-center rounded bg-neutral-900/80 hover:bg-neutral-700 text-neutral-300 text-xs backdrop-blur-sm"
                  title="Copy"
                  onclick={(e) => { e.stopPropagation(); gallery.copyToClipboard(image); }}
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
                </button>
                <button
                  class="w-6 h-6 flex items-center justify-center rounded bg-red-900/60 hover:bg-red-800 text-neutral-300 text-xs backdrop-blur-sm"
                  title="Delete"
                  onclick={(e) => { e.stopPropagation(); gallery.deleteImage(image); }}
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
                </button>
              </div>
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</div>
