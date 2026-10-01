<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { getVideoDraftStatus, getH3UpscalerStatus, installH3Upscaler, refineVideoDraft, retakeVideoDraft, deleteVideoDraft } from "../../utils/api.js";
  import type { VideoDraftStatus, H3UpscalerStatus } from "../../utils/api.js";

  let { filename }: { filename: string } = $props();
  let status = $state<VideoDraftStatus | null>(null);
  let installer = $state<H3UpscalerStatus | null>(null);
  let open = $state(false);
  let busy = $state(false);
  let queued = $state(false);
  let error = $state("");
  let steps = $state(8);
  let sigma = $state(0.35);
  /** Retake range in seconds; H3 always renders at 24 fps. */
  const FPS = 24;
  let retakeFrom = $state(0);
  let retakeTo = $state(1);
  let retakeSteps = $state(20);
  const duration = $derived(status?.draft ? status.draft.frames / FPS : 0);
  const retakeValid = $derived(
    duration > 0 && Number.isFinite(retakeFrom) && Number.isFinite(retakeTo) &&
      retakeFrom >= 0 && retakeFrom < retakeTo && retakeTo <= duration &&
      Number.isInteger(retakeSteps) && retakeSteps >= 8 && retakeSteps <= 40,
  );

  $effect(() => {
    const name = filename;
    const connected = connection.connected;
    let active = true;
    void getVideoDraftStatus(name).then(value => { if (active) status = value; }).catch(() => {});
    if (connected) void getH3UpscalerStatus().then(value => { if (active) installer = value; }).catch(() => {});
    return () => { active = false; };
  });
  async function run(action: "refine" | "retake" | "install" | "delete" | "refresh") {
    busy = true;
    error = "";
    try {
      if (action === "refine") { await refineVideoDraft(filename, steps, sigma); queued = true; }
      if (action === "retake" && status?.draft) {
        const frames = status.draft.frames;
        const start = Math.max(0, Math.floor(retakeFrom * FPS));
        const end = Math.min(frames, Math.max(start + 1, Math.ceil(retakeTo * FPS)));
        await retakeVideoDraft(filename, start, end, retakeSteps);
        queued = true;
      }
      if (action === "install") installer = await installH3Upscaler();
      if (action === "delete") { await deleteVideoDraft(filename); open = false; }
      status = await getVideoDraftStatus(filename);
      if (action === "refresh") installer = await getH3UpscalerStatus();
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }
</script>

{#if status?.retained}
  <div class="relative">
    <button type="button" class="min-h-11 rounded-lg px-2 text-xs text-neutral-100" class:bg-neutral-700={open}
      aria-expanded={open} onclick={() => { open = !open; if (open) { queued = false; void run("refresh"); } }}>{locale.t("video.draft.open")}</button>
    {#if open}
      <div class="absolute bottom-full right-0 z-20 mb-2 flex w-80 max-w-[85vw] flex-col gap-3 rounded-xl border border-neutral-700 bg-neutral-900 p-3 text-neutral-100 shadow-xl">
        <div class="flex items-center justify-between">
          <span class="text-sm font-medium">{locale.t("video.draft.title")}</span>
          <button type="button" class="min-h-11 px-3" aria-label={locale.t("common.close")} onclick={() => open = false}>×</button>
        </div>
        {#if status.draft}
          <p class="text-xs text-neutral-500">{locale.t("video.draft.storage", { size: locale.formatBytes(status.draft.bytes) })}</p>
        {/if}
        {#if !status.available}
          <p class="text-xs text-amber-300">{status.error}</p>
        {:else}
          <section class="flex flex-col gap-2 border-t border-neutral-800 pt-2">
            <span class="text-xs font-medium">{locale.t("video.draft.retake_title")}</span>
            <p class="text-xs text-neutral-400">{locale.t("video.draft.retake_hint")}</p>
            {#if !status.retake_ready}
              <p class="text-xs text-amber-300">{locale.t("video.draft.retake_update_nodes")}</p>
            {:else}
              <div class="flex gap-2">
                <label class="flex flex-1 flex-col gap-1 text-xs">{locale.t("video.draft.retake_from")}
                  <input type="number" min="0" max={duration} step="0.1" bind:value={retakeFrom} disabled={busy} class="min-h-11 rounded bg-neutral-800 p-2" />
                </label>
                <label class="flex flex-1 flex-col gap-1 text-xs">{locale.t("video.draft.retake_to")}
                  <input type="number" min="0" max={duration} step="0.1" bind:value={retakeTo} disabled={busy} class="min-h-11 rounded bg-neutral-800 p-2" />
                </label>
              </div>
              <label class="flex items-center justify-between gap-2 text-xs">{locale.t("generation.sampler.steps")}
                <input type="number" min="8" max="40" step="1" bind:value={retakeSteps} disabled={busy} class="w-20 min-h-11 rounded bg-neutral-800 p-2" />
              </label>
              <button type="button" class="min-h-11 rounded-lg bg-[var(--theme-accent-600)] px-3 text-xs disabled:opacity-50"
                disabled={busy || queued || !retakeValid}
                onclick={() => void run("retake")}>{locale.t("video.draft.retake_submit")}</button>
            {/if}
          </section>
          <section class="flex flex-col gap-2 border-t border-neutral-800 pt-2">
            <span class="text-xs font-medium">{locale.t("video.draft.refine_title")}</span>
            {#if status.draft}
              <p class="text-xs text-neutral-300">{status.draft.width}×{status.draft.height} → {status.draft.width * 2}×{status.draft.height * 2}</p>
            {/if}
            <p class="text-xs text-neutral-400">{locale.t("video.draft.hint")}</p>
            {#if !status.upscaler_ready}
              {#if installer?.can_install}
                <button type="button" class="min-h-11 rounded-lg bg-neutral-800 px-3 text-xs disabled:opacity-50" disabled={busy} onclick={() => void run("install")}>{locale.t("video.draft.install")}</button>
              {:else}
                <p class="text-xs text-amber-300">{locale.t("video.draft.remote_install")}</p>
              {/if}
            {:else}
              <label class="flex items-center justify-between gap-2 text-xs">{locale.t("generation.sampler.steps")}
                <input type="number" min="4" max="20" step="1" bind:value={steps} disabled={busy} class="w-20 min-h-11 rounded bg-neutral-800 p-2" />
              </label>
              <label class="flex items-center justify-between gap-2 text-xs">{locale.t("video.draft.strength")}
                <input type="number" min="0.15" max="0.6" step="0.05" bind:value={sigma} disabled={busy} class="w-20 min-h-11 rounded bg-neutral-800 p-2" />
              </label>
              <button type="button" class="min-h-11 rounded-lg bg-[var(--theme-accent-600)] px-3 text-xs disabled:opacity-50"
                disabled={busy || queued || !Number.isInteger(steps) || steps < 4 || steps > 20 || !Number.isFinite(sigma) || sigma < 0.15 || sigma > 0.6}
                onclick={() => void run("refine")}>{locale.t("video.draft.submit")}</button>
            {/if}
          </section>
        {/if}
        {#if queued}<p role="status" class="text-xs text-green-400">{locale.t("video.draft.queued")}</p>{/if}
        {#if busy}<p role="status" class="text-xs text-neutral-400">{locale.t("common.loading")}</p>{/if}
        {#if error}<p role="alert" class="text-xs text-red-400">{error}</p>{/if}
        <button type="button" class="min-h-11 text-xs text-neutral-400 underline disabled:opacity-50" disabled={busy} onclick={() => void run("refresh")}>{locale.t("generation.video.acceleration_refresh")}</button>
        <button type="button" class="min-h-11 text-xs text-red-300 disabled:opacity-50" disabled={busy || queued} onclick={() => void run("delete")}>{locale.t("video.draft.delete")}</button>
      </div>
    {/if}
  </div>
{/if}
