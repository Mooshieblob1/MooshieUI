<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import {
    deflickerVideo,
    downloadModel,
    RAFT_OPTICAL_FLOW_MODEL,
    type DeflickerDirection,
  } from "../../utils/api.js";
  import InfoTip from "../ui/InfoTip.svelte";

  interface Props {
    filename: string;
    onClose: () => void;
  }

  let { filename, onClose }: Props = $props();

  let strength = $state(1.0);
  let direction = $state<DeflickerDirection>("bidirectional");
  let threshold = $state(0.1);
  let selectedModel = $state<string | null>(null);

  let downloading = $state(false);
  let submitting = $state(false);
  let queued = $state(false);
  let errorText = $state<string | null>(null);

  const installedModels = $derived(models.opticalFlowModels);
  const hasModel = $derived(installedModels.length > 0);
  const defaultModel = $derived(
    installedModels.find((m) => m.toLowerCase().includes("raft_large")) ??
      installedModels[0] ??
      "",
  );
  const activeModel = $derived(
    selectedModel && installedModels.includes(selectedModel)
      ? selectedModel
      : defaultModel,
  );

  async function installRaft() {
    if (downloading || models.remote) return;
    downloading = true;
    errorText = null;
    try {
      await downloadModel(
        RAFT_OPTICAL_FLOW_MODEL.url,
        RAFT_OPTICAL_FLOW_MODEL.category,
        RAFT_OPTICAL_FLOW_MODEL.filename,
        undefined,
        RAFT_OPTICAL_FLOW_MODEL.sha256,
      );
      await models.refresh();
    } catch (e) {
      errorText = String(e);
    } finally {
      downloading = false;
    }
  }

  async function submit() {
    if (submitting || !hasModel) return;
    submitting = true;
    errorText = null;
    queued = false;
    try {
      await deflickerVideo(
        filename,
        strength,
        direction,
        threshold,
        activeModel || undefined,
      );
      queued = true;
    } catch (e) {
      errorText = locale.t("video.deflicker.failed", { error: String(e) });
    } finally {
      submitting = false;
    }
  }
</script>

<div
  class="absolute bottom-full right-0 mb-2 w-80 rounded-xl bg-neutral-900 border border-neutral-700 shadow-xl p-3 flex flex-col gap-3 text-neutral-100"
>
  <div class="flex items-center justify-between">
    <span class="text-sm font-medium">{locale.t("video.deflicker.title")}</span>
    <button type="button" class="text-neutral-400 text-xs px-1" onclick={onClose}>x</button>
  </div>

  {#if !hasModel}
    <p class="text-xs text-neutral-400">{locale.t("video.deflicker.needs_model")}</p>
    <button
      type="button"
      class="rounded-lg bg-neutral-800 border border-neutral-700 px-3 py-1.5 text-xs hover:bg-neutral-700 disabled:opacity-50"
      disabled={downloading || models.remote}
      onclick={() => void installRaft()}
    >
      {downloading
        ? locale.t("video.deflicker.downloading")
        : locale.t("video.deflicker.download_model")}
    </button>
    {#if models.remote}
      <p class="text-[11px] text-neutral-400">
        {locale.t("generation.model.external_download_hint")}
      </p>
    {/if}
    {#if errorText}
      <p class="text-xs text-red-400">{errorText}</p>
    {/if}
  {:else}
    {#if installedModels.length > 1}
      <label class="flex flex-col gap-1 text-xs text-neutral-400">
        <span>{locale.t("video.deflicker.model")}</span>
        <select
          class="bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1 text-xs text-neutral-100"
          value={activeModel}
          onchange={(e) => (selectedModel = e.currentTarget.value)}
        >
          {#each installedModels as model (model)}
            <option value={model}>{model}</option>
          {/each}
        </select>
      </label>
    {/if}

    <div class="flex flex-col gap-1">
      <div class="flex items-center justify-between text-xs text-neutral-400">
        <span>
          {locale.t("video.deflicker.strength")}
          <InfoTip text={locale.t("video.deflicker.strength_tip")} />
        </span>
        <span class="font-mono text-neutral-200">{strength.toFixed(1)}</span>
      </div>
      <input
        type="range"
        min="0"
        max="10"
        step="0.1"
        bind:value={strength}
        class="w-full accent-[var(--theme-accent-500)]"
      />
    </div>

    <div class="flex items-center justify-between gap-2">
      <span class="text-xs text-neutral-400">
        {locale.t("video.deflicker.direction")}
        <InfoTip text={locale.t("video.deflicker.direction_tip")} />
      </span>
      <div class="flex rounded-lg overflow-hidden border border-neutral-700">
        <button
          type="button"
          class="px-2 py-1 text-xs"
          class:bg-neutral-700={direction === "bidirectional"}
          class:text-neutral-400={direction !== "bidirectional"}
          onclick={() => (direction = "bidirectional")}
        >
          {locale.t("video.deflicker.direction_bidirectional")}
        </button>
        <button
          type="button"
          class="px-2 py-1 text-xs"
          class:bg-neutral-700={direction === "forward"}
          class:text-neutral-400={direction !== "forward"}
          onclick={() => (direction = "forward")}
        >
          {locale.t("video.deflicker.direction_forward")}
        </button>
      </div>
    </div>

    <div class="flex flex-col gap-1">
      <div class="flex items-center justify-between text-xs text-neutral-400">
        <span>
          {locale.t("video.deflicker.threshold")}
          <InfoTip text={locale.t("video.deflicker.threshold_tip")} />
        </span>
        <span class="font-mono text-neutral-200">{threshold.toFixed(2)}</span>
      </div>
      <input
        type="range"
        min="0"
        max="1"
        step="0.02"
        bind:value={threshold}
        class="w-full accent-[var(--theme-accent-500)]"
      />
    </div>

    {#if queued}
      <p class="text-xs text-emerald-400">{locale.t("video.deflicker.queued")}</p>
    {:else}
      <button
        type="button"
        class="rounded-lg bg-[var(--theme-accent-600)] px-3 py-1.5 text-xs disabled:opacity-50"
        disabled={submitting}
        onclick={() => void submit()}
      >
        {locale.t("video.deflicker.submit")}
      </button>
    {/if}

    {#if errorText}
      <p class="text-xs text-red-400">{errorText}</p>
    {/if}
  {/if}
</div>
