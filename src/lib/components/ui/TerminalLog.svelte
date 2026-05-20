<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import { getLogs } from "../../utils/api.js";
  import { isTauri } from "../../utils/ipc.js";
  import { locale } from "../../stores/locale.svelte.js";

  let {
    onclose,
  }: {
    onclose: () => void;
  } = $props();

  type LogSource = "comfyui" | "app";

  let activeTab = $state<LogSource>("comfyui");
  let lines = $state<string[]>([]);
  let autoScroll = $state(true);
  let panelHeight = $state(260);
  let logEl = $state<HTMLElement | null>(null);
  let intervalId: ReturnType<typeof setInterval> | null = null;

  // Drag-to-resize state — plain vars, no reactivity needed
  let dragging = false;
  let dragStartY = 0;
  let dragStartHeight = 0;

  async function fetchLogs() {
    if (!isTauri) return;
    try {
      lines = await getLogs(activeTab);
    } catch {
      // silently ignore poll failures
    }
  }

  function startPolling() {
    stopPolling();
    void fetchLogs();
    intervalId = setInterval(() => void fetchLogs(), 1500);
  }

  function stopPolling() {
    if (intervalId !== null) {
      clearInterval(intervalId);
      intervalId = null;
    }
  }

  $effect(() => {
    // Re-fetch immediately when tab changes
    void fetchLogs();
  });

  $effect(() => {
    // Auto-scroll when new lines arrive
    if (autoScroll && logEl) {
      tick().then(() => {
        if (logEl) logEl.scrollTop = logEl.scrollHeight;
      });
    }
  });

  function onScroll() {
    if (!logEl) return;
    const atBottom = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 8;
    autoScroll = atBottom;
  }

  function scrollToBottom() {
    autoScroll = true;
    tick().then(() => {
      if (logEl) logEl.scrollTop = logEl.scrollHeight;
    });
  }

  function copyAll() {
    navigator.clipboard.writeText(lines.join("\n")).catch(() => {});
  }

  function clearDisplay() {
    lines = [];
  }

  // Drag-resize handle
  function onDragStart(e: MouseEvent | TouchEvent) {
    dragging = true;
    dragStartY = "touches" in e ? e.touches[0].clientY : e.clientY;
    dragStartHeight = panelHeight;

    const onMove = (ev: MouseEvent | TouchEvent) => {
      if (!dragging) return;
      const y = "touches" in ev ? ev.touches[0].clientY : ev.clientY;
      const delta = dragStartY - y;
      panelHeight = Math.max(120, Math.min(700, dragStartHeight + delta));
    };
    const onUp = () => {
      dragging = false;
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      window.removeEventListener("touchmove", onMove);
      window.removeEventListener("touchend", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    window.addEventListener("touchmove", onMove);
    window.addEventListener("touchend", onUp);
  }

  function levelClass(line: string): string {
    const lower = line.toLowerCase();
    if (lower.includes(" error") || lower.includes("[error]") || lower.includes("traceback") || lower.includes("exception")) {
      return "text-red-400";
    }
    if (lower.includes(" warn") || lower.includes("[warn]") || lower.includes("warning")) {
      return "text-amber-400";
    }
    if (lower.includes(" info") || lower.includes("[info]")) {
      return "text-neutral-200";
    }
    return "text-neutral-400";
  }

  onMount(() => {
    startPolling();
  });

  onDestroy(() => {
    stopPolling();
  });
</script>

<div
  class="flex flex-col border-t border-neutral-700 bg-neutral-950 font-mono text-xs select-text"
  style="height: {panelHeight}px; flex-shrink: 0;"
>
  <!-- Resize handle -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="h-1 bg-neutral-800 hover:bg-indigo-600 cursor-row-resize transition-colors shrink-0"
    onmousedown={onDragStart}
    ontouchstart={onDragStart}
  ></div>

  <!-- Header bar -->
  <div class="flex items-center gap-1 px-2 py-1 border-b border-neutral-800 bg-neutral-900 shrink-0">
    <!-- Tabs -->
    <button
      class="px-2.5 py-0.5 rounded text-[11px] transition-colors {activeTab === 'comfyui'
        ? 'bg-neutral-700 text-neutral-100'
        : 'text-neutral-500 hover:text-neutral-300'}"
      onclick={() => { activeTab = 'comfyui'; }}
    >{locale.t("terminal.tab_comfyui")}</button>
    <button
      class="px-2.5 py-0.5 rounded text-[11px] transition-colors {activeTab === 'app'
        ? 'bg-neutral-700 text-neutral-100'
        : 'text-neutral-500 hover:text-neutral-300'}"
      onclick={() => { activeTab = 'app'; }}
    >{locale.t("terminal.tab_app")}</button>

    <div class="flex-1"></div>

    <!-- Line count -->
    <span class="text-neutral-600 text-[10px]">{locale.t("common.lines", { count: String(lines.length) })}</span>

    <!-- Auto-scroll indicator / button -->
    <button
      class="px-1.5 py-0.5 rounded text-[10px] transition-colors {autoScroll
        ? 'text-indigo-400 hover:text-indigo-300'
        : 'text-neutral-500 hover:text-neutral-300'}"
      onclick={scrollToBottom}
      title={locale.t("terminal.scroll_bottom")}
    >{locale.t("terminal.auto_scroll")}</button>

    <!-- Copy -->
    <button
      class="px-1.5 py-0.5 rounded text-[10px] text-neutral-500 hover:text-neutral-300 transition-colors"
      onclick={copyAll}
      title={locale.t("terminal.copy_all")}
    >{locale.t("terminal.copy_btn")}</button>

    <!-- Clear -->
    <button
      class="px-1.5 py-0.5 rounded text-[10px] text-neutral-500 hover:text-neutral-300 transition-colors"
      onclick={clearDisplay}
      title={locale.t("terminal.clear")}
    >{locale.t("terminal.clear_btn")}</button>

    <!-- Close -->
    <button
      class="ml-1 w-5 h-5 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-700 transition-colors"
      onclick={onclose}
      title={locale.t("terminal.close")}
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none"
        stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
        <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
      </svg>
    </button>
  </div>

  <!-- Log lines -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="flex-1 overflow-y-auto overflow-x-auto px-3 py-1.5 leading-5"
    bind:this={logEl}
    onscroll={onScroll}
  >
    {#if lines.length === 0}
      <span class="text-neutral-600 italic">
        {activeTab === 'comfyui' ? locale.t("terminal.empty_comfyui") : locale.t("terminal.empty_app")}
      </span>
    {:else}
      {#each lines as line (line + lines.indexOf(line))}
        <div class="whitespace-pre-wrap break-all {levelClass(line)}">{line}</div>
      {/each}
    {/if}
  </div>
</div>
