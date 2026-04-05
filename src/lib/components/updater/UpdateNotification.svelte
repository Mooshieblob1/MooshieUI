<script lang="ts">
  import { onMount } from "svelte";
  import { check } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { stopComfyui } from "../../utils/api.js";
  import { locale } from "../../stores/locale.svelte.js";

  // @ts-ignore — injected by Vite at build time
  const currentVersion: string = __APP_VERSION__ ?? "dev";

  type UpdateState = "idle" | "available" | "downloading" | "ready" | "error" | "version_mismatch";

  let updateState: UpdateState = $state("idle");
  let version = $state("");
  let downloadProgress = $state(0);
  let totalSize = $state(0);
  let errorMessage = $state("");
  let dismissed = $state(false);
  let expectedVersion = $state("");

  let updateObj: Awaited<ReturnType<typeof check>> | null = null;

  const progressPercent = $derived(
    totalSize > 0 ? Math.round((downloadProgress / totalSize) * 100) : 0
  );

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  onMount(async () => {
    // Check if a previous update didn't apply correctly
    const pending = localStorage.getItem("mooshieui_pending_update");
    if (pending) {
      localStorage.removeItem("mooshieui_pending_update");
      if (pending !== currentVersion) {
        console.warn(`[Updater] Version mismatch after update: expected v${pending}, running v${currentVersion}`);
        expectedVersion = pending;
        updateState = "version_mismatch";
        return;
      } else {
        console.log(`[Updater] Update to v${pending} applied successfully`);
      }
    }

    // Delay so app startup isn't blocked
    await new Promise((r) => setTimeout(r, 3000));
    try {
      console.log(`[Updater] Checking for updates (current: v${currentVersion})...`);
      const update = await check();
      if (update) {
        updateObj = update;
        version = update.version;
        updateState = "available";
        console.log(`[Updater] Update available: v${update.version} (current: v${currentVersion})`);
      } else {
        console.log("[Updater] No updates available");
      }
    } catch (e) {
      console.warn("[Updater] Update check failed:", e);
    }
  });

  async function downloadAndInstall() {
    if (!updateObj) return;
    updateState = "downloading";
    console.log(`[Updater] Starting download of v${updateObj.version}...`);
    try {
      await updateObj.downloadAndInstall((event) => {
        if (event.event === "Started") {
          totalSize = event.data.contentLength ?? 0;
          downloadProgress = 0;
          console.log(`[Updater] Download started (size: ${formatBytes(totalSize)})`);
        } else if (event.event === "Progress") {
          downloadProgress += event.data.chunkLength;
        } else if (event.event === "Finished") {
          console.log("[Updater] Download and install finished");
          updateState = "ready";
        }
      });
      // Store the expected version so we can verify after restart
      localStorage.setItem("mooshieui_pending_update", version);
      console.log(`[Updater] Stored pending update version: v${version}`);
      updateState = "ready";
    } catch (e) {
      console.error("[Updater] Download/install failed:", e);
      updateState = "error";
      errorMessage = String(e);
    }
  }

  async function restartApp() {
    console.log("[Updater] Restarting app to apply update...");
    try { await stopComfyui(); } catch {}
    await relaunch();
  }

  function dismiss() {
    dismissed = true;
  }
</script>

{#if updateState !== "idle" && !dismissed}
  <div class="flex items-center gap-3 px-4 py-2 border-b text-sm
    {updateState === 'error' || updateState === 'version_mismatch'
      ? 'bg-red-900/30 border-red-800/50 text-red-200'
      : 'bg-indigo-900/30 border-indigo-800/50 text-indigo-200'}">

    {#if updateState === "available"}
      <svg class="w-4 h-4 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
          d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
      </svg>
      <span class="flex-1">{locale.t('updater.available', { version })}</span>
      <button
        onclick={downloadAndInstall}
        class="px-3 py-1 bg-indigo-600 hover:bg-indigo-500 text-white rounded text-xs font-medium transition-colors cursor-pointer"
      >{locale.t('updater.download_now')}</button>
      <button
        onclick={dismiss}
        class="px-3 py-1 bg-neutral-700 hover:bg-neutral-600 text-neutral-300 rounded text-xs font-medium transition-colors cursor-pointer"
      >{locale.t('updater.later')}</button>

    {:else if updateState === "downloading"}
      <div class="w-4 h-4 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin shrink-0"></div>
      <span class="flex-1">{locale.t('updater.downloading', { version })} {formatBytes(downloadProgress)}{totalSize > 0 ? ` / ${formatBytes(totalSize)}` : ''}</span>
      <div class="w-32 h-2 bg-neutral-700 rounded-full overflow-hidden">
        <div
          class="h-full bg-indigo-500 transition-[width] duration-300"
          style="width: {progressPercent}%"
        ></div>
      </div>

    {:else if updateState === "ready"}
      <svg class="w-4 h-4 shrink-0 text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
      </svg>
      <span class="flex-1">{locale.t('updater.ready', { version })}</span>
      <button
        onclick={restartApp}
        class="px-3 py-1 bg-emerald-600 hover:bg-emerald-500 text-white rounded text-xs font-medium transition-colors cursor-pointer"
      >{locale.t('updater.restart_now')}</button>
      <button
        onclick={dismiss}
        class="px-3 py-1 bg-neutral-700 hover:bg-neutral-600 text-neutral-300 rounded text-xs font-medium transition-colors cursor-pointer"
      >{locale.t('updater.later')}</button>

    {:else if updateState === "version_mismatch"}
      <svg class="w-4 h-4 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
          d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L4.082 16.5c-.77.833.192 2.5 1.732 2.5z" />
      </svg>
      <span class="flex-1">{locale.t('updater.version_mismatch', { expected: expectedVersion, current: currentVersion })}</span>
      <button
        onclick={dismiss}
        class="px-3 py-1 bg-neutral-700 hover:bg-neutral-600 text-neutral-300 rounded text-xs font-medium transition-colors cursor-pointer"
      >{locale.t('updater.dismiss')}</button>

    {:else if updateState === "error"}
      <span class="flex-1">{locale.t('updater.error', { error: errorMessage })}</span>
      <button
        onclick={dismiss}
        class="px-3 py-1 bg-neutral-700 hover:bg-neutral-600 text-neutral-300 rounded text-xs font-medium transition-colors cursor-pointer"
      >{locale.t('updater.dismiss')}</button>
    {/if}
  </div>
{/if}
