<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { downloadModel, installPipPackage } from "../../utils/api.js";
  import { ipcListen } from "../../utils/ipc.js";
  import { onMount } from "svelte";
  import InfoTip from "../ui/InfoTip.svelte";
  import EditableValue from "../ui/EditableValue.svelte";
  import { scrollCapture } from "../../utils/scrollCapture.js";

  interface RecommendedModel {
    label: string;
    filename: string;
    url: string;
    sha256: string;
  }

  const recommendedModels: RecommendedModel[] = [
    {
      label: "YOLO11n Face Seg (Recommended)",
      filename: "Anzhc Face seg 640 v4 y11n.pt",
      url: "https://huggingface.co/Anzhc/Anzhcs_YOLOs/resolve/0319daeae9ae40752c2fb3904069cb35cc61d2ec/Anzhc%20Face%20seg%20640%20v4%20y11n.pt",
      sha256: "1e77ad7bd349babd8a4a90478bfc965348642b63a8d95d3b43ee13db42fd0a64",
    },
    {
      label: "YOLOv8n Face (Lightweight)",
      filename: "face_yolov8n.pt",
      url: "https://huggingface.co/Bingsu/adetailer/resolve/main/face_yolov8n.pt",
      sha256: "70b640f8f60b1cf0dcc72f30caf3da9495eb2fb6509da48c53374ad6806e6a9c",
    },
  ];

  let downloading = $state<string | null>(null);
  let downloadError = $state<string | null>(null);

  let dlBytes = $state(0);
  let dlTotal = $state(0);

  function formatBytes(bytes: number): string {
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  const dlPercent = $derived(dlTotal > 0 ? Math.round((dlBytes / dlTotal) * 100) : 0);

  onMount(async () => {
    await ipcListen("download:progress", (event: any) => {
      const data = event.payload as {
        filename: string;
        downloaded: number;
        total: number;
        done: boolean;
      };
      if (data.done) {
        dlBytes = 0;
        dlTotal = 0;
      } else {
        dlBytes = data.downloaded;
        dlTotal = data.total;
      }
    });
  });

  function getModelOptions() {
    const installed = models.ultralyticsModels;
    const options: { value: string; label: string; needsDownload: boolean }[] = [];

    for (const rec of recommendedModels) {
      const isInstalled = installed.includes(rec.filename);
      options.push({
        value: rec.filename,
        label: isInstalled ? rec.label : `⬇ ${rec.label}`,
        needsDownload: !isInstalled,
      });
    }

    for (const m of installed) {
      if (!recommendedModels.some((r) => r.filename === m)) {
        options.push({ value: m, label: m, needsDownload: false });
      }
    }

    return options;
  }

  async function handleModelSelect(filename: string) {
    const rec = recommendedModels.find((r) => r.filename === filename);
    const isInstalled = models.ultralyticsModels.includes(filename);

    if (rec && !isInstalled) {
      downloading = filename;
      downloadError = null;
      try {
        await downloadModel(rec.url, "ultralytics", rec.filename, undefined, rec.sha256);
        // Ensure ultralytics Python package is installed (required by MooshieFaceDetailer)
        await installPipPackage("ultralytics==8.4.34");
        await models.refresh();
      } catch (e) {
        downloadError = `Download failed: ${e}`;
        generation.facefixDetector = null;
        return;
      } finally {
        downloading = null;
      }
    }

    generation.facefixDetector = filename;
  }
</script>

<div class="space-y-3">
  <!-- Enable toggle -->
  <div class="flex items-center justify-between">
    <label class="text-xs text-neutral-400">{locale.t('generation.facefix.title')}<InfoTip text={locale.t('generation.facefix.tip')} /></label>
    <button
      class="relative w-10 h-5 rounded-full transition-colors {generation.facefixEnabled
        ? 'bg-indigo-600'
        : 'bg-neutral-700'}"
      onclick={() => (generation.facefixEnabled = !generation.facefixEnabled)}
      role="switch"
      aria-checked={generation.facefixEnabled}
    >
      <span
        class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform {generation.facefixEnabled
          ? 'translate-x-5'
          : ''}"
      ></span>
    </button>
  </div>

  {#if generation.facefixEnabled}
    <!-- Detector Model -->
    <div>
      <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.facefix.detector')}<InfoTip text={locale.t('generation.facefix.detector_tip')} /></label>
      <select
        value={generation.facefixDetector ?? ""}
        onchange={(e) => handleModelSelect((e.target as HTMLSelectElement).value)}
        disabled={downloading !== null}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors disabled:opacity-50"
      >
        <option value="">{locale.t('generation.facefix.select_model')}</option>
        {#each getModelOptions() as opt}
          <option value={opt.value}>{opt.label}</option>
        {/each}
      </select>
      {#if downloading}
        <div class="mt-2 bg-neutral-800/80 rounded-lg px-3 py-2">
          <div class="flex items-center justify-between text-[11px] text-neutral-400 mb-1">
            <span class="truncate mr-2">{locale.t('generation.facefix.downloading', { model: downloading || '' })}</span>
            {#if dlTotal > 0}
              <span class="shrink-0 tabular-nums">{formatBytes(dlBytes)} / {formatBytes(dlTotal)} ({dlPercent}%)</span>
            {/if}
          </div>
          {#if dlTotal > 0}
            <div class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden">
              <div
                class="bg-indigo-400 h-full rounded-full transition-[width] duration-300 ease-out"
                style="width: {dlPercent}%"
              ></div>
            </div>
          {:else}
            <div class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden">
              <div class="bg-indigo-400 h-full rounded-full w-1/3 animate-pulse"></div>
            </div>
          {/if}
        </div>
      {/if}
      {#if downloadError}
        <p class="text-xs text-red-400 mt-1">{downloadError}</p>
      {/if}
    </div>

    <div class="grid grid-cols-2 gap-3">
      <!-- Denoise -->
      <div use:scrollCapture>
        <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          <span>{locale.t('generation.facefix.denoise')}<InfoTip text={locale.t('generation.facefix.denoise_tip')} /></span>
          <EditableValue value={generation.facefixDenoise} min={0} max={1} step={0.05} decimals={2} onchange={(v) => generation.facefixDenoise = v} />
        </label>
        <input
          type="range"
          bind:value={generation.facefixDenoise}
          min="0"
          max="1"
          step="0.05"
          class="w-full accent-indigo-500"
        />
      </div>

      <!-- Steps -->
      <div use:scrollCapture>
        <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          <span>{locale.t('generation.facefix.steps')}<InfoTip text={locale.t('generation.facefix.steps_tip')} /></span>
          <EditableValue value={generation.facefixSteps} min={1} max={50} step={1} onchange={(v) => generation.facefixSteps = v} />
        </label>
        <input
          type="range"
          bind:value={generation.facefixSteps}
          min="1"
          max="50"
          step="1"
          class="w-full accent-indigo-500"
        />
      </div>
    </div>

    <!-- Guide Size -->
    <div use:scrollCapture>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        <span>{locale.t('generation.facefix.guide_size')}<InfoTip text={locale.t('generation.facefix.guide_size_tip')} /></span>
        <EditableValue value={generation.facefixGuideSize} min={256} max={1024} step={64} suffix="px" onchange={(v) => generation.facefixGuideSize = v} />
      </label>
      <input
        type="range"
        bind:value={generation.facefixGuideSize}
        min="256"
        max="1024"
        step="64"
        class="w-full accent-indigo-500"
      />
    </div>
  {/if}
</div>
