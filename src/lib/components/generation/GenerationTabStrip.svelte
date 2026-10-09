<script lang="ts">
  import { generationTabs } from "../../stores/generationTabs.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { isTauri } from "../../utils/ipc.js";

  /**
   * Settings tabs along the top edge. Collapsed, each tab is a thin line where
   * its chip would sit, the active one lit; hovering the edge slides the chips
   * down. With a single tab there are no lines, and the hover only offers the
   * button that opens a second one.
   */

  const OPEN_DELAY_MS = 120;
  const CLOSE_DELAY_MS = 450;
  const FLASH_MS = 1200;

  let expanded = $state(false);
  let openTimer: ReturnType<typeof setTimeout> | undefined;
  let closeTimer: ReturnType<typeof setTimeout> | undefined;

  const multiple = $derived(generationTabs.count > 1);

  function scheduleOpen() {
    clearTimeout(closeTimer);
    if (expanded) return;
    clearTimeout(openTimer);
    openTimer = setTimeout(() => (expanded = true), OPEN_DELAY_MS);
  }

  function scheduleClose(delay = CLOSE_DELAY_MS) {
    clearTimeout(openTimer);
    clearTimeout(closeTimer);
    closeTimer = setTimeout(() => (expanded = false), delay);
  }

  /** Show the strip briefly after a shortcut so the user sees where they landed. */
  function flash() {
    expanded = true;
    scheduleClose(FLASH_MS);
  }

  function modeLabel(mode: string): string {
    return locale.t(`generation.mode.${mode}`);
  }

  // Desktop only: a browser keeps Ctrl+T, Ctrl+W and Ctrl+Tab for itself.
  function handleKeydown(e: KeyboardEvent) {
    if (!isTauri || !(e.ctrlKey || e.metaKey) || e.altKey) return;
    const key = e.key.toLowerCase();
    if (key === "t" && !e.shiftKey) {
      e.preventDefault();
      generationTabs.newTab();
      flash();
    } else if (key === "w" && !e.shiftKey && multiple) {
      e.preventDefault();
      generationTabs.close(generationTabs.activeId);
      flash();
    } else if (e.key === "Tab" && multiple) {
      e.preventDefault();
      generationTabs.cycle(e.shiftKey ? -1 : 1);
      flash();
    }
  }

  $effect(() => () => {
    clearTimeout(openTimer);
    clearTimeout(closeTimer);
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<div
  class="absolute inset-x-0 top-0 z-40 h-3"
  role="presentation"
  onmouseenter={scheduleOpen}
  onmouseleave={() => scheduleClose()}
>
  <!-- Collapsed: one line per tab, laid out exactly like the chips below. -->
  {#if multiple}
    <div
      class="pointer-events-none flex justify-center px-20 pt-1 transition-opacity duration-150 {expanded
        ? 'opacity-0'
        : 'opacity-100'}"
      aria-hidden="true"
    >
      <div class="flex min-w-0 max-w-full gap-1 border border-transparent px-1.5">
      {#each generationTabs.tabs as tab (tab.id)}
        <div class="flex w-40 min-w-0 shrink justify-center">
          <div
            class="w-3/4 rounded-full transition-all {tab.id === generationTabs.activeId
              ? 'h-1 bg-indigo-500'
              : 'h-0.5 bg-neutral-600'}"
          ></div>
        </div>
      {/each}
      <div class="w-7 shrink-0"></div>
      </div>
    </div>
  {/if}

  <!-- Expanded: the chips themselves. -->
  <div
    class="absolute inset-x-0 top-0 flex justify-center px-20 transition-all duration-150 {expanded
      ? 'translate-y-0 opacity-100'
      : 'pointer-events-none -translate-y-full opacity-0'}"
    inert={!expanded}
    onfocusin={() => {
      clearTimeout(closeTimer);
      expanded = true;
    }}
    onfocusout={() => scheduleClose()}
  >
    <div
      class="flex min-w-0 max-w-full items-center gap-1 rounded-b-xl border border-t-0 border-neutral-800 bg-neutral-900/95 px-1.5 py-1.5 shadow-2xl shadow-black/40 backdrop-blur"
      role="tablist"
      aria-label={locale.t("generation.tabs.label")}
    >
      {#if multiple}
        {#each generationTabs.tabs as tab, index (tab.id)}
          {@const active = tab.id === generationTabs.activeId}
          {@const info = generationTabs.label(tab)}
          <div
            class="group flex w-40 min-w-0 shrink items-center rounded-lg border transition-colors {active
              ? 'border-indigo-500/60 bg-neutral-800 text-neutral-100'
              : 'border-transparent text-neutral-400 hover:bg-neutral-800/70 hover:text-neutral-200'}"
          >
            <button
              type="button"
              role="tab"
              aria-selected={active}
              class="flex min-w-0 flex-1 items-center gap-1.5 py-1 pl-2 text-left text-xs"
              title={info.prompt || modeLabel(info.mode)}
              onclick={() => generationTabs.switchTo(tab.id)}
            >
              <span class="shrink-0 tabular-nums {active ? 'text-indigo-400' : 'text-neutral-500'}">{index + 1}</span>
              <span class="truncate">{info.prompt || locale.t("generation.tabs.empty_prompt")}</span>
            </button>
            <button
              type="button"
              class="mr-0.5 flex size-5 shrink-0 items-center justify-center rounded text-neutral-500 opacity-60 transition hover:bg-neutral-700 hover:text-neutral-100 group-hover:opacity-100"
              title={locale.t("generation.tabs.close")}
              aria-label={locale.t("generation.tabs.close")}
              onclick={() => generationTabs.close(tab.id)}
            >
              <svg class="size-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true">
                <path stroke-linecap="round" d="M6 6l12 12M18 6L6 18" />
              </svg>
            </button>
          </div>
        {/each}
      {/if}
      <button
        type="button"
        class="flex h-7 shrink-0 items-center justify-center gap-1 rounded-lg text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-100 {multiple
          ? 'w-7'
          : 'px-2.5 text-xs'}"
        title={isTauri ? `${locale.t("generation.tabs.new_hint")} (Ctrl+T)` : locale.t("generation.tabs.new_hint")}
        aria-label={locale.t("generation.tabs.new")}
        onclick={() => generationTabs.newTab()}
      >
        <svg class="size-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true">
          <path stroke-linecap="round" d="M12 5v14M5 12h14" />
        </svg>
        {#if !multiple}<span>{locale.t("generation.tabs.new")}</span>{/if}
      </button>
    </div>
  </div>
</div>
