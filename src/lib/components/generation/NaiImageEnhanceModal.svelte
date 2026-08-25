<script lang="ts">
  /**
   * The NovelAI image Enhance form, in one app-wide modal.
   *
   * App-wide for the same reason the Director Tools modal is: it acts on an
   * image the user is already looking at (a lightbox view, a gallery entry),
   * and none of those sit next to the generation settings.
   *
   * Enhance is not its own endpoint. It is an img2img pass at a larger canvas,
   * so this submits through `requestGeneration` exactly as the Generate button
   * does, and the prompt, undesired content and characters it sends are the
   * ones already typed into the generation panel. There is deliberately no
   * prompt field here: that panel *is* the enhance prompt box, and a second
   * copy would only ever disagree with it.
   *
   * Nothing here waits for the result. The backend reports through the same
   * synthetic prompt id and `comfyui:*` events a NovelAI generation uses, so
   * once the id is handed to `progress.enqueue` the image arrives in the
   * session grid and the gallery on its own.
   */
  import { gallery } from "../../stores/gallery.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { naiImageEnhance } from "../../stores/naiImageEnhance.svelte.js";
  import { novelai } from "../../stores/novelai.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { requestGeneration } from "../../utils/generationSubmit.js";
  import {
    imageBytesToBase64,
    loadOutputImageForGenerationInput,
  } from "../../utils/galleryActions.js";
  import { estimateNovelAiCost } from "../../utils/novelaiCost.js";
  import { MAGNITUDE_MAX, MAGNITUDE_MIN } from "../../utils/novelaiEnhance.js";
  import type { OutputImage } from "../../types/index.js";

  /**
   * The source the load below was started for.
   *
   * The effect fires on any read of `naiImageEnhance.source`, so without this a
   * field the modal writes during the load would restart the load it is part
   * of. Compared by identity, which is enough: `open()` always hands over the
   * gallery's own object.
   */
  let loadedFor: OutputImage | null = null;

  $effect(() => {
    const source = naiImageEnhance.source;
    if (!source) {
      loadedFor = null;
      return;
    }
    if (source === loadedFor) return;
    loadedFor = source;
    void loadSource(source);
  });

  // Populates the subscription so the Opus allowance is known before the first
  // quote is drawn, rather than the price visibly correcting itself after.
  $effect(() => {
    if (naiImageEnhance.isOpen && novelai.apiKeyConfigured) void novelai.ensureSubscription();
  });

  /**
   * Read the source's bytes and its true pixel size.
   *
   * Both come from one decode. `metadata.size` records what was *requested* of
   * the generator, which an already-upscaled or imported image no longer
   * matches, and every number this modal shows is derived from the real size.
   * Holding the base64 also means the submit below spends no time transcoding.
   */
  async function loadSource(source: OutputImage) {
    naiImageEnhance.loadingSource = true;
    naiImageEnhance.error = null;
    try {
      // The same loader img2img and inpainting use, so a JXL gallery entry is
      // decoded to PNG here rather than being read off disk raw.
      const { bytes } = await loadOutputImageForGenerationInput(source, "enhance_input.png");
      const bitmap = await createImageBitmap(new Blob([new Uint8Array(bytes)]));
      // The modal may have been closed or pointed at another image while the
      // decode was in flight; writing then would quote this image's size next
      // to that one's preview.
      if (naiImageEnhance.source !== source) {
        bitmap.close();
        return;
      }
      naiImageEnhance.sourceWidth = bitmap.width;
      naiImageEnhance.sourceHeight = bitmap.height;
      naiImageEnhance.imageBase64 = imageBytesToBase64(bytes);
      bitmap.close();
    } catch (e) {
      console.error("Failed to read the image to enhance:", e);
      if (naiImageEnhance.source === source) naiImageEnhance.error = locale.t("novelai.enhance.read_failed");
    } finally {
      if (naiImageEnhance.source === source) naiImageEnhance.loadingSource = false;
    }
  }

  function sizeLabel(size: { width: number; height: number }): string {
    return locale.t("novelai.enhance.size", {
      width: String(size.width),
      height: String(size.height),
    });
  }

  /**
   * Anlas for a canvas, at the denoise the request will actually carry.
   *
   * Strength is part of the price: NovelAI charges an img2img pass for the
   * steps it samples, so moving magnitude moves this number too. Batch is
   * fixed at one here because that is what the request sends.
   */
  function costFor(size: { width: number; height: number }, strength: number): number {
    if (!(size.width > 0 && size.height > 0)) return 0;
    return estimateNovelAiCost({
      width: size.width,
      height: size.height,
      steps: generation.steps,
      nSamples: 1,
      strength,
      isOpus: novelai.isOpus,
      vibeEncodes: generation.novelaiSettings.vibes.filter((v) => !v.encoding).length,
    });
  }

  function costLabel(anlas: number): string {
    return anlas === 0
      ? locale.t("novelai.enhance.free")
      : locale.t("novelai.enhance.cost", { anlas: String(anlas) });
  }

  const denoise = $derived(naiImageEnhance.denoise);
  const oneXCost = $derived(costFor(naiImageEnhance.oneXSize, denoise.strength));
  const maxCost = $derived(costFor(naiImageEnhance.maxSize, denoise.strength));
  const totalCost = $derived(costFor(naiImageEnhance.targetSize, denoise.strength));

  /** Integer position for the slider; the box beside it keeps the decimals. */
  const magnitudeStep = $derived(Math.round(naiImageEnhance.magnitude));

  function onSliderInput(e: Event) {
    naiImageEnhance.setMagnitude(Number((e.currentTarget as HTMLInputElement).value));
  }

  function onMagnitudeInput(e: Event) {
    const raw = (e.currentTarget as HTMLInputElement).value;
    // Left alone while the box is empty or mid-edit ("1."): clamping on every
    // keystroke would fight a user typing "1.25" one character at a time. The
    // blur handler below settles whatever they end up with.
    if (raw.trim() === "") return;
    const value = Number(raw);
    if (Number.isFinite(value)) naiImageEnhance.magnitude = value;
  }

  function onMagnitudeBlur() {
    naiImageEnhance.setMagnitude(naiImageEnhance.magnitude);
  }

  async function run() {
    const image = naiImageEnhance.imageBase64;
    if (!image || !naiImageEnhance.canRun) return;
    naiImageEnhance.busy = true;
    naiImageEnhance.error = null;
    try {
      const target = naiImageEnhance.targetSize;
      const { strength, noise } = naiImageEnhance.denoise;
      // The whole generation panel, prompt and characters included, then the
      // handful of fields an enhance overrides. `toParams`'s override map does
      // not reach size, batch or the nested NovelAI block, so those are set on
      // the built object.
      const params = generation.toParams();
      // "image_edit" and not "img2img": the result is a pass over an existing
      // image, and tagging it img2img would overwrite that tab's last output.
      params.mode = "image_edit";
      params.input_image = image;
      params.mask_image = null;
      params.width = target.width;
      params.height = target.height;
      // One at a time. Anything else multiplies an already large canvas by the
      // batch, and the Opus allowance only ever covers a single sample.
      params.batch_size = 1;
      // No second pass on top of a paid enhance: a local upscale would resample
      // the very detail that was just bought.
      params.upscale_enabled = false;
      if (params.novelai) {
        params.novelai.action = "img2img";
        params.novelai.strength = strength;
        params.novelai.noise = noise;
        params.novelai.local_post_process = false;
      }
      const result = await requestGeneration(params);
      progress.enqueue(result.prompt_id, false, "image_edit", params);
      naiImageEnhance.dismiss();
      gallery.showToast(locale.t("novelai.enhance.started"), "success");
    } catch (e) {
      console.error("NovelAI enhance failed:", e);
      naiImageEnhance.error = String(e);
    } finally {
      naiImageEnhance.busy = false;
    }
  }

  function close() {
    naiImageEnhance.dismiss();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }

  function onFieldKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      run();
    }
  }
</script>

<svelte:window onkeydown={naiImageEnhance.isOpen ? onKeydown : undefined} />

{#if naiImageEnhance.isOpen}
  <div
    class="fixed inset-0 z-70 flex items-center justify-center bg-black/70 p-4 sm:p-8"
    onclick={(e) => {
      if (e.currentTarget === e.target) close();
    }}
    role="presentation"
  >
    <div
      class="flex max-h-[90vh] w-full max-w-[560px] flex-col overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-6 shadow-2xl sm:p-7"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      aria-label={locale.t("novelai.enhance.title")}
    >
      <div class="mb-4 flex items-start justify-between gap-3">
        <div>
          <h2 class="text-lg font-semibold text-neutral-100">
            {locale.t("novelai.enhance.title")}
          </h2>
          <p class="mt-1 text-sm text-neutral-400">
            {locale.t("novelai.enhance.subtitle")}
          </p>
        </div>
        <!-- Sized to a comfortable pointer target rather than to the glyph: this
             is the only close affordance besides Escape and the backdrop. -->
        <button
          class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-neutral-600 text-base text-neutral-300 hover:bg-neutral-800"
          aria-label={locale.t("common.cancel")}
          onclick={close}
        >
          ✕
        </button>
      </div>

      <div class="flex gap-4">
        {#if naiImageEnhance.previewUrl}
          <img
            src={naiImageEnhance.previewUrl}
            alt={locale.t("novelai.enhance.source")}
            class="h-24 w-24 shrink-0 self-start rounded-lg border border-neutral-700 object-cover"
          />
        {/if}
        <div class="min-w-0 flex-1">
          <span class="text-sm font-medium text-neutral-300">
            {locale.t("novelai.enhance.upscale_amount")}
          </span>
          {#if naiImageEnhance.loadingSource}
            <!-- Every button below is labelled with a resolution and a price,
                 and neither is knowable until the source has been decoded. -->
            <p class="mt-1.5 text-sm text-neutral-500">
              {locale.t("novelai.enhance.reading")}
            </p>
          {:else if naiImageEnhance.hasSourceSize}
            <div class="mt-1.5 flex flex-wrap gap-2">
              <button
                class="flex-1 rounded-lg border px-3 py-2 text-left text-sm transition-colors {naiImageEnhance.scaleChoice ===
                '1x'
                  ? 'border-indigo-500/60 bg-neutral-800/60 text-neutral-100'
                  : 'border-neutral-700 bg-neutral-950 text-neutral-300 hover:bg-neutral-800'}"
                disabled={naiImageEnhance.busy}
                onclick={() => (naiImageEnhance.scaleChoice = "1x")}
              >
                <span class="block font-medium">{locale.t("novelai.enhance.scale_1x")}</span>
                <span class="mt-0.5 block text-xs tabular-nums text-neutral-500">
                  {sizeLabel(naiImageEnhance.oneXSize)}
                </span>
                <span class="mt-0.5 block text-xs tabular-nums text-neutral-400">
                  {costLabel(oneXCost)}
                </span>
              </button>
              <!-- Hidden when the source is already at or near the 3MP ceiling:
                   the button would quote the same resolution as 1x, and an
                   enhance that does not enlarge is not a choice worth offering. -->
              {#if naiImageEnhance.maxScaleAvailable}
                <button
                  class="flex-1 rounded-lg border px-3 py-2 text-left text-sm transition-colors {naiImageEnhance.scaleChoice ===
                  'max'
                    ? 'border-indigo-500/60 bg-neutral-800/60 text-neutral-100'
                    : 'border-neutral-700 bg-neutral-950 text-neutral-300 hover:bg-neutral-800'}"
                  disabled={naiImageEnhance.busy}
                  onclick={() => (naiImageEnhance.scaleChoice = "max")}
                >
                  <span class="block font-medium">{locale.t("novelai.enhance.scale_max")}</span>
                  <span class="mt-0.5 block text-xs tabular-nums text-neutral-500">
                    {sizeLabel(naiImageEnhance.maxSize)}
                  </span>
                  <span class="mt-0.5 block text-xs tabular-nums text-neutral-400">
                    {costLabel(maxCost)}
                  </span>
                </button>
              {/if}
            </div>
          {/if}
        </div>
      </div>

      {#if !naiImageEnhance.showAdvanced}
        <div class="mt-4">
          <div class="flex items-center justify-between">
            <span class="text-sm font-medium text-neutral-300">
              {locale.t("novelai.enhance.magnitude")}
            </span>
            <!-- A number box beside the slider, not instead of it: the slider
                 steps in whole magnitudes because that is the scale NovelAI's
                 own panel works in, and anything between them is typed. -->
            <input
              type="number"
              min={MAGNITUDE_MIN}
              max={MAGNITUDE_MAX}
              step="0.01"
              class="w-20 rounded-lg border border-neutral-700 bg-neutral-950 px-2 py-1 text-right text-sm tabular-nums text-neutral-100 focus:border-indigo-500 focus:outline-none"
              aria-label={locale.t("novelai.enhance.magnitude")}
              disabled={naiImageEnhance.busy}
              value={naiImageEnhance.magnitude}
              oninput={onMagnitudeInput}
              onblur={onMagnitudeBlur}
              onkeydown={onFieldKeydown}
            />
          </div>
          <!-- Not bound: assigning 1.25 to a step-1 range snaps it back to 1 and
               would silently throw away a typed decimal. The thumb shows the
               nearest whole magnitude; the box holds the exact value. -->
          <input
            type="range"
            min={MAGNITUDE_MIN}
            max={MAGNITUDE_MAX}
            step="1"
            class="mt-1.5 w-full accent-indigo-500"
            aria-label={locale.t("novelai.enhance.magnitude")}
            disabled={naiImageEnhance.busy}
            value={magnitudeStep}
            oninput={onSliderInput}
          />
          <span class="mt-1.5 block text-xs text-neutral-500">
            {locale.t("novelai.enhance.magnitude_hint")}
          </span>
        </div>
      {:else}
        <div class="mt-4">
          <div class="flex items-center justify-between">
            <span class="text-sm font-medium text-neutral-300">
              {locale.t("novelai.enhance.strength")}
            </span>
            <span class="tabular-nums text-sm text-neutral-400">
              {naiImageEnhance.strength.toFixed(2)}
            </span>
          </div>
          <input
            type="range"
            min="0.01"
            max="0.99"
            step="0.01"
            class="mt-1.5 w-full accent-indigo-500"
            aria-label={locale.t("novelai.enhance.strength")}
            disabled={naiImageEnhance.busy}
            bind:value={naiImageEnhance.strength}
          />
          <span class="mt-1.5 block text-xs text-neutral-500">
            {locale.t("novelai.enhance.strength_hint")}
          </span>
        </div>

        <div class="mt-4">
          <div class="flex items-center justify-between">
            <span class="text-sm font-medium text-neutral-300">
              {locale.t("novelai.enhance.noise")}
            </span>
            <span class="tabular-nums text-sm text-neutral-400">
              {naiImageEnhance.noise.toFixed(2)}
            </span>
          </div>
          <input
            type="range"
            min="0"
            max="0.99"
            step="0.01"
            class="mt-1.5 w-full accent-indigo-500"
            aria-label={locale.t("novelai.enhance.noise")}
            disabled={naiImageEnhance.busy}
            bind:value={naiImageEnhance.noise}
          />
          <span class="mt-1.5 block text-xs text-neutral-500">
            {locale.t("novelai.enhance.noise_hint")}
          </span>
        </div>
      {/if}

      <button
        class="mt-3 self-start text-xs font-medium text-indigo-400 hover:text-indigo-300"
        disabled={naiImageEnhance.busy}
        onclick={() => naiImageEnhance.toggleAdvanced()}
      >
        {naiImageEnhance.showAdvanced
          ? locale.t("novelai.enhance.hide_advanced")
          : locale.t("novelai.enhance.show_advanced")}
      </button>

      <!-- Stated rather than duplicated. The generation panel's prompt boxes are
           the enhance's prompt boxes, and a user who wants to add to the
           enhance types it there. -->
      <p
        class="mt-4 rounded-lg border border-neutral-700 bg-neutral-950 px-3 py-2 text-xs text-neutral-400"
      >
        {locale.t("novelai.enhance.prompt_note")}
      </p>

      {#if naiImageEnhance.error}
        <div class="mt-4 rounded-lg border border-red-500/40 bg-red-500/10 p-3 text-sm text-red-200">
          {naiImageEnhance.error}
        </div>
      {/if}

      <div class="mt-5 flex items-center justify-between gap-2">
        <span class="text-sm tabular-nums text-neutral-400">
          {#if naiImageEnhance.hasSourceSize}
            {sizeLabel(naiImageEnhance.targetSize)} &middot; {costLabel(totalCost)}
          {/if}
        </span>
        <div class="flex items-center gap-2">
          <button
            class="rounded-lg border border-neutral-600 px-4 py-2 text-sm text-neutral-300 hover:bg-neutral-800"
            onclick={close}
          >
            {locale.t("common.cancel")}
          </button>
          <button
            class="rounded-lg bg-indigo-600 px-4 py-2 text-sm text-white hover:bg-indigo-500 disabled:opacity-40"
            disabled={!naiImageEnhance.canRun}
            onclick={run}
          >
            {#if naiImageEnhance.busy}
              <span class="inline-block animate-spin">⟳</span>
              {locale.t("novelai.enhance.running")}
            {:else}
              {locale.t("novelai.enhance.run")}
            {/if}
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}
