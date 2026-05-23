<script lang="ts">
  import { progress } from "../../stores/progress.svelte.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";

export type MobileTab = "generate" | "gallery" | "modelhub" | "artists" | "characters" | "settings";

  interface Props {
    current: MobileTab;
    onChange: (tab: MobileTab) => void;
    showModelhub?: boolean;
  }

  let { current, onChange, showModelhub = true }: Props = $props();

  const tabs = $derived(
    (
      [
        { id: "generate", labelKey: "nav.generate" },
        { id: "gallery", labelKey: "nav.gallery" },
        ...(showModelhub ? [{ id: "modelhub", labelKey: "nav.modelhub" }] : []),
        { id: "artists", labelKey: "nav.artists" },
        { id: "characters", labelKey: "artist_gallery.tab_characters" },
        { id: "settings", labelKey: "nav.settings" },
      ] as { id: MobileTab; labelKey: string }[]
    )
  );

  function tabLabel(id: MobileTab, labelKey: string): string {
    const fallback: Record<MobileTab, string> = {
      generate: "Generate",
      gallery: "Gallery",
      modelhub: "Models",
      artists: "Artists",
      characters: "Characters",
      settings: "Settings",
    };
    const t = locale.t(labelKey);
    return t === labelKey ? fallback[id] : t;
  }
</script>

<nav
  class="shrink-0 flex items-stretch gap-0.5 bg-neutral-950/95 backdrop-blur border-t border-neutral-800 px-1 pt-1 pb-[max(env(safe-area-inset-bottom),0.25rem)] tap-highlight-none"
>
  {#each tabs as tab}
    {@const active = current === tab.id}
    <button
      type="button"
      class="touch-target flex-1 flex flex-col items-center justify-center gap-0.5 py-1.5 rounded-lg transition-colors relative
        {active ? 'text-indigo-400' : 'text-neutral-500 active:bg-neutral-800/60'}"
      onclick={() => onChange(tab.id)}
      aria-current={active ? "page" : undefined}
    >
      <span class="w-5 h-5 flex items-center justify-center">
        {#if tab.id === "generate"}
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/></svg>
        {:else if tab.id === "gallery"}
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/></svg>
        {:else if tab.id === "modelhub"}
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
        {:else if tab.id === "artists"}
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="8" r="4"/><path d="M4 21c0-4 4-7 8-7s8 3 8 7"/></svg>
        {:else if tab.id === "characters"}
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="3"/><circle cx="16" cy="8" r="3"/><path d="M2 20c0-3.5 2.5-6 6-6"/><path d="M10 20c0-3.5 2.5-6 6-6"/><path d="M14 20h8"/></svg>
        {:else}
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
        {/if}
        {#if tab.id === "generate" && progress.isGenerating}
          <span
            class="absolute top-0 right-1/4 w-2 h-2 rounded-full
              {progress.queuePosition !== null && progress.queuePosition > 0 ? 'bg-amber-500' : 'bg-indigo-400 animate-pulse'}"
          ></span>
        {:else if tab.id === "settings"}
          <span
            class="absolute top-1 right-1/4 w-1.5 h-1.5 rounded-full {connection.connected ? 'bg-green-500' : 'bg-red-500'}"
            aria-hidden="true"
          ></span>
        {/if}
      </span>
      <span class="text-[10px] font-medium leading-none">{tabLabel(tab.id, tab.labelKey)}</span>
    </button>
  {/each}
</nav>
