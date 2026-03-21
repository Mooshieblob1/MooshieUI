<script lang="ts">
  import { downloads } from "../../stores/downloads.svelte.js";

  function formatBytes(bytes: number): string {
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  function formatSpeed(bytesPerSec: number): string {
    if (bytesPerSec <= 0) return "";
    if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(0)} KB/s`;
    return `${(bytesPerSec / (1024 * 1024)).toFixed(1)} MB/s`;
  }

  function shortName(filename: string): string {
    // Trim long filenames for display
    if (filename.length <= 40) return filename;
    const ext = filename.lastIndexOf(".");
    if (ext > 0) {
      return filename.slice(0, 30) + "..." + filename.slice(ext);
    }
    return filename.slice(0, 37) + "...";
  }
</script>

{#if downloads.hasActive}
  <div class="shrink-0 bg-neutral-900/95 border-b border-neutral-800">
    {#each [...downloads.active.values()] as dl (dl.filename)}
      {@const percent = dl.total > 0 ? Math.round((dl.downloaded / dl.total) * 100) : 0}
      <div class="flex items-center gap-3 px-4 py-1.5">
        {#if dl.done}
          <svg class="w-3.5 h-3.5 shrink-0 text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
          </svg>
        {:else}
          <div class="w-3.5 h-3.5 shrink-0 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin"></div>
        {/if}

        <span class="text-xs text-neutral-300 truncate min-w-0" title={dl.filename}>
          {#if dl.done}
            Downloaded {shortName(dl.filename)}
          {:else}
            Downloading {shortName(dl.filename)}
          {/if}
        </span>

        {#if !dl.done}
          <div class="flex-1 max-w-48 bg-neutral-700 rounded-full h-1.5 overflow-hidden">
            {#if dl.total > 0}
              <div
                class="bg-indigo-500 h-full rounded-full transition-[width] duration-300 ease-out"
                style="width: {percent}%"
              ></div>
            {:else}
              <div class="bg-indigo-500 h-full rounded-full w-1/3 animate-pulse"></div>
            {/if}
          </div>

          <span class="text-[11px] text-neutral-500 tabular-nums shrink-0">
            {#if dl.total > 0}
              {formatBytes(dl.downloaded)} / {formatBytes(dl.total)} ({percent}%){#if dl.speed > 0} · {formatSpeed(dl.speed)}{/if}
            {:else}
              {formatBytes(dl.downloaded)}{#if dl.speed > 0} · {formatSpeed(dl.speed)}{/if}
            {/if}
          </span>
        {/if}
      </div>
    {/each}
  </div>
{/if}
