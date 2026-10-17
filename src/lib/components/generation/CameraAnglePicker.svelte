<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { downloadModel } from "../../utils/api.js";
  import { ipcListen } from "../../utils/ipc.js";
  import {
    CAMERA_AZIMUTHS,
    CAMERA_ELEVATIONS,
    CAMERA_DISTANCES,
    MULTI_ANGLE_LORA,
    installedMultiAngleLora,
  } from "../../utils/cameraAngle.js";
  import InfoTip from "../ui/InfoTip.svelte";

  // Azimuth indices laid out as seen from above, front at the top; null is the subject.
  const compass: (number | null)[] = [7, 0, 1, 6, null, 2, 5, 4, 3];
  const elevationKeys = ["low", "eye", "elevated", "high"] as const;
  const distanceKeys = ["close", "medium", "wide"] as const;

  const loraInstalled = $derived(installedMultiAngleLora(models.loras) !== null);

  let downloading = $state(false);
  let downloadError = $state<string | null>(null);
  let dlBytes = $state(0);
  let dlTotal = $state(0);
  const dlPercent = $derived(dlTotal > 0 ? Math.round((dlBytes / dlTotal) * 100) : 0);

  $effect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    ipcListen("download:progress", (event: any) => {
      if (!downloading) return;
      const data = event.payload as { downloaded: number; total: number; done: boolean };
      dlBytes = data.done ? 0 : data.downloaded;
      dlTotal = data.done ? 0 : data.total;
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  async function downloadLora() {
    downloadError = null;
    downloading = true;
    try {
      await downloadModel(MULTI_ANGLE_LORA.url, "loras", MULTI_ANGLE_LORA.filename);
      await models.refresh();
    } catch (e) {
      downloadError = locale.t("generation.image_edit.camera.download_failed", { error: String(e) });
    } finally {
      downloading = false;
      dlBytes = 0;
      dlTotal = 0;
    }
  }

  function set(field: "cameraAzimuth" | "cameraElevation" | "cameraDistance", value: number) {
    generation[field] = value;
    generation.saveSettings();
  }

  function toggle() {
    generation.cameraAngleEnabled = !generation.cameraAngleEnabled;
    generation.saveSettings();
  }

  const segment = (selected: boolean) =>
    `flex-1 rounded-md px-1.5 py-1 text-[11px] transition-colors ${
      selected
        ? "bg-indigo-600 text-white"
        : "bg-neutral-800 text-neutral-300 hover:bg-neutral-700"
    }`;
</script>

<div class="space-y-3 rounded-lg border border-neutral-700/60 bg-neutral-800/30 p-3">
  <div class="flex items-center justify-between">
    <label class="text-xs text-neutral-400">
      {locale.t("generation.image_edit.camera.title")}
      <InfoTip text={locale.t("generation.image_edit.camera.tip")} />
    </label>
    <button
      class="relative w-10 h-5 rounded-full transition-colors {generation.cameraAngleEnabled
        ? 'bg-indigo-600'
        : 'bg-neutral-700'}"
      onclick={toggle}
      role="switch"
      aria-checked={generation.cameraAngleEnabled}
      aria-label={locale.t("generation.image_edit.camera.title")}
    >
      <span
        class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform {generation.cameraAngleEnabled
          ? 'translate-x-5'
          : ''}"
      ></span>
    </button>
  </div>

  {#if generation.cameraAngleEnabled}
    <div>
      <span class="text-xs text-neutral-400 block mb-1.5">
        {locale.t("generation.image_edit.camera.direction")}
      </span>
      <div class="mx-auto w-40">
        <p class="text-center text-[10px] uppercase tracking-wide text-neutral-500 mb-1">
          {locale.t("generation.image_edit.camera.front")}
        </p>
        <div class="grid grid-cols-3 gap-1">
          {#each compass as azimuth, i (i)}
            {#if azimuth === null}
              <div class="flex items-center justify-center">
                <div class="h-6 w-6 rounded-full border border-neutral-500 bg-neutral-700"></div>
              </div>
            {:else}
              <button
                type="button"
                class="h-9 rounded-md text-[11px] tabular-nums transition-colors {generation.cameraAzimuth ===
                azimuth
                  ? 'bg-indigo-600 text-white'
                  : 'bg-neutral-800 text-neutral-300 hover:bg-neutral-700'}"
                title={CAMERA_AZIMUTHS[azimuth]}
                aria-pressed={generation.cameraAzimuth === azimuth}
                onclick={() => set("cameraAzimuth", azimuth)}
              >
                {azimuth * 45}°
              </button>
            {/if}
          {/each}
        </div>
        <p class="text-center text-[10px] uppercase tracking-wide text-neutral-500 mt-1">
          {locale.t("generation.image_edit.camera.back")}
        </p>
      </div>
    </div>

    <div>
      <span class="text-xs text-neutral-400 block mb-1.5">
        {locale.t("generation.image_edit.camera.elevation")}
      </span>
      <div class="flex gap-1">
        {#each elevationKeys as key, i (key)}
          <button
            type="button"
            class={segment(generation.cameraElevation === i)}
            title={CAMERA_ELEVATIONS[i]}
            aria-pressed={generation.cameraElevation === i}
            onclick={() => set("cameraElevation", i)}
          >
            {locale.t(`generation.image_edit.camera.elevation_${key}`)}
          </button>
        {/each}
      </div>
    </div>

    <div>
      <span class="text-xs text-neutral-400 block mb-1.5">
        {locale.t("generation.image_edit.camera.distance")}
      </span>
      <div class="flex gap-1">
        {#each distanceKeys as key, i (key)}
          <button
            type="button"
            class={segment(generation.cameraDistance === i)}
            title={CAMERA_DISTANCES[i]}
            aria-pressed={generation.cameraDistance === i}
            onclick={() => set("cameraDistance", i)}
          >
            {locale.t(`generation.image_edit.camera.distance_${key}`)}
          </button>
        {/each}
      </div>
    </div>

    <div>
      <span class="text-[11px] text-neutral-500 block mb-1">
        {locale.t("generation.image_edit.camera.prompt_preview")}
      </span>
      <code class="block rounded bg-neutral-900/70 px-2 py-1 text-[11px] text-neutral-300 break-words">
        {generation.cameraAnglePrompt}
      </code>
    </div>

    {#if loraInstalled}
      <p class="text-[11px] text-neutral-500">
        {locale.t("generation.image_edit.camera.lora_active", {
          strength: MULTI_ANGLE_LORA.strength.toFixed(1),
        })}
      </p>
    {:else}
      <p class="text-[11px] text-amber-300/80">
        {locale.t("generation.image_edit.camera.lora_missing")}
      </p>
      <button
        class="w-full rounded-lg bg-neutral-800 border border-neutral-700 px-3 py-2 text-xs text-neutral-100 hover:border-indigo-500 transition-colors disabled:opacity-50"
        disabled={downloading}
        onclick={downloadLora}
      >
        {locale.t("generation.image_edit.camera.lora_download")}
      </button>
      {#if downloading}
        <div class="bg-neutral-800/80 rounded-lg px-3 py-2">
          <div class="flex items-center justify-between text-[11px] text-neutral-400 mb-1">
            <span class="truncate mr-2">
              {locale.t("generation.upscale.downloading", { model: MULTI_ANGLE_LORA.filename })}
            </span>
            {#if dlTotal > 0}
              <span class="shrink-0 tabular-nums">
                {locale.formatBytes(dlBytes)} / {locale.formatBytes(dlTotal)} ({dlPercent}%)
              </span>
            {/if}
          </div>
          {#if dlTotal > 0}
            <div class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden">
              <div
                class="bg-indigo-400 h-full rounded-full transition-[width] duration-300 ease-out"
                style="width: {dlPercent}%"
              ></div>
            </div>
          {/if}
        </div>
      {/if}
      {#if downloadError}
        <p class="text-[11px] text-red-400">{downloadError}</p>
      {/if}
    {/if}
  {/if}
</div>
