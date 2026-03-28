<script lang="ts">
  import { canvas } from "../../../stores/canvas.svelte.js";
  import { generation } from "../../../stores/generation.svelte.js";
  import { progress } from "../../../stores/progress.svelte.js";
  import { locale } from "../../../stores/locale.svelte.js";

  const MAX_STAGE_PIXELS = 1024 * 1024;

  async function normalizeStagedImage(sourceUrl: string): Promise<{ url: string; width: number; height: number }> {
    const response = await fetch(sourceUrl);
    const blob = await response.blob();
    const tempUrl = URL.createObjectURL(blob);

    const dims = await new Promise<{ width: number; height: number }>((resolve, reject) => {
      const img = new Image();
      img.onload = () => resolve({ width: img.naturalWidth, height: img.naturalHeight });
      img.onerror = () => reject(new Error("Failed to decode staged image"));
      img.src = tempUrl;
    });

    const pixels = dims.width * dims.height;
    if (pixels <= MAX_STAGE_PIXELS) {
      return { url: tempUrl, width: dims.width, height: dims.height };
    }

    const scale = Math.sqrt(MAX_STAGE_PIXELS / pixels);
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
          reject(new Error("Failed to create stage resize context"));
          return;
        }
        ctx.imageSmoothingEnabled = true;
        ctx.imageSmoothingQuality = "high";
        ctx.drawImage(img, 0, 0, targetWidth, targetHeight);
        out.toBlob((result) => {
          if (!result) {
            reject(new Error("Failed to encode staged resize"));
            return;
          }
          resolve(result);
        }, "image/png");
      };
      img.onerror = () => reject(new Error("Failed to load staged source"));
      img.src = tempUrl;
    });

    URL.revokeObjectURL(tempUrl);
    return {
      url: URL.createObjectURL(resizedBlob),
      width: targetWidth,
      height: targetHeight,
    };
  }

  async function stageLatestOutput() {
    if (!progress.lastOutputImage) return;
    try {
      const normalized = await normalizeStagedImage(progress.lastOutputImage);
      canvas.stageImage(normalized.url);
      generation.width = normalized.width;
      generation.height = normalized.height;

      if (canvas.isCanvasMode && (canvas.canvasWidth !== normalized.width || canvas.canvasHeight !== normalized.height)) {
        canvas.initCanvas(normalized.width, normalized.height);
      }
    } catch (e) {
      console.error("Failed to stage latest output:", e);
    }
  }
</script>

<div class="border-t border-neutral-800 bg-neutral-900/70 px-3 py-2">
  <div class="flex items-center gap-2">
    <button
      onclick={stageLatestOutput}
      disabled={!progress.lastOutputImage}
      class="text-[11px] px-2 py-1 rounded border transition-colors {progress.lastOutputImage
        ? 'border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300'
        : 'border-neutral-800 text-neutral-600 cursor-not-allowed'}"
      title={locale.t('canvas.stage_latest_title')}
    >
      {locale.t('canvas.stage_latest')}
    </button>

    {#if canvas.isStagingActive && canvas.currentStagingImage}
      <button
        onclick={() => canvas.prevStaging()}
        class="w-6 h-6 rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800"
        title={locale.t('canvas.prev_staged')}
      >
        &lt;
      </button>

      <img
        src={canvas.currentStagingImage}
        alt="Staged"
        class="w-10 h-10 rounded border border-neutral-700 object-cover"
      />

      <button
        onclick={() => canvas.nextStaging()}
        class="w-6 h-6 rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800"
        title={locale.t('canvas.next_staged')}
      >
        &gt;
      </button>

      <button
        onclick={() => canvas.dismissCurrentStaging()}
        class="text-[11px] px-2 py-1 rounded border border-neutral-700 text-neutral-300 hover:border-red-500 hover:text-red-300"
        title={locale.t('canvas.dismiss_title')}
      >
        {locale.t('canvas.dismiss')}
      </button>

      <button
        onclick={() => canvas.clearStaging()}
        class="text-[11px] px-2 py-1 rounded border border-neutral-700 text-neutral-300 hover:border-neutral-500"
        title={locale.t('canvas.clear_all_title')}
      >
        {locale.t('canvas.clear_all')}
      </button>

      <span class="text-[11px] text-neutral-500 ml-auto">
        {locale.t('canvas.staging_count', { current: String(canvas.stagingIndex + 1), total: String(canvas.stagingImages.length) })}
      </span>
    {:else}
      <span class="text-[11px] text-neutral-500">{locale.t('canvas.no_staged')}</span>
    {/if}
  </div>
</div>
