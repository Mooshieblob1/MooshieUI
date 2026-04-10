<script lang="ts">
  import { getGpuStats } from "../../utils/api.js";
  import { locale } from "../../stores/locale.svelte.js";
  import type { GpuStats } from "../../types/index.js";

  let gpus = $state<GpuStats[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let timer: ReturnType<typeof setInterval> | undefined;

  async function refresh() {
    try {
      gpus = await getGpuStats();
      error = null;
    } catch (e: any) {
      error = e?.message ?? "Failed to fetch GPU stats";
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    refresh();
    timer = setInterval(refresh, 5000);
    return () => {
      if (timer) clearInterval(timer);
    };
  });

  function vramPercent(gpu: GpuStats): number {
    if (!gpu.vram_total_mb) return 0;
    return Math.round((gpu.vram_used_mb / gpu.vram_total_mb) * 100);
  }

  function statusColor(status: string | undefined): string {
    if (!status) return "bg-neutral-600";
    if (status === "running") return "bg-green-500";
    if (status === "idle") return "bg-indigo-500";
    if (status === "starting") return "bg-amber-500";
    if (status === "error") return "bg-red-500";
    return "bg-neutral-600";
  }

  function statusLabel(status: string | undefined): string {
    if (!status) return "No worker";
    if (status === "running") return "Running";
    if (status === "idle") return "Idle";
    if (status === "starting") return "Starting";
    if (status === "error") return "Error";
    return status;
  }

  function vramBarColor(pct: number): string {
    if (pct > 90) return "bg-red-500";
    if (pct > 70) return "bg-amber-500";
    return "bg-indigo-500";
  }

  function tempColor(temp: number): string {
    if (temp > 85) return "text-red-400";
    if (temp > 70) return "text-amber-400";
    return "text-neutral-300";
  }
</script>

{#if loading}
  <div class="flex items-center gap-2 text-neutral-500 text-sm py-3">
    <svg class="w-4 h-4 animate-spin" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
      <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
      <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"></path>
    </svg>
    Loading GPU info…
  </div>
{:else if error}
  <div class="text-sm text-red-400 py-2">{error}</div>
{:else if gpus.length === 0}
  <div class="text-sm text-neutral-500 py-2">No GPUs detected</div>
{:else}
  <div class="space-y-3">
    {#each gpus as gpu (gpu.index)}
      <div class="bg-neutral-800/60 border border-neutral-700/50 rounded-lg p-4 space-y-3">
        <!-- Header: GPU name + worker status badge -->
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2">
            <span class="text-sm font-medium text-neutral-100">GPU {gpu.index}</span>
            <span class="text-xs text-neutral-400 truncate max-w-[200px]">{gpu.name}</span>
          </div>
          <div class="flex items-center gap-2">
            {#if gpu.worker}
              <span class="inline-flex items-center gap-1.5 text-xs text-neutral-300">
                <span class="w-2 h-2 rounded-full {statusColor(gpu.worker.status)}"></span>
                {statusLabel(gpu.worker.status)}
                {#if gpu.worker.reserved}
                  <span class="text-amber-400 text-[10px]">(busy)</span>
                {/if}
              </span>
            {:else}
              <span class="inline-flex items-center gap-1.5 text-xs text-neutral-500">
                <span class="w-2 h-2 rounded-full bg-neutral-600"></span>
                No worker
              </span>
            {/if}
          </div>
        </div>

        <!-- VRAM bar -->
        <div>
          <div class="flex items-center justify-between text-[11px] mb-1">
            <span class="text-neutral-400">VRAM</span>
            <span class="text-neutral-300">{gpu.vram_used_mb} / {gpu.vram_total_mb} MiB ({vramPercent(gpu)}%)</span>
          </div>
          <div class="w-full h-2 bg-neutral-700 rounded-full overflow-hidden">
            <div
              class="h-full rounded-full transition-all duration-500 {vramBarColor(vramPercent(gpu))}"
              style="width: {vramPercent(gpu)}%"
            ></div>
          </div>
        </div>

        <!-- Stats row -->
        <div class="flex items-center gap-4 text-[11px]">
          <div class="flex items-center gap-1.5">
            <span class="text-neutral-500">Utilization</span>
            <span class="text-neutral-200 font-medium">{gpu.gpu_util}%</span>
          </div>
          <div class="flex items-center gap-1.5">
            <span class="text-neutral-500">Temp</span>
            <span class="font-medium {tempColor(gpu.temperature)}">{gpu.temperature}°C</span>
          </div>
          <div class="flex items-center gap-1.5">
            <span class="text-neutral-500">Power</span>
            <span class="text-neutral-200 font-medium">{gpu.power_draw_w}W</span>
          </div>
          {#if gpu.worker?.label}
            <div class="flex items-center gap-1.5 ml-auto">
              <span class="text-neutral-500">Worker</span>
              <span class="text-neutral-300">{gpu.worker.label} (:{gpu.worker.port})</span>
            </div>
          {/if}
        </div>
      </div>
    {/each}
  </div>
  <p class="text-[10px] text-neutral-600 mt-2">Auto-refreshes every 5 seconds</p>
{/if}
