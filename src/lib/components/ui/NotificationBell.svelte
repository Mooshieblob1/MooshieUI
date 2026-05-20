<script lang="ts">
  import { notifications } from "../../stores/notifications.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";

  function kindIcon(kind: string): string {
    switch (kind) {
      case "success": return "✓";
      case "warning": return "⚠";
      case "error": return "✕";
      default: return "ℹ";
    }
  }

  function kindColor(kind: string): string {
    switch (kind) {
      case "success": return "text-emerald-400";
      case "warning": return "text-amber-400";
      case "error": return "text-red-400";
      default: return "text-indigo-400";
    }
  }

  function formatTime(iso: string): string {
    const diff = Date.now() - new Date(iso).getTime();
    if (diff < 0 || isNaN(diff)) return "";
    const sec = Math.floor(diff / 1000);
    if (sec < 60) return "Just now";
    const min = Math.floor(sec / 60);
    if (min < 60) return `${min}m ago`;
    const hrs = Math.floor(min / 60);
    if (hrs < 24) return `${hrs}h ago`;
    const days = Math.floor(hrs / 24);
    return `${days}d ago`;
  }
</script>

<div class="relative mx-auto">
  <button
    class="w-8 h-8 rounded-lg flex items-center justify-center transition-colors text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200 relative"
    onclick={() => notifications.togglePanel()}
    title={locale.t("notifications.title")}
    aria-label={locale.t("notifications.title")}
  >
    <svg
      xmlns="http://www.w3.org/2000/svg"
      class="w-4.5 h-4.5"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
    >
      <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9" />
      <path d="M13.73 21a2 2 0 0 1-3.46 0" />
    </svg>
    {#if notifications.hasUnread}
      <div
        class="absolute -top-1 -right-1 min-w-4 h-4 rounded-full bg-red-500 text-[9px] font-bold text-white flex items-center justify-center px-0.5 pointer-events-none"
      >
        {notifications.unreadCount > 9 ? "9+" : notifications.unreadCount}
      </div>
    {/if}
  </button>

  {#if notifications.panelOpen}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="fixed inset-0 z-40"
      onmousedown={() => (notifications.panelOpen = false)}
      onkeydown={(e) => { if (e.key === "Escape") notifications.panelOpen = false; }}
    ></div>
    <div
      class="absolute bottom-full left-0 mb-2 w-80 max-h-96 overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 shadow-2xl z-50"
    >
      <div class="flex items-center justify-between px-4 py-3 border-b border-neutral-800">
        <h3 class="text-sm font-semibold text-neutral-100">{locale.t("notifications.title")}</h3>
        <div class="flex items-center gap-3">
          {#if notifications.hasUnread}
            <button
              class="text-[11px] text-indigo-400 hover:text-indigo-300 transition-colors"
              onclick={() => notifications.markAllRead()}
            >
              {locale.t("notifications.mark_all_read")}
            </button>
          {/if}
          {#if notifications.notifications.length > 0}
            <button
              class="text-[11px] text-neutral-400 hover:text-neutral-200 transition-colors"
              onclick={() => notifications.clearAll()}
            >
              {locale.t("notifications.clear_all")}
            </button>
          {/if}
        </div>
      </div>
      <div class="divide-y divide-neutral-800">
        {#if notifications.notifications.length === 0}
          <div class="px-4 py-6 text-center text-xs text-neutral-500">
            {locale.t("notifications.empty")}
          </div>
        {:else}
          {#each notifications.notifications as notif (notif.id)}
            <div
              class="flex w-full items-start gap-2 px-4 py-3 text-left transition-colors hover:bg-neutral-800/50 {notif.read ? 'opacity-60' : ''}"
            >
              <button
                type="button"
                class="flex min-w-0 flex-1 items-start gap-2 text-left focus:outline-none"
                onclick={() => notifications.markRead(notif.id)}
              >
                <span class="text-xs mt-0.5 {kindColor(notif.kind)}">{kindIcon(notif.kind)}</span>
                <div class="min-w-0 flex-1">
                  <p class="text-xs font-medium text-neutral-200 truncate">{notif.title}</p>
                  {#if notif.body}
                    <p class="text-[11px] text-neutral-400 mt-0.5 line-clamp-2">{notif.body}</p>
                  {/if}
                  <p class="text-[10px] text-neutral-600 mt-1">{formatTime(notif.created_at)}</p>
                </div>
                {#if !notif.read}
                  <span class="w-2 h-2 rounded-full bg-indigo-400 shrink-0 mt-1.5"></span>
                {/if}
              </button>
              <button
                type="button"
                class="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-neutral-600 transition-colors hover:bg-neutral-800 hover:text-neutral-200 focus:outline-none focus:ring-2 focus:ring-indigo-500"
                onclick={() => notifications.dismiss(notif.id)}
                aria-label={locale.t("common.dismiss_notification")}
                title={locale.t("common.dismiss_notification")}
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>
              </button>
            </div>
          {/each}
        {/if}
      </div>
    </div>
  {/if}
</div>
