<script lang="ts">
  import { canvas } from "../../../stores/canvas.svelte.js";
  import { generation } from "../../../stores/generation.svelte.js";
  import { progress } from "../../../stores/progress.svelte.js";
  import { locale } from "../../../stores/locale.svelte.js";
  import { gallery } from "../../../stores/gallery.svelte.js";
  import { uploadImageBytes } from "../../../utils/api.js";
  import { prepareOutputImageForEditMode } from "../../../utils/editImagePreparation.js";
  import type { OutputImage } from "../../../types/index.js";

  const editSessionImages = $derived(
    gallery.sessionImages.filter(
      (image) => image.generation_mode === "img2img" || image.generation_mode === "inpainting",
    ),
  );

  let selectingFilename = $state<string | null>(null);

  async function selectEditSource(image: OutputImage) {
    try {
      selectingFilename = image.filename;
      const prepared = await prepareOutputImageForEditMode(image, "inpainting");
      const normalized = prepared.normalized;
      if (!normalized) return;

      const response = await uploadImageBytes(prepared.uploadBytes, prepared.uploadFilename);
      generation.inputImage = response.name;
      generation.mode = "inpainting";
      progress.setLastOutputForMode("inpainting", null);
      canvas.clearMask();
      canvas.clearStaging();
      if (canvas.hasResettableInpaintSource) {
        canvas.setPreparedInpaintOverride({
          previewUrl: normalized.previewUrl,
          width: normalized.width,
          height: normalized.height,
          uploadedInputName: response.name,
          owned: true,
        });
      } else {
        canvas.setInpaintOriginalSource({
          previewUrl: normalized.previewUrl,
          width: normalized.width,
          height: normalized.height,
          uploadedInputName: response.name,
        });
      }
      generation.width = normalized.width;
      generation.height = normalized.height;
      canvas.isCanvasMode = true;

      if (canvas.layers.length === 0 || canvas.canvasWidth !== normalized.width || canvas.canvasHeight !== normalized.height) {
        canvas.initCanvas(normalized.width, normalized.height);
      }
    } catch (e) {
      console.error("Failed to select edit source:", e);
    } finally {
      selectingFilename = null;
    }
  }
</script>

<div class="border-t border-neutral-800 bg-neutral-900/70 px-3 py-2">
  <div class="flex items-center gap-2 mb-2">
    {#if generation.mode === "inpainting" ? canvas.hasResettableInpaintSource : canvas.currentPreparedInputImage}
      <img
        src={generation.mode === "inpainting" ? canvas.resettableInpaintPreviewImage : canvas.currentPreparedInputImage}
        alt={locale.t("canvas.staged_alt")}
        class="w-10 h-10 rounded border border-neutral-700 object-cover"
      />
      <span class="text-[11px] text-neutral-400">{locale.t('generation.image.staged_active')}</span>
      <div class="ml-auto flex items-center gap-1">
        {#if canvas.canApplyInpaintResult}
          <button
            onclick={() => canvas.applyInpaintResult()}
            class="text-[11px] px-2 py-1 rounded border border-emerald-600 bg-emerald-600/20 text-emerald-200 hover:border-emerald-400 hover:bg-emerald-600/30 hover:text-emerald-100"
            title={locale.t('canvas.apply_inpaint_title')}
          >
            {locale.t('canvas.accept')}
          </button>
        {/if}
        {#if canvas.canUndoInpaintBase}
          <button
            onclick={() => canvas.undoInpaintBase()}
            class="text-[11px] px-2 py-1 rounded border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300"
            title={locale.t('canvas.undo_inpaint')}
          >
            {locale.t('canvas.undo_inpaint')}
          </button>
        {/if}
        <button
          onclick={() => canvas.clearPreparedInputs()}
          class="text-[11px] px-2 py-1 rounded border border-neutral-700 text-neutral-300 hover:border-red-500 hover:text-red-300"
          title={locale.t('canvas.clear_all_title')}
        >
          {locale.t('canvas.clear_all')}
        </button>
      </div>
    {:else}
      <span class="text-[11px] text-neutral-500">{locale.t('canvas.no_staged')}</span>
    {/if}
  </div>

  <div class="flex gap-2 overflow-x-auto">
    {#if editSessionImages.length === 0}
      <span class="text-[11px] text-neutral-500">{locale.t('bottom_panel.no_images')}</span>
    {:else}
      {#each editSessionImages as image}
        <button
          class="shrink-0 w-14 h-14 rounded border overflow-hidden transition-colors {selectingFilename === image.filename
            ? 'border-indigo-400'
            : 'border-neutral-700 hover:border-indigo-500'}"
          onclick={() => void selectEditSource(image)}
          title={image.filename}
        >
          <img src={image.url} alt={image.filename} class="w-full h-full object-cover" />
        </button>
      {/each}
    {/if}
  </div>
</div>
