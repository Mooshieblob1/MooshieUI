<script lang="ts">
  import { onMount } from "svelte";
  import { deleteQueueItem, getQueue, interruptGeneration } from "../../utils/api.js";
  import type { QueueDisplayItem } from "../../types/index.js";

  let loading = $state(false);
  let interrupting = $state(false);
  let runningItems = $state<QueueDisplayItem[]>([]);
  let pendingItems = $state<QueueDisplayItem[]>([]);
  let error = $state<string | null>(null);

  function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null;
  }

  function findPromptId(value: unknown): string | null {
    if (typeof value === "string" && value.length > 12) return value;
    if (Array.isArray(value)) {
      for (const item of value) {
        const found = findPromptId(item);
        if (found) return found;
      }
      return null;
    }
    if (isRecord(value)) {
      const direct = value["prompt_id"];
      if (typeof direct === "string") return direct;
      for (const nested of Object.values(value)) {
        const found = findPromptId(nested);
        if (found) return found;
      }
    }
    return null;
  }

  function findMode(value: unknown): string | undefined {
    if (isRecord(value)) {
      const mode = value["mode"];
      if (typeof mode === "string") return mode;
      for (const nested of Object.values(value)) {
        const found = findMode(nested);
        if (found) return found;
      }
    }
    if (Array.isArray(value)) {
      for (const nested of value) {
        const found = findMode(nested);
        if (found) return found;
      }
    }
    return undefined;
  }

  function buildSummary(item: unknown): string {
    if (Array.isArray(item)) {
      return `Array item (${item.length} fields)`;
    }
    if (isRecord(item)) {
      const keys = Object.keys(item);
      if (keys.length === 0) return "Object item";
      return `Fields: ${keys.slice(0, 4).join(", ")}${keys.length > 4 ? "…" : ""}`;
    }
    return String(item);
  }

  function normalizeQueueItems(items: unknown[]): QueueDisplayItem[] {
    return items.map((item, index) => {
      const promptId = findPromptId(item) ?? `unknown-${index}`;
      const maybeNumber = Array.isArray(item) && typeof item[0] === "number" ? item[0] : undefined;
      const mode = findMode(item);
      return {
        id: `${promptId}-${index}`,
        promptId,
        number: maybeNumber,
        mode,
        summary: buildSummary(item),
        raw: item,
      };
    });
  }

  async function refreshQueue() {
    loading = true;
    error = null;
    try {
      const queue = await getQueue();
      runningItems = normalizeQueueItems(queue.queue_running);
      pendingItems = normalizeQueueItems(queue.queue_pending);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function removePending(item: QueueDisplayItem) {
    try {
      await deleteQueueItem(item.promptId);
      await refreshQueue();
    } catch (e) {
      error = String(e);
    }
  }

  async function interruptCurrent() {
    interrupting = true;
    try {
      await interruptGeneration();
      await refreshQueue();
    } catch (e) {
      error = String(e);
    } finally {
      interrupting = false;
    }
  }

  onMount(() => {
    refreshQueue();
    const timer = setInterval(refreshQueue, 2500);
    return () => clearInterval(timer);
  });
</script>

<div class="h-full overflow-y-auto p-6">
  <div class="max-w-5xl mx-auto space-y-4">
    <div class="flex items-center justify-between gap-3">
      <h2 class="text-lg font-semibold text-neutral-100">Queue Management</h2>
      <div class="flex items-center gap-2">
        <button
          class="px-3 py-1.5 text-xs rounded border border-neutral-700 text-neutral-300 hover:border-neutral-500 hover:text-neutral-100 transition-colors"
          onclick={refreshQueue}
          disabled={loading}
        >
          {loading ? "Refreshing..." : "Refresh"}
        </button>
        <button
          class="px-3 py-1.5 text-xs rounded border border-red-800 text-red-300 hover:border-red-500 hover:text-red-200 transition-colors"
          onclick={interruptCurrent}
          disabled={interrupting}
        >
          {interrupting ? "Interrupting..." : "Interrupt Running"}
        </button>
      </div>
    </div>

    {#if error}
      <div class="rounded-lg border border-red-900/70 bg-red-900/20 px-3 py-2 text-xs text-red-300">{error}</div>
    {/if}

    <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
      <section class="rounded-xl border border-neutral-800 bg-neutral-900/50 p-3 space-y-2">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-medium text-neutral-200">Running</h3>
          <span class="text-xs text-neutral-500">{runningItems.length}</span>
        </div>
        {#if runningItems.length === 0}
          <p class="text-xs text-neutral-500">No running jobs.</p>
        {:else}
          <div class="space-y-2">
            {#each runningItems as item}
              <div class="rounded border border-neutral-800 bg-neutral-900 px-3 py-2">
                <p class="text-xs text-neutral-300 break-all">{item.promptId}</p>
                <p class="text-[11px] text-neutral-500 mt-0.5">{item.summary}</p>
                {#if item.mode}
                  <p class="text-[10px] text-indigo-300 mt-1">Mode: {item.mode}</p>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </section>

      <section class="rounded-xl border border-neutral-800 bg-neutral-900/50 p-3 space-y-2">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-medium text-neutral-200">Pending</h3>
          <span class="text-xs text-neutral-500">{pendingItems.length}</span>
        </div>
        {#if pendingItems.length === 0}
          <p class="text-xs text-neutral-500">No queued jobs.</p>
        {:else}
          <div class="space-y-2">
            {#each pendingItems as item}
              <div class="rounded border border-neutral-800 bg-neutral-900 px-3 py-2">
                <div class="flex items-start justify-between gap-2">
                  <div class="min-w-0">
                    <p class="text-xs text-neutral-300 break-all">{item.promptId}</p>
                    <p class="text-[11px] text-neutral-500 mt-0.5">{item.summary}</p>
                    {#if item.mode}
                      <p class="text-[10px] text-indigo-300 mt-1">Mode: {item.mode}</p>
                    {/if}
                  </div>
                  <button
                    class="shrink-0 px-2 py-1 text-[10px] rounded border border-red-800 text-red-300 hover:border-red-500 hover:text-red-200 transition-colors"
                    onclick={() => removePending(item)}
                    title="Remove from queue"
                  >
                    Remove
                  </button>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </section>
    </div>
  </div>
</div>
