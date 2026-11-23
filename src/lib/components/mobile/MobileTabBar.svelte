<script lang="ts">
  import { progress } from "../../stores/progress.svelte.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { notifications } from "../../stores/notifications.svelte.js";
  import { model3d } from "../../stores/model3d.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import MobileNavIcon from "./MobileNavIcon.svelte";
  import { navLabel, type MobileNavId } from "./mobileNav.js";

  interface Props {
    items: MobileNavId[];
    /** The open destination; anything not in `items` lights up "More". */
    current: MobileNavId;
    moreOpen: boolean;
    onChange: (id: MobileNavId) => void;
    onMore: () => void;
  }

  let { items, current, moreOpen, onChange, onMore }: Props = $props();

  const moreActive = $derived(moreOpen || !items.includes(current));
  // "More" holds Settings and the notification list now, so it carries what
  // the old Settings tab's connection dot said, plus unread notifications.
  const moreAlert = $derived(!connection.connected || notifications.hasUnread);

  function busy(id: MobileNavId): "running" | "queued" | null {
    if (id === "model3d") return model3d.busy ? "running" : null;
    if (!progress.isGenerating) return null;
    const busyId = progress.currentMode === "video" ? "video" : "generate";
    if (id !== busyId) return null;
    return progress.queuePosition !== null && progress.queuePosition > 0 ? "queued" : "running";
  }
</script>

{#snippet slot(id: MobileNavId | "more", label: string, active: boolean, onclick: () => void)}
  {@const state = id === "more" ? null : busy(id)}
  <button
    type="button"
    class="touch-target flex h-16 min-w-0 flex-col items-center justify-center gap-1 px-0.5"
    {onclick}
    aria-current={active && id !== "more" ? "page" : undefined}
    aria-expanded={id === "more" ? moreOpen : undefined}
    aria-haspopup={id === "more" ? "dialog" : undefined}
  >
    <span
      class="relative flex h-8 w-14 items-center justify-center rounded-full transition-colors duration-150 {active
        ? 'bg-indigo-500/20 text-indigo-300'
        : 'text-neutral-400 active:bg-neutral-800'}"
    >
      <MobileNavIcon id={id} class="size-[22px]" />
      {#if state}
        <span
          class="absolute top-0.5 right-2.5 size-2.5 rounded-full ring-2 ring-neutral-950 {state === 'queued'
            ? 'bg-amber-500'
            : 'bg-indigo-400 motion-safe:animate-pulse'}"
          aria-hidden="true"
        ></span>
      {:else if id === "more" && moreAlert}
        <span
          class="absolute top-0.5 right-2.5 size-2.5 rounded-full ring-2 ring-neutral-950 {connection.connected
            ? 'bg-indigo-400'
            : 'bg-red-500'}"
          aria-hidden="true"
        ></span>
      {/if}
    </span>
    <span
      class="max-w-full truncate text-[11px] leading-none {active
        ? 'font-semibold text-neutral-100'
        : 'font-medium text-neutral-500'}">{label}</span
    >
  </button>
{/snippet}

<nav
  class="grid shrink-0 border-t border-neutral-800 bg-neutral-950 pb-[env(safe-area-inset-bottom)] tap-highlight-none"
  style="grid-template-columns: repeat({items.length + 1}, minmax(0, 1fr));"
  aria-label={locale.t("mobile.nav.label")}
>
  {#each items as id (id)}
    {@render slot(id, navLabel(id), !moreOpen && current === id, () => onChange(id))}
  {/each}
  {@render slot("more", locale.t("mobile.nav.more"), moreActive, onMore)}
</nav>
