/**
 * OS-level notification abstraction.
 *
 * Desktop (Tauri): uses tauri-plugin-notification via dynamic import so the
 * import never throws in browser mode.
 *
 * Browser mode: uses the Web Notifications API.
 */
import { isTauri } from "./ipc.js";

/** Result of requestOsNotificationPermission. */
export type OsNotificationPermission = "granted" | "denied" | "default";

/**
 * Request permission to show OS notifications.
 * Returns the resulting permission state.
 */
export async function requestOsNotificationPermission(): Promise<OsNotificationPermission> {
  if (isTauri) {
    try {
      const { isPermissionGranted, requestPermission } = await import(
        "@tauri-apps/plugin-notification"
      );
      const granted = await isPermissionGranted();
      if (granted) return "granted";
      const perm = await requestPermission();
      return perm === "granted" ? "granted" : "denied";
    } catch {
      return "denied";
    }
  }

  // Browser mode — Web Notifications API
  if (!("Notification" in window)) return "denied";
  if (Notification.permission === "granted") return "granted";
  if (Notification.permission === "denied") return "denied";
  try {
    const result = await Notification.requestPermission();
    return result as OsNotificationPermission;
  } catch {
    return "denied";
  }
}

/**
 * Show an OS notification.
 * Silently no-ops if permission is not granted or the API is unavailable.
 */
export async function notifyOs(opts: { title: string; body?: string }): Promise<void> {
  if (isTauri) {
    try {
      const { isPermissionGranted, sendNotification } = await import(
        "@tauri-apps/plugin-notification"
      );
      const granted = await isPermissionGranted();
      if (!granted) return;
      sendNotification({ title: opts.title, body: opts.body });
    } catch {
      // Non-critical
    }
    return;
  }

  // Browser mode
  if (!("Notification" in window) || Notification.permission !== "granted") return;
  try {
    new Notification(opts.title, { body: opts.body });
  } catch {
    // Non-critical
  }
}

/**
 * Returns true when the window is currently unfocused / hidden.
 * Used to implement the "only when unfocused" setting.
 *
 * Desktop: uses Tauri's getCurrentWindow().isFocused() (async, guarded).
 * Browser: uses document.hasFocus() (sync).
 */
export async function isWindowUnfocused(): Promise<boolean> {
  if (isTauri) {
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const focused = await getCurrentWindow().isFocused();
      return !focused;
    } catch {
      // Fallback to document API if Tauri call fails
    }
  }
  return !document.hasFocus();
}
