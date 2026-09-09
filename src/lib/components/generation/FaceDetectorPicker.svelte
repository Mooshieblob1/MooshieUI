<script lang="ts">
  /**
   * The detector-model picker shared by both face panels.
   *
   * Detection is local YOLO for the FaceFix panel and for the NovelAI face
   * detailer alike, so the weight list, the download-on-select flow and the
   * `ultralytics` install all live here rather than being duplicated. The
   * caller owns where the chosen filename is stored; this component only
   * reports it once the file is actually on disk.
   */
  import { models } from "../../stores/models.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { downloadModel, installPipPackage } from "../../utils/api.js";
  import { isBrowserMode } from "../../utils/ipc.js";
  import { ipcListen } from "../../utils/ipc.js";
  import { onMount } from "svelte";
  import InfoTip from "../ui/InfoTip.svelte";

  interface Props {
    /** Currently selected weight, or null/empty for none. */
    value: string | null;
    /** Called with the weight once it is installed, or null if the fetch failed. */
    onchange: (filename: string | null) => void;
  }

  let { value, onchange }: Props = $props();

  interface RecommendedModel {
    labelKey: string;
    filename: string;
    url: string;
    sha256: string;
  }

  const recommendedModels: RecommendedModel[] = [
    {
      labelKey: "generation.facefix.yolo11n",
      filename: "Anzhc Face seg 640 v4 y11n.pt",
      url: "https://huggingface.co/Anzhc/Anzhcs_YOLOs/resolve/0319daeae9ae40752c2fb3904069cb35cc61d2ec/Anzhc%20Face%20seg%20640%20v4%20y11n.pt",
      sha256: "1e77ad7bd349babd8a4a90478bfc965348642b63a8d95d3b43ee13db42fd0a64",
    },
    {
      labelKey: "generation.facefix.yolov8n",
      filename: "face_yolov8n.pt",
      url: "https://huggingface.co/Bingsu/adetailer/resolve/main/face_yolov8n.pt",
      sha256: "70b640f8f60b1cf0dcc72f30caf3da9495eb2fb6509da48c53374ad6806e6a9c",
    },
  ];

  let downloading = $state<string | null>(null);
  let downloadError = $state<string | null>(null);

  let dlBytes = $state(0);
  let dlTotal = $state(0);

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
        label: isInstalled ? locale.t(rec.labelKey) : `⬇ ${locale.t(rec.labelKey)}`,
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
        // Ensure ultralytics Python package is installed (required by the
        // MooshieFaceDetailer / MooshieFaceDetect nodes)
        if (!isBrowserMode) {
          await installPipPackage("ultralytics==8.4.34");
        }
        await models.refresh();
      } catch (e) {
        downloadError = `Download failed: ${e}`;
        onchange(null);
        return;
      } finally {
        downloading = null;
      }
    }

    onchange(filename);
  }
</script>

<div>
  <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.facefix.detector')}<InfoTip text={locale.t('generation.facefix.detector_tip')} /></label>
  <select
    value={value ?? ""}
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
          <span class="shrink-0 tabular-nums">{locale.formatBytes(dlBytes)} / {locale.formatBytes(dlTotal)} ({dlPercent}%)</span>
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
