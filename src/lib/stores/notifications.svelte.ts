import { isBrowserMode, authHeaders } from "../utils/ipc.js";

const LOCAL_NOTIFICATIONS_KEY = "mooshie-local-notifications";
const MAX_LOCAL_NOTIFICATIONS = 100;

export interface Notification {
  id: string;
  target: string;
  title: string;
  body?: string;
  kind: string;
  read: boolean;
  created_at: string;
  local?: boolean;
  /** When true, title/body are locale keys; params supplies interpolation. */
  i18n?: boolean;
  params?: Record<string, unknown>;
}

type NotificationInput = {
  title: string;
  body?: string;
  kind?: string;
  target?: string;
};

function loadLocalNotifications(): Notification[] {
  try {
    const raw = globalThis.localStorage?.getItem(LOCAL_NOTIFICATIONS_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((n): n is Notification => (
        typeof n?.id === "string" &&
        typeof n?.title === "string" &&
        typeof n?.kind === "string" &&
        typeof n?.created_at === "string"
      ))
      .map((n) => ({
        ...n,
        target: n.target ?? "local",
        read: Boolean(n.read),
        local: true,
      }));
  } catch {
    return [];
  }
}

function saveLocalNotifications(notifications: Notification[]) {
  try {
    globalThis.localStorage?.setItem(LOCAL_NOTIFICATIONS_KEY, JSON.stringify(notifications));
  } catch {
    // Non-critical
  }
}

function sortNotifications(notifications: Notification[]): Notification[] {
  return [...notifications].sort((a, b) => {
    const readOrder = Number(a.read) - Number(b.read);
    if (readOrder !== 0) return readOrder;
    return new Date(b.created_at).getTime() - new Date(a.created_at).getTime();
  });
}

class NotificationStore {
  private remoteNotifications = $state<Notification[]>([]);
  private localNotifications = $state<Notification[]>(loadLocalNotifications());
  private remoteUnreadCount = $state(0);
  panelOpen = $state(false);
  private pollInterval: ReturnType<typeof setInterval> | null = null;

  get notifications(): Notification[] {
    return sortNotifications([...this.localNotifications, ...this.remoteNotifications]);
  }

  get unreadCount(): number {
    return this.localNotifications.filter((n) => !n.read).length + this.remoteUnreadCount;
  }

  get hasUnread(): boolean {
    return this.unreadCount > 0;
  }

  async fetchNotifications() {
    if (!isBrowserMode) {
      this.remoteNotifications = [];
      this.remoteUnreadCount = 0;
      return;
    }
    try {
      const resp = await fetch("/internal-api/_notifications", {
        headers: authHeaders(),
      });
      if (!resp.ok) return;
      const data = await resp.json();
      this.remoteNotifications = data.notifications ?? [];
      this.remoteUnreadCount = this.remoteNotifications.filter((n) => !n.read).length;
    } catch {
      // Non-critical
    }
  }

  async fetchUnreadCount() {
    if (!isBrowserMode) {
      this.remoteUnreadCount = 0;
      return;
    }
    try {
      const resp = await fetch("/internal-api/_notifications/unread_count", {
        headers: authHeaders(),
      });
      if (!resp.ok) return;
      const data = await resp.json();
      this.remoteUnreadCount = data.unread_count ?? 0;
    } catch {
      // Non-critical
    }
  }

  addLocalNotification(input: NotificationInput): Notification {
    const notification: Notification = {
      id: `local_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`,
      target: input.target ?? "local",
      title: input.title,
      body: input.body,
      kind: input.kind ?? "info",
      read: false,
      created_at: new Date().toISOString(),
      local: true,
    };
    this.localNotifications = [notification, ...this.localNotifications].slice(0, MAX_LOCAL_NOTIFICATIONS);
    saveLocalNotifications(this.localNotifications);
    return notification;
  }

  async markRead(notificationId: string) {
    const localNotif = this.localNotifications.find((n) => n.id === notificationId);
    if (localNotif) {
      localNotif.read = true;
      saveLocalNotifications(this.localNotifications);
      return;
    }

    if (!isBrowserMode) return;
    try {
      await fetch("/internal-api/_notifications/mark_read", {
        method: "POST",
        headers: authHeaders({ "Content-Type": "application/json" }),
        body: JSON.stringify({ notification_id: notificationId }),
      });
      const notif = this.remoteNotifications.find((n) => n.id === notificationId);
      const wasUnread = notif ? !notif.read : false;
      if (notif) notif.read = true;
      if (wasUnread) this.remoteUnreadCount = Math.max(0, this.remoteUnreadCount - 1);
    } catch {
      // Non-critical
    }
  }

  async markAllRead() {
    for (const n of this.localNotifications) n.read = true;
    saveLocalNotifications(this.localNotifications);

    if (!isBrowserMode) return;
    try {
      await fetch("/internal-api/_notifications/mark_all_read", {
        method: "POST",
        headers: authHeaders(),
      });
      for (const n of this.remoteNotifications) n.read = true;
      this.remoteUnreadCount = 0;
    } catch {
      // Non-critical
    }
  }

  async dismiss(notificationId: string) {
    const localNotif = this.localNotifications.find((n) => n.id === notificationId);
    if (localNotif) {
      this.localNotifications = this.localNotifications.filter((n) => n.id !== notificationId);
      saveLocalNotifications(this.localNotifications);
      return;
    }

    if (!isBrowserMode) return;
    try {
      await fetch("/internal-api/_notifications/dismiss", {
        method: "POST",
        headers: authHeaders({ "Content-Type": "application/json" }),
        body: JSON.stringify({ notification_id: notificationId }),
      });
      const notif = this.remoteNotifications.find((n) => n.id === notificationId);
      const wasUnread = notif ? !notif.read : false;
      this.remoteNotifications = this.remoteNotifications.filter((n) => n.id !== notificationId);
      if (wasUnread) this.remoteUnreadCount = Math.max(0, this.remoteUnreadCount - 1);
    } catch {
      // Non-critical
    }
  }

  async clearAll() {
    this.localNotifications = [];
    saveLocalNotifications(this.localNotifications);

    if (!isBrowserMode) return;
    try {
      await fetch("/internal-api/_notifications/clear", {
        method: "POST",
        headers: authHeaders(),
      });
      this.remoteNotifications = [];
      this.remoteUnreadCount = 0;
    } catch {
      // Non-critical
    }
  }

  togglePanel() {
    this.panelOpen = !this.panelOpen;
    if (this.panelOpen) {
      this.fetchNotifications();
    }
  }

  startPolling() {
    if (!isBrowserMode) return;
    this.fetchUnreadCount();
    this.pollInterval = setInterval(() => {
      this.fetchUnreadCount();
    }, 30000); // Poll every 30s
  }

  stopPolling() {
    if (this.pollInterval) {
      clearInterval(this.pollInterval);
      this.pollInterval = null;
    }
  }
}

export const notifications = new NotificationStore();
