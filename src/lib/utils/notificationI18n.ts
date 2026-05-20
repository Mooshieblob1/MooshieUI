import { locale } from "../stores/locale.svelte.js";
import type { Notification } from "../stores/notifications.svelte.js";

function paramsRecord(
  params: Record<string, unknown> | undefined,
): Record<string, string | number> | undefined {
  if (!params) return undefined;
  const out: Record<string, string | number> = {};
  for (const [k, v] of Object.entries(params)) {
    if (typeof v === "string" || typeof v === "number") out[k] = v;
    else if (v != null) out[k] = String(v);
  }
  return out;
}

export function notificationTitle(notif: Notification): string {
  if (notif.i18n) {
    return locale.t(notif.title, paramsRecord(notif.params));
  }
  return notif.title;
}

export function notificationBody(notif: Notification): string | undefined {
  if (!notif.body) return undefined;
  if (notif.i18n) {
    return locale.t(notif.body, paramsRecord(notif.params));
  }
  return notif.body;
}

export function formatNotificationTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  if (diff < 0 || isNaN(diff)) return "";
  const sec = Math.floor(diff / 1000);
  if (sec < 60) return locale.t("notifications.time.just_now");
  const min = Math.floor(sec / 60);
  if (min < 60) return locale.t("notifications.time.minutes_ago", { min });
  const hrs = Math.floor(min / 60);
  if (hrs < 24) return locale.t("notifications.time.hours_ago", { hrs });
  const days = Math.floor(hrs / 24);
  return locale.t("notifications.time.days_ago", { days });
}
