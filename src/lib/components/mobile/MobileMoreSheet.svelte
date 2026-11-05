<script lang="ts">
  /**
   * Everything the desktop sidebar offers that doesn't fit the phone's bottom
   * bar: the remaining pages, the Interrogate tool, notifications, bug reports
   * and the connection / version line the sidebar shows at its foot.
   */
  import { portal } from "../../utils/portal.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { notifications } from "../../stores/notifications.svelte.js";
  import { comfyuiUpdate } from "../../stores/comfyuiUpdate.svelte.js";
  import { model3d } from "../../stores/model3d.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { setForceDesktopOverride } from "../../utils/device.js";
  import MobileNavIcon from "./MobileNavIcon.svelte";
  import { navLabel, type MobileNavId } from "./mobileNav.js";

  interface Props {
    /** Destinations not on the bottom bar. */
    items: MobileNavId[];
    current: MobileNavId;
    logoUrl: string;
    appVersion: string;
    onNavigate: (id: MobileNavId) => void;
    onInterrogate: () => void;
    onReportBug: () => void;
    onclose: () => void;
  }

  let { items, current, logoUrl, appVersion, onNavigate, onInterrogate, onReportBug, onclose }: Props =
    $props();

  function run(action: () => void) {
    onclose();
    action();
  }

  function useDesktopLayout() {
    setForceDesktopOverride(true);
    location.reload();
  }

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onclose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  });
</script>

<div use:portal class="fixed inset-0 z-[140] flex flex-col justify-end tap-highlight-none" role="presentation">
  <button
    type="button"
    class="mobile-sheet-backdrop absolute inset-0 h-full w-full cursor-default bg-black/60"
    tabindex="-1"
    aria-label={locale.t("common.close")}
    onclick={onclose}
  ></button>
  <div
    class="mobile-sheet relative flex max-h-[85dvh] flex-col overflow-hidden rounded-t-3xl border-t border-neutral-700/80 bg-neutral-900 shadow-2xl shadow-black/60"
    role="dialog"
    aria-modal="true"
    aria-label={locale.t("mobile.nav.more")}
  >
    <div class="bottom-sheet-handle shrink-0"></div>

    <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 pb-[calc(env(safe-area-inset-bottom)+1rem)]">
      <!-- Identity + status, the sidebar's footer -->
      <div class="flex items-center gap-3 pb-4">
        <img src={logoUrl} alt="" class="size-10 shrink-0 rounded-xl object-contain" />
        <div class="min-w-0 flex-1">
          <p class="truncate text-sm font-semibold text-neutral-100">{locale.t("app.brand_name")}</p>
          <p class="truncate text-xs text-neutral-500">
            v{appVersion}{#if comfyuiUpdate.installed}
              · ComfyUI {comfyuiUpdate.installed}{/if}
          </p>
        </div>
        <span
          class="flex shrink-0 items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-medium {connection.connected
            ? 'border-green-700/50 bg-green-500/10 text-green-300'
            : 'border-red-700/50 bg-red-500/10 text-red-300'}"
        >
          <span class="size-1.5 rounded-full {connection.connected ? 'bg-green-400' : 'bg-red-400'}" aria-hidden="true"></span>
          {connection.connected ? locale.t("nav.connected") : locale.t("nav.disconnected")}
        </span>
      </div>

      {#if comfyuiUpdate.updateAvailable && comfyuiUpdate.target}
        <button
          type="button"
          class="mb-4 flex w-full items-center gap-3 rounded-xl border border-amber-700/50 bg-amber-500/10 px-3 py-2.5 text-left text-xs text-amber-200"
          onclick={() => run(() => onNavigate("settings"))}
        >
          <span class="flex-1">{locale.t("comfyui_update.title")}: {comfyuiUpdate.installed} → {comfyuiUpdate.target}</span>
          <svg class="size-4 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m9 18 6-6-6-6" /></svg>
        </button>
      {/if}

      <!-- Pages -->
      <div class="grid grid-cols-3 gap-2">
        {#each items as id (id)}
          {@const active = current === id}
          <button
            type="button"
            class="relative flex min-h-[5.25rem] flex-col items-center justify-center gap-2 rounded-2xl border px-1 py-3 text-center transition-colors {active
              ? 'border-indigo-500/60 bg-indigo-500/15 text-indigo-200'
              : 'border-neutral-800 bg-neutral-800/50 text-neutral-300 active:bg-neutral-800'}"
            aria-current={active ? "page" : undefined}
            onclick={() => run(() => onNavigate(id))}
          >
            <MobileNavIcon {id} class="size-6" />
            <span class="line-clamp-2 text-xs font-medium leading-tight">{navLabel(id)}</span>
            {#if id === "model3d" && model3d.busy}
              <span class="absolute top-2 right-2 size-2 rounded-full bg-indigo-400 motion-safe:animate-pulse" aria-hidden="true"></span>
            {/if}
          </button>
        {/each}
      </div>

      <!-- Tools -->
      <div class="mt-4 overflow-hidden rounded-2xl border border-neutral-800 bg-neutral-800/30">
        <button
          type="button"
          class="flex min-h-12 w-full items-center gap-3 px-4 py-3 text-left text-sm text-neutral-200 active:bg-neutral-800"
          onclick={() => run(onInterrogate)}
        >
          <MobileNavIcon id="interrogate" class="size-5 text-neutral-400" />
          <span class="flex-1">{locale.t("generation.interrogate.title")}</span>
        </button>
        <button
          type="button"
          class="flex min-h-12 w-full items-center gap-3 border-t border-neutral-800 px-4 py-3 text-left text-sm text-neutral-200 active:bg-neutral-800"
          onclick={() => run(() => (notifications.panelOpen = true))}
        >
          <MobileNavIcon id="bell" class="size-5 text-neutral-400" />
          <span class="flex-1">{locale.t("notifications.title")}</span>
          {#if notifications.hasUnread}
            <span class="min-w-5 rounded-full bg-red-500 px-1.5 py-0.5 text-center text-[11px] font-bold text-white">
              {notifications.unreadCount > 9 ? "9+" : notifications.unreadCount}
            </span>
          {/if}
        </button>
        <button
          type="button"
          class="flex min-h-12 w-full items-center gap-3 border-t border-neutral-800 px-4 py-3 text-left text-sm text-neutral-200 active:bg-neutral-800"
          onclick={() => run(onReportBug)}
        >
          <MobileNavIcon id="bug" class="size-5 text-neutral-400" />
          <span class="flex-1">{locale.t("nav.report_bug")}</span>
        </button>
        <button
          type="button"
          class="flex min-h-12 w-full items-center gap-3 border-t border-neutral-800 px-4 py-3 text-left text-sm text-neutral-200 active:bg-neutral-800"
          onclick={useDesktopLayout}
        >
          <svg class="size-5 text-neutral-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="2" y="4" width="20" height="13" rx="2" /><path d="M8 21h8M12 17v4" /></svg>
          <span class="flex-1">{locale.t("settings.appearance.layout_use_desktop")}</span>
        </button>
      </div>
    </div>
  </div>
</div>
