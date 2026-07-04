<script lang="ts">
  import { notifications } from "../../stores/notifications.svelte.js";
  import type { Notification } from "../../stores/notifications.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import {
    formatNotificationTime,
    notificationBody,
    notificationTitle,
  } from "../../utils/notificationI18n.js";

  interface Props {
    onOpenSettings?: () => void;
  }
  let { onOpenSettings }: Props = $props();

  // Full-view modal for a clicked notification (long bodies get clamped in the list).
  let openedNotification = $state<Notification | null>(null);

  // Bodies longer than this are likely clamped by line-clamp-2, so show a read-more hint.
  const READ_MORE_THRESHOLD = 120;

  function openNotification(notif: Notification) {
    openedNotification = notif;
    void notifications.markRead(notif.id);
  }

  function isPasswordMigration(notif: Notification): boolean {
    return notif.i18n === true && notif.title === "notifications.legacy_password_migration.title";
  }

  function isComfyuiOutdated(notif: Notification): boolean {
    return notif.i18n === true && notif.title === "notifications.comfyui_outdated.title";
  }

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
            {@const body = notificationBody(notif)}
            <div
              class="flex w-full items-start gap-2 px-4 py-3 text-left transition-colors hover:bg-neutral-800/50 {notif.read ? 'opacity-60' : ''}"
            >
              <button
                type="button"
                class="flex min-w-0 flex-1 items-start gap-2 text-left focus:outline-none"
                onclick={() => openNotification(notif)}
              >
                <span class="text-xs mt-0.5 {kindColor(notif.kind)}">{kindIcon(notif.kind)}</span>
                <div class="min-w-0 flex-1">
                  <p class="text-xs font-medium text-neutral-200 truncate">{notificationTitle(notif)}</p>
                  {#if body}
                    <p class="text-[11px] text-neutral-400 mt-0.5 line-clamp-2">{body}</p>
                    {#if body.length > READ_MORE_THRESHOLD}
                      <p class="text-[10px] text-indigo-400 mt-0.5">{locale.t("notifications.read_more")}</p>
                    {/if}
                  {/if}
                  <p class="text-[10px] text-neutral-600 mt-1">{formatNotificationTime(notif.created_at)}</p>
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

  {#if openedNotification}
    {@const notif = openedNotification}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="fixed inset-0 z-60 flex items-center justify-center bg-black/70"
      onmousedown={(e) => { if (e.target === e.currentTarget) openedNotification = null; }}
      onkeydown={(e) => { if (e.key === "Escape") openedNotification = null; }}
    >
      <div class="relative w-full max-w-md max-h-[80vh] overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 shadow-2xl m-4 p-5 space-y-3">
        <button
          class="absolute top-3 right-3 w-7 h-7 flex items-center justify-center rounded-full bg-black/50 text-neutral-300 hover:text-white hover:bg-black/70 transition-colors text-sm"
          onclick={() => (openedNotification = null)}
          aria-label={locale.t("common.close")}
        >
          ✕
        </button>
        <div class="flex items-start gap-2 pr-8">
          <span class="text-sm mt-0.5 {kindColor(notif.kind)}">{kindIcon(notif.kind)}</span>
          <h3 class="text-sm font-semibold text-neutral-100 wrap-break-word">{notificationTitle(notif)}</h3>
        </div>
        {#if notificationBody(notif)}
          <p class="text-xs leading-relaxed text-neutral-300 whitespace-pre-wrap wrap-break-word">{notificationBody(notif)}</p>
        {/if}
        <p class="text-[10px] text-neutral-600">{formatNotificationTime(notif.created_at)}</p>
        <div class="flex items-center justify-end gap-2 pt-1">
          {#if (isPasswordMigration(notif) || isComfyuiOutdated(notif)) && onOpenSettings}
            <button
              class="px-3 py-1.5 text-xs rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white transition-colors"
              onclick={() => {
                openedNotification = null;
                notifications.panelOpen = false;
                onOpenSettings?.();
              }}
            >
              {locale.t("notifications.open_settings")}
            </button>
          {/if}
          <button
            class="px-3 py-1.5 text-xs rounded-lg border border-neutral-700 text-neutral-300 hover:border-neutral-500 hover:text-neutral-100 transition-colors"
            onclick={() => (openedNotification = null)}
          >
            {locale.t("common.close")}
          </button>
        </div>
      </div>
    </div>
  {/if}
</div>
