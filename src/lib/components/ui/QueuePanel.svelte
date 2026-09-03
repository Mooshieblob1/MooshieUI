<script lang="ts">
  import { queue } from "../../stores/queue.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import type { QueuePanelRow } from "../../stores/queue.svelte.js";

  const rows = $derived(queue.rows);
  const pending = $derived(queue.pendingRows);
  const running = $derived(queue.runningRow);
  const isEmpty = $derived(rows.length === 0);

  function handleReorderFront(row: QueuePanelRow) {
    void queue.reorder(row.promptId, 0);
  }
  function handleMoveUp(row: QueuePanelRow) {
    void queue.reorder(row.promptId, Math.max(0, row.userPosition - 1));
  }
  function handleMoveDown(row: QueuePanelRow) {
    void queue.reorder(row.promptId, row.userPosition + 1);
  }
  function handleCancel(row: QueuePanelRow) {
    void queue.cancel(row.promptId);
  }
  function handleInterrupt() {
    void queue.interrupt();
  }
  function handleClearPending() {
    void queue.clearPending();
  }
</script>

{#if queue.panelOpen}
  <!-- Backdrop -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-40"
    onmousedown={() => queue.closePanel()}
    onkeydown={(e) => { if (e.key === "Escape") queue.closePanel(); }}
  ></div>

  <!-- Panel -->
  <div
    class="absolute bottom-full right-0 mb-2 w-96 max-h-[28rem] flex flex-col rounded-xl border border-neutral-700 bg-neutral-900 shadow-2xl z-50"
    role="dialog"
    aria-label={locale.t("queue.panel.title")}
  >
    <!-- Header -->
    <div class="flex items-center justify-between px-4 py-3 border-b border-neutral-800 shrink-0">
      <h3 class="text-sm font-semibold text-neutral-100">{locale.t("queue.panel.title")}</h3>
      <div class="flex items-center gap-2">
        {#if running}
          <button
            class="text-[11px] text-amber-400 hover:text-amber-300 transition-colors"
            onclick={handleInterrupt}
          >
            {locale.t("queue.panel.interrupt")}
          </button>
        {/if}
        {#if pending.length > 0}
          <button
            class="text-[11px] text-neutral-400 hover:text-neutral-200 transition-colors"
            onclick={handleClearPending}
          >
            {locale.t("queue.panel.clear_pending")}
          </button>
        {/if}
      </div>
    </div>

    <!-- Error message -->
    {#if queue.errorMsg}
      <div class="px-4 py-2 bg-red-950/60 border-b border-red-800/40 text-xs text-red-300 shrink-0">
        {queue.errorMsg}
      </div>
    {/if}

    <!-- Row list -->
    <div class="overflow-y-auto flex-1 divide-y divide-neutral-800">
      {#if isEmpty}
        <div class="px-4 py-6 text-center text-xs text-neutral-500">
          {locale.t("queue.panel.empty")}
        </div>
      {:else}
        {#each rows as row (row.promptId)}
          <div class="px-4 py-3 flex items-start gap-3 hover:bg-neutral-800/40 transition-colors">
            <!-- Position badge -->
            <span class="shrink-0 text-[11px] font-mono text-neutral-500 pt-0.5 w-6 text-right">
              {#if row.running}
                <span class="text-emerald-400 font-bold">&#9654;</span>
              {:else}
                {locale.t("queue.panel.position", { pos: String(row.userPosition) })}
              {/if}
            </span>

            <!-- Content -->
            <div class="min-w-0 flex-1">
              <p class="text-xs text-neutral-200 truncate" title={row.summary}>{row.summary}</p>
              <div class="flex items-center gap-2 mt-0.5 flex-wrap">
                {#if row.modelName}
                  <span class="text-[10px] text-neutral-500 truncate max-w-[160px]" title={row.modelName}>
                    {row.modelName}
                  </span>
                {/if}
                {#if row.dimensions}
                  <span class="text-[10px] text-neutral-600">{row.dimensions}</span>
                {/if}
                {#if row.batchLabel}
                  <span class="text-[10px] text-neutral-600">{row.batchLabel}</span>
                {/if}
                {#if row.running && row.elapsedSecs != null}
                  <span class="text-[10px] text-emerald-500">
                    {locale.t("queue.panel.elapsed", { secs: String(row.elapsedSecs) })}
                  </span>
                {/if}
              </div>
            </div>

            <!-- Actions -->
            <div class="shrink-0 flex items-center gap-1">
              {#if row.running}
                <!-- Running: interrupt button -->
                <button
                  class="text-[10px] px-2 py-1 rounded bg-amber-900/40 text-amber-400 hover:bg-amber-800/50 transition-colors"
                  onclick={handleInterrupt}
                  title={locale.t("queue.panel.interrupt")}
                >
                  {locale.t("queue.panel.interrupt")}
                </button>
              {:else}
                <!-- Pending: reorder + cancel -->
                {#if row.userPosition > 1}
                  <button
                    class="p-1 rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-700 transition-colors"
                    onclick={() => handleReorderFront(row)}
                    title={locale.t("queue.panel.move_to_front")}
                    aria-label={locale.t("queue.panel.move_to_front")}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                      <polyline points="17 11 12 6 7 11"/><polyline points="17 18 12 13 7 18"/>
                    </svg>
                  </button>
                {/if}
                {#if row.userPosition > 0}
                  <button
                    class="p-1 rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-700 transition-colors"
                    onclick={() => handleMoveUp(row)}
                    title={locale.t("queue.panel.move_up")}
                    aria-label={locale.t("queue.panel.move_up")}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                      <polyline points="18 15 12 9 6 15"/>
                    </svg>
                  </button>
                {/if}
                {#if row.userPosition < pending.length - 1}
                  <button
                    class="p-1 rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-700 transition-colors"
                    onclick={() => handleMoveDown(row)}
                    title={locale.t("queue.panel.move_down")}
                    aria-label={locale.t("queue.panel.move_down")}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                      <polyline points="6 9 12 15 18 9"/>
                    </svg>
                  </button>
                {/if}
                <button
                  class="p-1 rounded text-neutral-500 hover:text-red-400 hover:bg-neutral-700 transition-colors"
                  onclick={() => handleCancel(row)}
                  title={locale.t("queue.panel.cancel")}
                  aria-label={locale.t("queue.panel.cancel")}
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                    <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
                  </svg>
                </button>
              {/if}
            </div>
          </div>
        {/each}
      {/if}
    </div>
  </div>
{/if}
