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
  import CanvasEditor from "../canvas/CanvasEditor.svelte";
  import LayerPanel from "../canvas/layers/LayerPanel.svelte";
  import { canvas } from "../../stores/canvas.svelte.js";
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

  let canvasEditorRef: CanvasEditor | undefined = $state();
  let imagePreviewUrl = $state<string | null>(null);
  let maskPreviewUrl = $state<string | null>(null);
  let uploading = $state(false);
  let imageAspect = $state<{ w: number; h: number } | null>(null);
  let dragOver = $state(false);
  let maskDragOver = $state(false);
  let promptsSectionOpen = $state(true);
  let dimensionsSectionOpen = $state(true);
  let imageSectionOpen = $state(true);
  let layersSectionOpen = $state(true);
  let controlsSectionOpen = $state(true);
  let modelSectionOpen = $state(true);
  let postSectionOpen = $state(true);

  const MAX_INPUT_PIXELS = 1024 * 1024;

  function applyImageGeometry(width: number, height: number) {
    imageAspect = { w: width, h: height };
    generation.width = width;
    generation.height = height;

    if (canvas.isCanvasMode && (canvas.canvasWidth !== width || canvas.canvasHeight !== height)) {
      canvas.initCanvas(width, height);
    }
  }

  async function normalizeImageBytes(
    imageBytes: number[],
    fallbackFilename: string
  ): Promise<{ bytes: number[]; previewUrl: string; width: number; height: number; filename: string }> {
    const sourceBlob = new Blob([new Uint8Array(imageBytes)], { type: "image/png" });
    const sourceUrl = URL.createObjectURL(sourceBlob);

    const dims = await new Promise<{ width: number; height: number }>((resolve, reject) => {
      const img = new Image();
      img.onload = () => resolve({ width: img.naturalWidth, height: img.naturalHeight });
      img.onerror = () => reject(new Error("Failed to read image dimensions"));
      img.src = sourceUrl;
    });

    const sourcePixels = dims.width * dims.height;
    if (sourcePixels <= MAX_INPUT_PIXELS) {
      return {
        bytes: imageBytes,
        previewUrl: sourceUrl,
        width: dims.width,
        height: dims.height,
        filename: fallbackFilename,
      };
    }

    const scale = Math.sqrt(MAX_INPUT_PIXELS / sourcePixels);
    const targetWidth = Math.max(8, Math.round(dims.width * scale));
    const targetHeight = Math.max(8, Math.round(dims.height * scale));

    const resizedBlob = await new Promise<Blob>((resolve, reject) => {
      const img = new Image();
      img.onload = () => {
        const out = document.createElement("canvas");
        out.width = targetWidth;
        out.height = targetHeight;
        const ctx = out.getContext("2d");
        if (!ctx) {
          reject(new Error("Failed to create resize context"));
          return;
        }
        ctx.imageSmoothingEnabled = true;
        ctx.imageSmoothingQuality = "high";
        ctx.drawImage(img, 0, 0, targetWidth, targetHeight);
        out.toBlob((blob) => {
          if (!blob) {
            reject(new Error("Failed to encode resized image"));
            return;
          }
          resolve(blob);
        }, "image/png");
      };
      img.onerror = () => reject(new Error("Failed to decode source image"));
      img.src = sourceUrl;
    });

    URL.revokeObjectURL(sourceUrl);
    const resizedBuffer = await resizedBlob.arrayBuffer();
    const resizedBytes = Array.from(new Uint8Array(resizedBuffer));
    const resizedPreview = URL.createObjectURL(resizedBlob);

    return {
      bytes: resizedBytes,
      previewUrl: resizedPreview,
      width: targetWidth,
      height: targetHeight,
      filename: fallbackFilename,
    };
  }

  function getFilenameFromPath(path: string): string {
    const name = path.split(/[\\/]/).pop() ?? "input.png";
    return name.trim() || "input.png";
  }

  async function browseImage() {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }],
    });
    if (!selected) return;

    uploading = true;
    try {
      const selectedPath = typeof selected === "string" ? selected : selected[0];
      if (!selectedPath) return;

      const bytes = Array.from(await readFile(selectedPath));
      const normalized = await normalizeImageBytes(bytes, getFilenameFromPath(selectedPath));

      if (imagePreviewUrl) URL.revokeObjectURL(imagePreviewUrl);
      imagePreviewUrl = normalized.previewUrl;
      applyImageGeometry(normalized.width, normalized.height);
      canvas.setReferenceImage(imagePreviewUrl);

      // Upload normalized bytes to ComfyUI
      const response = await uploadImageBytes(normalized.bytes, normalized.filename);
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
      canvas.setPersistedMaskPreview(maskPreviewUrl);

      const response = await uploadImage(selected);
      generation.maskImage = response.name;
    } catch (e) {
      console.error("Failed to upload mask:", e);
    } finally {
      uploading = false;
    }
  }

  async function handleImageDrop(e: DragEvent) {
    e.preventDefault();
    dragOver = false;
    const file = e.dataTransfer?.files?.[0];
    if (!file || !file.type.startsWith("image/")) return;

    uploading = true;
    try {
      const buffer = await file.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buffer));
      const normalized = await normalizeImageBytes(bytes, file.name || "dropped_image.png");

      if (imagePreviewUrl) URL.revokeObjectURL(imagePreviewUrl);
      imagePreviewUrl = normalized.previewUrl;
      applyImageGeometry(normalized.width, normalized.height);
      canvas.setReferenceImage(imagePreviewUrl);

      const response = await uploadImageBytes(normalized.bytes, normalized.filename);
      generation.inputImage = response.name;
    } catch (e) {
      console.error("Failed to handle dropped image:", e);
    } finally {
      uploading = false;
    }
  }

  async function handleMaskDrop(e: DragEvent) {
    e.preventDefault();
    maskDragOver = false;
    const file = e.dataTransfer?.files?.[0];
    if (!file || !file.type.startsWith("image/")) return;

    uploading = true;
    try {
      const buffer = await file.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buffer));
      const blob = new Blob([new Uint8Array(bytes)], { type: "image/png" });
      if (maskPreviewUrl) URL.revokeObjectURL(maskPreviewUrl);
      maskPreviewUrl = URL.createObjectURL(blob);
      canvas.setPersistedMaskPreview(maskPreviewUrl);

      const response = await uploadImageBytes(bytes, file.name || "dropped_mask.png");
      generation.maskImage = response.name;
    } catch (e) {
      console.error("Failed to handle dropped mask:", e);
    } finally {
      uploading = false;
    }
  }

  function clearImage() {
    generation.inputImage = null;
    imageAspect = null;
    canvas.setReferenceImage(null);
    if (imagePreviewUrl) {
      URL.revokeObjectURL(imagePreviewUrl);
      imagePreviewUrl = null;
    }
  }

  function clearMask() {
    canvas.clearMask();
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

  async function inpaintImage(image: OutputImage) {
    try {
      let bytes: number[];
      if (image.gallery_filename) {
        bytes = await loadGalleryImage(image.gallery_filename);
      } else {
        bytes = await getOutputImage(image.filename, image.subfolder);
      }

      const normalized = await normalizeImageBytes(bytes, image.filename || "inpaint_input.png");
      const response = await uploadImageBytes(normalized.bytes, normalized.filename);
      generation.inputImage = response.name;
      canvas.clearMask();
      generation.mode = "inpainting";
      canvas.isCanvasMode = true;

      if (imagePreviewUrl) URL.revokeObjectURL(imagePreviewUrl);
      imagePreviewUrl = normalized.previewUrl;
      applyImageGeometry(normalized.width, normalized.height);
      canvas.setReferenceImage(imagePreviewUrl);

      if (canvas.layers.length === 0) {
        canvas.initCanvas(generation.width, generation.height);
      }

      gallery.showToast("Image loaded for inpainting");
    } catch (e) {
      console.error("Failed to set up inpainting:", e);
      gallery.showToast("Failed to load image");
    }
  }

  // Panel widths (px)
  const LEFT_DEFAULT = 405;
  const RIGHT_DEFAULT = 338;
  let leftWidth = $state(LEFT_DEFAULT);
  let rightWidth = $state(RIGHT_DEFAULT);

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

  function resetLeftWidth() {
    leftWidth = LEFT_DEFAULT;
  }

  function resetRightWidth() {
    rightWidth = RIGHT_DEFAULT;
  }

  $effect(() => {
    if (generation.mode !== "inpainting" && canvas.isCanvasMode) {
      canvas.isCanvasMode = false;
    }
  });
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
    class="overflow-y-auto px-4 pt-4 flex flex-col gap-4 shrink-0"
    style="width: {leftWidth}px"
  >
    <!-- Mode tabs -->
    <div class="flex gap-1 bg-neutral-900 rounded-lg p-1">
      {#each modes as mode}
        <button
          onclick={() => {
            generation.mode = mode.id;
            if (mode.id !== "inpainting") canvas.isCanvasMode = false;
          }}
          class="flex-1 text-xs py-1.5 rounded-md transition-colors {generation.mode ===
          mode.id
            ? 'bg-neutral-700 text-white'
            : 'text-neutral-400 hover:text-neutral-200'}"
        >
          {mode.label}
        </button>
      {/each}
    </div>

    <!-- Canvas Editor toggle -->
    {#if generation.mode === "inpainting"}
      <button
        onclick={() => {
          canvas.isCanvasMode = !canvas.isCanvasMode;
          if (canvas.isCanvasMode && canvas.layers.length === 0) {
            canvas.initCanvas(generation.width, generation.height);
          }
        }}
        class="flex items-center justify-between w-full px-3 py-2 rounded-lg text-xs transition-colors {canvas.isCanvasMode
          ? 'bg-indigo-600/20 border border-indigo-500/50 text-indigo-300'
          : 'bg-neutral-800 border border-neutral-700 text-neutral-400 hover:text-neutral-200 hover:border-neutral-600'}"
      >
        <span class="flex items-center gap-2">
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/></svg>
          Canvas Editor
        </span>
        <span class="text-[10px] {canvas.isCanvasMode ? 'text-indigo-400' : 'text-neutral-500'}">
          {canvas.isCanvasMode ? 'ON' : 'OFF'}
        </span>
      </button>
    {/if}

    <div class="rounded-lg border border-neutral-800 bg-neutral-900/40">
      <button
        class="w-full px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
        onclick={() => (promptsSectionOpen = !promptsSectionOpen)}
        title={promptsSectionOpen ? "Collapse Prompts" : "Expand Prompts"}
      >
        <span class="font-medium">Prompts</span>
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {promptsSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
      </button>
      {#if promptsSectionOpen}
        <div class="px-3 pb-3 pt-1">
          <PromptInputs />
        </div>
      {/if}
    </div>

    <div class="rounded-lg border border-neutral-800 bg-neutral-900/40">
      <button
        class="w-full px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
        onclick={() => (dimensionsSectionOpen = !dimensionsSectionOpen)}
        title={dimensionsSectionOpen ? "Collapse Dimensions" : "Expand Dimensions"}
      >
        <span class="font-medium">Dimensions</span>
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {dimensionsSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
      </button>
      {#if dimensionsSectionOpen}
        <div class="px-3 pb-3 pt-1">
          <DimensionControls suggestedAspect={imageAspect} />
        </div>
      {/if}
    </div>

    {#if generation.mode !== "txt2img"}
      <div class="rounded-lg border border-neutral-800 bg-neutral-900/40">
        <button
          class="w-full px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (imageSectionOpen = !imageSectionOpen)}
          title={imageSectionOpen ? "Collapse Image Inputs" : "Expand Image Inputs"}
        >
          <span class="font-medium">Image Inputs</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {imageSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
        {#if imageSectionOpen}
          <div class="px-3 pb-3 pt-1 space-y-3">
        {#if canvas.currentStagingImage}
          <div class="rounded-md border border-amber-700/50 bg-amber-900/20 p-2 flex items-center justify-between gap-2">
            <span class="text-[11px] text-amber-300">Staged image active. Input image controls are disabled.</span>
            <button
              class="px-2 py-1 text-[11px] rounded border border-amber-600/60 text-amber-200 hover:border-amber-400 hover:text-amber-100 transition-colors"
              onclick={() => canvas.dismissCurrentStaging()}
              title="Remove staged image"
            >
              Remove Staged
            </button>
          </div>
        {/if}

        <!-- Input Image -->
        <div class="{canvas.currentStagingImage ? 'opacity-50 pointer-events-none' : ''}">
          <p class="text-xs text-neutral-400 mb-1">Input Image</p>
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
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="w-full bg-neutral-800 border border-dashed rounded-lg p-4 text-sm transition-colors flex flex-col items-center justify-center gap-2 cursor-pointer {dragOver
                ? 'border-indigo-400 bg-indigo-500/10 text-indigo-300'
                : 'border-neutral-600 text-neutral-400 hover:border-indigo-500 hover:text-indigo-400'}"
              onclick={browseImage}
              ondragover={(e) => { e.preventDefault(); dragOver = true; }}
              ondragleave={() => { dragOver = false; }}
              ondrop={handleImageDrop}
              role="button"
              tabindex="0"
            >
              {#if uploading}
                <div class="w-4 h-4 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin"></div>
                Uploading...
              {:else if dragOver}
                <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                Drop image here
              {:else}
                <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                Browse or drop image
              {/if}
            </div>
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
          <div class="rounded-md border border-neutral-800 bg-neutral-900/70 p-2.5">
            <label class="flex items-center justify-between gap-3 text-xs text-neutral-300">
              <span class="leading-tight">Differential Diffusion<InfoTip text="Recommended for v-pred / Anima style models during inpainting unless you are using a CFG++ sampler. Helps preserve source structure while editing masked regions." /></span>
              <input
                type="checkbox"
                bind:checked={generation.differentialDiffusion}
                class="accent-indigo-500 w-4 h-4 shrink-0"
              />
            </label>
          </div>
        {/if}

        {#if generation.mode === "inpainting"}
          <!-- Mask Image -->
          <div>
            <div class="flex items-center justify-between mb-1">
              <p class="text-xs text-neutral-400">Mask Image</p>
              <button
                class="px-2 py-1 text-[10px] rounded border border-neutral-700 text-neutral-300 hover:border-red-500 hover:text-red-300 transition-colors"
                onclick={clearMask}
                title="Remove current mask"
              >
                Remove Mask
              </button>
            </div>
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
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                class="w-full bg-neutral-800 border border-dashed rounded-lg p-4 text-sm transition-colors flex flex-col items-center justify-center gap-2 cursor-pointer {maskDragOver
                  ? 'border-indigo-400 bg-indigo-500/10 text-indigo-300'
                  : 'border-neutral-600 text-neutral-400 hover:border-indigo-500 hover:text-indigo-400'}"
                onclick={browseMask}
                ondragover={(e) => { e.preventDefault(); maskDragOver = true; }}
                ondragleave={() => { maskDragOver = false; }}
                ondrop={handleMaskDrop}
                role="button"
                tabindex="0"
              >
                {#if uploading}
                  <div class="w-4 h-4 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin"></div>
                  Uploading...
                {:else if maskDragOver}
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                  Drop mask here
                {:else}
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                  Browse or drop mask
                {/if}
              </div>
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
      </div>
    {/if}

    <div class="sticky bottom-0 mt-auto border-t border-neutral-800 bg-neutral-950/95 backdrop-blur-sm rounded-t-lg px-3 pt-3 pb-4">
      <h3 class="text-xs text-neutral-400 mb-2 font-medium">Generate</h3>
      <GenerateButton canvasEditorRef={canvasEditorRef} />
    </div>
  </div>

  <!-- Left divider -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="w-1 mx-1 shrink-0 cursor-col-resize hover:bg-indigo-500/40 transition-colors {dragging === 'left' ? 'bg-indigo-500/60' : 'bg-neutral-800'}"
    onmousedown={(e) => onDividerDown("left", e)}
    ondblclick={resetLeftWidth}
    title="Drag to resize, double-click to reset"
  ></div>

  <!-- Center panel: Preview & Output / Canvas Editor -->
  {#if canvas.isCanvasMode}
    <div class="flex-1 min-w-0 flex flex-col overflow-hidden">
      <CanvasEditor bind:this={canvasEditorRef} />
    </div>
  {:else}
    <div class="flex-1 min-w-0 p-6 flex flex-col gap-4 overflow-y-auto">
      <ProgressBar />
      <PreviewImage />
    </div>
  {/if}

  <!-- Right divider -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="w-1 mx-1 shrink-0 cursor-col-resize hover:bg-indigo-500/40 transition-colors {dragging === 'right' ? 'bg-indigo-500/60' : 'bg-neutral-800'}"
    onmousedown={(e) => onDividerDown("right", e)}
    ondblclick={resetRightWidth}
    title="Drag to resize, double-click to reset"
  ></div>

  <!-- Right panel: Model & Sampler Settings -->
  <div
    class="overflow-y-auto p-4 space-y-4 shrink-0"
    style="width: {rightWidth}px"
  >
    {#if generation.mode === "inpainting"}
      <div class="rounded-lg border border-neutral-800 bg-neutral-900/40">
        <button
          class="w-full px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (layersSectionOpen = !layersSectionOpen)}
          title={layersSectionOpen ? "Collapse Inpainting & Layers" : "Expand Inpainting & Layers"}
        >
          <span class="font-medium">Inpainting & Layers</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {layersSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
        {#if layersSectionOpen}
          <div class="px-3 pb-3 pt-1 space-y-2">
            <div class="grid grid-cols-2 gap-1">
              <button
                onclick={() => canvas.setInpaintDrawMode("mask")}
                class="px-2 py-1 text-[10px] rounded border transition-colors {canvas.inpaintDrawMode === 'mask'
                  ? 'border-indigo-500 text-indigo-300 bg-indigo-500/10'
                  : 'border-neutral-700 text-neutral-400 hover:border-neutral-500 hover:text-neutral-200'}"
                title="Inpaint Mask Mode"
              >
                Inpaint Mask
              </button>
              <button
                onclick={() => canvas.setInpaintDrawMode("regular")}
                class="px-2 py-1 text-[10px] rounded border transition-colors {canvas.inpaintDrawMode === 'regular'
                  ? 'border-indigo-500 text-indigo-300 bg-indigo-500/10'
                  : 'border-neutral-700 text-neutral-400 hover:border-neutral-500 hover:text-neutral-200'}"
                title="Regular Inpaint Mode"
              >
                Regular Inpaint
              </button>
            </div>

            {#if canvas.isCanvasMode}
              <LayerPanel />
            {:else}
              <div class="space-y-2">
                <p class="text-[11px] text-neutral-500">Canvas editor is off. Enable it to manage layers.</p>
                <button
                  onclick={() => {
                    canvas.isCanvasMode = true;
                    if (canvas.layers.length === 0) {
                      canvas.initCanvas(generation.width, generation.height);
                    }
                  }}
                  class="w-full px-2 py-1.5 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
                  title="Enable canvas editor"
                >
                  Enable Canvas Editor
                </button>
              </div>
            {/if}
          </div>
        {/if}
      </div>

      <div class="rounded-lg border border-neutral-800 bg-neutral-900/40">
        <button
          class="w-full px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (controlsSectionOpen = !controlsSectionOpen)}
          title={controlsSectionOpen ? "Collapse Generation Settings" : "Expand Generation Settings"}
        >
          <span class="font-medium">Generation Settings</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {controlsSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
        {#if controlsSectionOpen}
          <div class="px-3 pb-3 pt-1 space-y-4">
            <ModelSelector />

            <div class="border-t border-neutral-800 pt-4">
              <SamplerSettings />
            </div>

            <div class="border-t border-neutral-800 pt-4">
              <UpscaleSettings />
            </div>

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
                      <div class="absolute inset-0 bg-black/45 opacity-0 group-hover:opacity-100 transition-opacity flex items-center justify-center p-2">
                        <div class="grid grid-cols-3 gap-1.5">
                        <button
                          class="w-8 h-8 flex items-center justify-center rounded bg-indigo-900/70 hover:bg-indigo-700 text-neutral-300 text-xs backdrop-blur-sm"
                          title="Upscale"
                          onclick={(e) => { e.stopPropagation(); upscaleImage(image); }}
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/><line x1="11" y1="8" x2="11" y2="14"/><line x1="8" y1="11" x2="14" y2="11"/></svg>
                        </button>
                        <button
                          class="w-8 h-8 flex items-center justify-center rounded bg-indigo-900/70 hover:bg-indigo-700 text-neutral-300 text-xs backdrop-blur-sm"
                          title="Inpaint"
                          onclick={(e) => { e.stopPropagation(); inpaintImage(image); }}
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/></svg>
                        </button>
                        <button
                          class="w-8 h-8 flex items-center justify-center rounded bg-neutral-900/85 hover:bg-neutral-700 text-neutral-300 text-xs backdrop-blur-sm"
                          title="Save As"
                          onclick={(e) => { e.stopPropagation(); gallery.saveImageAs(image); }}
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                        </button>
                        <button
                          class="w-8 h-8 flex items-center justify-center rounded bg-neutral-900/85 hover:bg-neutral-700 text-neutral-300 text-xs backdrop-blur-sm"
                          title="Copy"
                          onclick={(e) => { e.stopPropagation(); gallery.copyToClipboard(image); }}
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
                        </button>
                        <button
                          class="w-8 h-8 flex items-center justify-center rounded bg-red-900/70 hover:bg-red-800 text-neutral-300 text-xs backdrop-blur-sm"
                          title="Delete"
                          onclick={(e) => { e.stopPropagation(); gallery.deleteImage(image); }}
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
                        </button>
                        </div>
                      </div>
                    </div>
                  {/each}
                </div>
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {:else}
      <div class="rounded-lg border border-neutral-800 bg-neutral-900/40">
        <button
          class="w-full px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (modelSectionOpen = !modelSectionOpen)}
          title={modelSectionOpen ? "Collapse Model & Sampler" : "Expand Model & Sampler"}
        >
          <span class="font-medium">Model & Sampler</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {modelSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
        {#if modelSectionOpen}
          <div class="px-3 pb-3 pt-1 space-y-4">
            <ModelSelector />

            <div class="border-t border-neutral-800 pt-4">
              <SamplerSettings />
            </div>
          </div>
        {/if}
      </div>

      <div class="rounded-lg border border-neutral-800 bg-neutral-900/40">
        <button
          class="w-full px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (postSectionOpen = !postSectionOpen)}
          title={postSectionOpen ? "Collapse Upscale & History" : "Expand Upscale & History"}
        >
          <span class="font-medium">Upscale & History</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {postSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
        {#if postSectionOpen}
          <div class="px-3 pb-3 pt-1 space-y-4">
            <UpscaleSettings />

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
                      <div class="absolute inset-0 bg-black/45 opacity-0 group-hover:opacity-100 transition-opacity flex items-center justify-center p-2">
                        <div class="grid grid-cols-3 gap-1.5">
                        <button
                          class="w-8 h-8 flex items-center justify-center rounded bg-indigo-900/70 hover:bg-indigo-700 text-neutral-300 text-xs backdrop-blur-sm"
                          title="Upscale"
                          onclick={(e) => { e.stopPropagation(); upscaleImage(image); }}
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/><line x1="11" y1="8" x2="11" y2="14"/><line x1="8" y1="11" x2="14" y2="11"/></svg>
                        </button>
                        <button
                          class="w-8 h-8 flex items-center justify-center rounded bg-indigo-900/70 hover:bg-indigo-700 text-neutral-300 text-xs backdrop-blur-sm"
                          title="Inpaint"
                          onclick={(e) => { e.stopPropagation(); inpaintImage(image); }}
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/></svg>
                        </button>
                        <button
                          class="w-8 h-8 flex items-center justify-center rounded bg-neutral-900/85 hover:bg-neutral-700 text-neutral-300 text-xs backdrop-blur-sm"
                          title="Save As"
                          onclick={(e) => { e.stopPropagation(); gallery.saveImageAs(image); }}
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                        </button>
                        <button
                          class="w-8 h-8 flex items-center justify-center rounded bg-neutral-900/85 hover:bg-neutral-700 text-neutral-300 text-xs backdrop-blur-sm"
                          title="Copy"
                          onclick={(e) => { e.stopPropagation(); gallery.copyToClipboard(image); }}
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
                        </button>
                        <button
                          class="w-8 h-8 flex items-center justify-center rounded bg-red-900/70 hover:bg-red-800 text-neutral-300 text-xs backdrop-blur-sm"
                          title="Delete"
                          onclick={(e) => { e.stopPropagation(); gallery.deleteImage(image); }}
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
                        </button>
                        </div>
                      </div>
                    </div>
                  {/each}
                </div>
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>
