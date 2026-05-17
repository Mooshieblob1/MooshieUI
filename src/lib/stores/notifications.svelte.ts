import { isBrowserMode, authHeaders } from "../utils/ipc.js";

export interface Notification {
  id: string;
  target: string;
  title: string;
  body?: string;
  kind: string;
  read: boolean;
  created_at: string;
}

class NotificationStore {
  notifications = $state<Notification[]>([]);
  unreadCount = $state(0);
  panelOpen = $state(false);
  private pollInterval: ReturnType<typeof setInterval> | null = null;

  get hasUnread(): boolean {
    return this.unreadCount > 0;
  }

  async fetchNotifications() {
    if (!isBrowserMode) {
      // In desktop mode, we don't have the notification API yet
      return;
    }
    try {
      const resp = await fetch("/internal-api/_notifications", {
        headers: authHeaders(),
      });
      if (!resp.ok) return;
      const data = await resp.json();
      this.notifications = data.notifications ?? [];
    } catch {
      // Non-critical
    }
  }

  async fetchUnreadCount() {
    if (!isBrowserMode) {
      this.unreadCount = 0;
      return;
    }
    try {
      const resp = await fetch("/internal-api/_notifications/unread_count", {
        headers: authHeaders(),
      });
      if (!resp.ok) return;
      const data = await resp.json();
      this.unreadCount = data.unread_count ?? 0;
    } catch {
      // Non-critical
    }
  }

  async markRead(notificationId: string) {
    if (!isBrowserMode) return;
    try {
      await fetch("/internal-api/_notifications/mark_read", {
        method: "POST",
        headers: authHeaders({ "Content-Type": "application/json" }),
        body: JSON.stringify({ notification_id: notificationId }),
      });
      const notif = this.notifications.find((n) => n.id === notificationId);
      if (notif) notif.read = true;
      this.unreadCount = Math.max(0, this.unreadCount - 1);
    } catch {
      // Non-critical
    }
  }

  async markAllRead() {
    if (!isBrowserMode) return;
    try {
      await fetch("/internal-api/_notifications/mark_all_read", {
        method: "POST",
        headers: authHeaders(),
      });
      for (const n of this.notifications) n.read = true;
      this.unreadCount = 0;
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
