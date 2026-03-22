<script lang="ts">
  import { onMount } from "svelte";
  import { check } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { exit } from "@tauri-apps/plugin-process";
  import { stopComfyui } from "../../utils/api.js";

  type UpdateState = "idle" | "available" | "downloading" | "ready" | "error";

  let state = $state<UpdateState>("idle");
  let version = $state("");
  let downloadProgress = $state(0);
  let totalSize = $state(0);
  let errorMessage = $state("");
  let dismissed = $state(false);

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
    // Delay so app startup isn't blocked
    await new Promise((r) => setTimeout(r, 3000));
    try {
      const update = await check();
      if (update) {
        updateObj = update;
        version = update.version;
        state = "available";
      }
    } catch (e) {
      console.warn("Update check failed:", e);
    }
  });

  async function downloadAndInstall() {
    if (!updateObj) return;
    state = "downloading";
    try {
      await updateObj.downloadAndInstall((event) => {
        if (event.event === "Started") {
          totalSize = event.data.contentLength ?? 0;
          downloadProgress = 0;
        } else if (event.event === "Progress") {
          downloadProgress += event.data.chunkLength;
        } else if (event.event === "Finished") {
          state = "ready";
        }
      });
      state = "ready";
    } catch (e) {
      state = "error";
      errorMessage = String(e);
    }
  }

  async function restartApp() {
    try { await stopComfyui(); } catch {}
    await relaunch();
  }

  function dismiss() {
    dismissed = true;
  }
</script>

{#if state !== "idle" && !dismissed}
  <div class="flex items-center gap-3 px-4 py-2 border-b text-sm
    {state === 'error'
      ? 'bg-red-900/30 border-red-800/50 text-red-200'
      : 'bg-indigo-900/30 border-indigo-800/50 text-indigo-200'}">

    {#if state === "available"}
      <svg class="w-4 h-4 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
          d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
      </svg>
      <span class="flex-1">MooshieUI <strong>v{version}</strong> is available</span>
      <button
        onclick={downloadAndInstall}
        class="px-3 py-1 bg-indigo-600 hover:bg-indigo-500 text-white rounded text-xs font-medium transition-colors cursor-pointer"
      >Update Now</button>
      <button
        onclick={dismiss}
        class="px-3 py-1 bg-neutral-700 hover:bg-neutral-600 text-neutral-300 rounded text-xs font-medium transition-colors cursor-pointer"
      >Later</button>

    {:else if state === "downloading"}
      <div class="w-4 h-4 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin shrink-0"></div>
      <span class="flex-1">Downloading v{version}... {formatBytes(downloadProgress)}{totalSize > 0 ? ` / ${formatBytes(totalSize)}` : ''}</span>
      <div class="w-32 h-2 bg-neutral-700 rounded-full overflow-hidden">
        <div
          class="h-full bg-indigo-500 transition-[width] duration-300"
          style="width: {progressPercent}%"
        ></div>
      </div>

    {:else if state === "ready"}
      <svg class="w-4 h-4 shrink-0 text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
      </svg>
      <span class="flex-1">Update downloaded. Restart to apply v{version}.</span>
      <button
        onclick={restartApp}
        class="px-3 py-1 bg-emerald-600 hover:bg-emerald-500 text-white rounded text-xs font-medium transition-colors cursor-pointer"
      >Restart Now</button>
      <button
        onclick={dismiss}
        class="px-3 py-1 bg-neutral-700 hover:bg-neutral-600 text-neutral-300 rounded text-xs font-medium transition-colors cursor-pointer"
      >Later</button>

    {:else if state === "error"}
      <span class="flex-1">Update failed: {errorMessage}</span>
      <button
        onclick={dismiss}
        class="px-3 py-1 bg-neutral-700 hover:bg-neutral-600 text-neutral-300 rounded text-xs font-medium transition-colors cursor-pointer"
      >Dismiss</button>
    {/if}
  </div>
{/if}
