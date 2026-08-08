<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { rifeInstall } from "../../stores/rifeInstall.svelte.js";
  import { interpolateVideo } from "../../utils/api.js";
  import {
    RIFE_MULTIPLIERS,
    RIFE_SCALE_FACTORS,
    RIFE_MEMORY_WARN_BYTES,
    estimatedPeakBytes,
    formatGigabytes,
    interpolatedFps,
    interpolatedFrameCount,
  } from "../../utils/rife.js";

  interface Props {
    filename: string;
    sourceFps: number;
    sourceWidth: number;
    sourceHeight: number;
    frameCount: number;
    onClose: () => void;
  }

  let { filename, sourceFps, sourceWidth, sourceHeight, frameCount, onClose }: Props = $props();

  // Seeded from the saved defaults but never written back: a one-off 4x pass
  // here must not change what the next generation does.
  let multiplier = $state(generation.videoRifeMultiplier);
  let scaleFactor = $state(generation.videoRifeScaleFactor);
  let fastMode = $state(generation.videoRifeFastMode);
  let ensemble = $state(generation.videoRifeEnsemble);
  let submitting = $state(false);
  let queued = $state(false);
  let errorText = $state<string | null>(null);

  const outFps = $derived(interpolatedFps(sourceFps, multiplier));
  const outFrames = $derived(interpolatedFrameCount(frameCount, multiplier));
  const peakBytes = $derived(estimatedPeakBytes(outFrames, sourceWidth, sourceHeight));
  const heavy = $derived(peakBytes > RIFE_MEMORY_WARN_BYTES);

  $effect(() => {
    void rifeInstall.refresh();
    rifeInstall.listen();
  });

  async function submit() {
    submitting = true;
    errorText = null;
    try {
      await interpolateVideo(filename, multiplier, scaleFactor, fastMode, ensemble);
      queued = true;
    } catch (e) {
      errorText = locale.t("video.interpolate.failed", { error: String(e) });
    } finally {
      submitting = false;
    }
  }
</script>

<div
  class="absolute bottom-full right-0 mb-2 w-80 rounded-xl bg-neutral-900 border border-neutral-700 shadow-xl p-3 flex flex-col gap-3 text-neutral-100"
>
  <div class="flex items-center justify-between">
    <span class="text-sm font-medium">{locale.t("video.interpolate.title")}</span>
    <button class="text-neutral-400 text-xs px-1" onclick={onClose}>x</button>
  </div>

  {#if rifeInstall.installed === false}
    <p class="text-xs text-neutral-400">{locale.t("video.interpolate.needs_install")}</p>
    <button
      class="rounded-lg bg-neutral-800 border border-neutral-700 px-3 py-1.5 text-xs disabled:opacity-50"
      disabled={rifeInstall.installing}
      onclick={() => rifeInstall.install()}
    >
      {rifeInstall.installing ? rifeInstall.message : locale.t("video.interpolate.install")}
    </button>
    {#if rifeInstall.error}
      <p class="text-xs text-red-400">{rifeInstall.error}</p>
    {/if}
  {:else}
    <div class="flex items-center gap-2">
      <span class="text-xs text-neutral-400">{locale.t("generation.video.rife_multiplier")}</span>
      <div class="flex rounded-lg overflow-hidden border border-neutral-700">
        {#each RIFE_MULTIPLIERS as factor (factor)}
          <button
            type="button"
            class="px-2 py-1 text-xs"
            class:bg-neutral-700={multiplier === factor}
            class:text-neutral-400={multiplier !== factor}
            onclick={() => (multiplier = factor)}
          >
            {factor}x
          </button>
        {/each}
      </div>
    </div>

    <p class="text-[11px] text-neutral-400">
      {locale.t("video.interpolate.summary", {
        fromFps: sourceFps.toFixed(0),
        toFps: outFps.toFixed(0),
        fromFrames: String(frameCount),
        toFrames: String(outFrames),
      })}
    </p>
    <p class="text-[11px] text-neutral-500">{locale.t("video.interpolate.duration_note")}</p>

    <details>
      <summary class="text-xs text-neutral-400 cursor-pointer select-none">
        {locale.t("generation.video.rife_advanced")}
      </summary>
      <div class="mt-2 flex flex-col gap-2">
        <label class="flex items-center gap-2 text-xs text-neutral-400">
          <span>{locale.t("generation.video.rife_scale")}</span>
          <select
            class="bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1 text-xs text-neutral-100"
            value={scaleFactor}
            onchange={(e) => (scaleFactor = Number(e.currentTarget.value))}
          >
            {#each RIFE_SCALE_FACTORS as scale (scale)}
              <option value={scale}>{scale}</option>
            {/each}
          </select>
        </label>
        <label class="flex items-center gap-2 text-xs text-neutral-400">
          <input
            type="checkbox"
            class="accent-[var(--theme-accent-500)]"
            bind:checked={fastMode}
          />
          <span>{locale.t("generation.video.rife_fast_mode")}</span>
        </label>
        <label class="flex items-center gap-2 text-xs text-neutral-400">
          <input
            type="checkbox"
            class="accent-[var(--theme-accent-500)]"
            bind:checked={ensemble}
          />
          <span>{locale.t("generation.video.rife_ensemble")}</span>
        </label>
      </div>
    </details>

    {#if heavy}
      <p class="text-[11px] text-amber-400">
        {locale.t("video.interpolate.heavy_warning", { size: formatGigabytes(peakBytes) })}
      </p>
    {/if}

    {#if queued}
      <p class="text-xs text-emerald-400">{locale.t("video.interpolate.queued")}</p>
    {:else}
      <button
        class="rounded-lg bg-[var(--theme-accent-600)] px-3 py-1.5 text-xs disabled:opacity-50"
        disabled={submitting}
        onclick={submit}
      >
        {locale.t("video.interpolate.submit")}
      </button>
    {/if}

    {#if errorText}
      <p class="text-xs text-red-400">{errorText}</p>
    {/if}
  {/if}
</div>
