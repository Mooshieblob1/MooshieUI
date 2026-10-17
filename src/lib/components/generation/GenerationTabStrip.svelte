<script lang="ts">
  import { generationTabs } from "../../stores/generationTabs.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { isTauri } from "../../utils/ipc.js";
  import ProfilesMenu from "./ProfilesMenu.svelte";

  /**
   * Settings tabs along the top edge, always on show so the feature is found
   * without knowing it exists. A single tab still gets its chip, and the new
   * tab button spells out what it does until a second tab is open. Profiles
   * (named, saved tabs) sit at the right end.
   */

  const multiple = $derived(generationTabs.count > 1);

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
    } else if (key === "w" && !e.shiftKey && multiple) {
      e.preventDefault();
      generationTabs.close(generationTabs.activeId);
    } else if (e.key === "Tab" && multiple) {
      e.preventDefault();
      generationTabs.cycle(e.shiftKey ? -1 : 1);
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- md:pl-20 lines the chips up with the panels, past the sidebar column. -->
<div class="flex shrink-0 items-center gap-2 bg-neutral-950 px-3 pt-1.5 md:pl-20">
  <div
    class="flex min-w-0 flex-1 items-center gap-1 overflow-hidden"
    role="tablist"
    aria-label={locale.t("generation.tabs.label")}
  >
    {#each generationTabs.tabs as tab, index (tab.id)}
      {@const active = tab.id === generationTabs.activeId}
      {@const info = generationTabs.label(tab)}
      <div
        class="group flex h-7 w-44 min-w-0 shrink items-center rounded-lg border transition-colors {active
          ? 'border-indigo-500/60 bg-neutral-800 text-neutral-100'
          : 'border-neutral-800 bg-neutral-900 text-neutral-400 hover:border-neutral-700 hover:text-neutral-200'}"
      >
        <button
          type="button"
          role="tab"
          aria-selected={active}
          class="flex h-full min-w-0 flex-1 items-center gap-1.5 pl-2 text-left text-xs {multiple ? '' : 'pr-2'}"
          title={info.name || info.prompt || modeLabel(info.mode)}
          onclick={() => generationTabs.switchTo(tab.id)}
        >
          <span class="shrink-0 tabular-nums {active ? 'text-indigo-400' : 'text-neutral-500'}">{index + 1}</span>
          {#if info.name}
            <span class="truncate font-medium">{info.name}</span>
          {:else}
            <span class="truncate">{info.prompt || locale.t("generation.tabs.empty_prompt")}</span>
          {/if}
        </button>
        {#if multiple}
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
        {/if}
      </div>
    {/each}
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
  <ProfilesMenu />
</div>
