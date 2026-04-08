/**
 * IPC abstraction layer — routes calls to either Tauri IPC or HTTP
 * depending on whether we're running inside the Tauri webview or a browser.
 */

/** True when running inside the Tauri desktop shell. */
export const isTauri: boolean = !!(window as any).__TAURI_INTERNALS__;

type UnlistenFn = () => void;
type EventCallback = (event: { payload: any }) => void;

// ---------------------------------------------------------------------------
// invoke — call a backend command
// ---------------------------------------------------------------------------

export async function ipcInvoke<T = unknown>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(command, args);
  }
  const resp = await fetch(`/internal-api/${command}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(args ?? {}),
  });
  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(text || `HTTP ${resp.status}`);
  }
  // Some commands return no body (204 or empty)
  const text = await resp.text();
  if (!text) return undefined as unknown as T;
  return JSON.parse(text) as T;
}

// ---------------------------------------------------------------------------
// listen — subscribe to backend events
// ---------------------------------------------------------------------------

let browserEventSource: EventSource | null = null;
const browserListeners = new Map<string, Set<EventCallback>>();

function ensureBrowserEventSource() {
  if (browserEventSource) return;
  browserEventSource = new EventSource("/internal-api/_events");
  browserEventSource.onmessage = (msg) => {
    try {
      const parsed = JSON.parse(msg.data);
      const eventName: string = parsed.event;
      const payload = parsed.payload;
      const listeners = browserListeners.get(eventName);
      if (listeners) {
        for (const cb of listeners) {
          cb({ payload });
        }
      }
    } catch {
      // ignore malformed messages
    }
  };
  browserEventSource.onerror = () => {
    // Reconnect after a short delay
    browserEventSource?.close();
    browserEventSource = null;
    setTimeout(() => {
      if (browserListeners.size > 0) {
        ensureBrowserEventSource();
      }
    }, 2000);
  };
}

export async function ipcListen(
  event: string,
  callback: EventCallback,
): Promise<UnlistenFn> {
  if (isTauri) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen(event, callback);
  }

  // Browser mode — use SSE
  ensureBrowserEventSource();
  if (!browserListeners.has(event)) {
    browserListeners.set(event, new Set());
  }
  browserListeners.get(event)!.add(callback);

  return () => {
    const set = browserListeners.get(event);
    if (set) {
      set.delete(callback);
      if (set.size === 0) browserListeners.delete(event);
    }
    // Close SSE if no listeners remain
    if (browserListeners.size === 0 && browserEventSource) {
      browserEventSource.close();
      browserEventSource = null;
    }
  };
}

// ---------------------------------------------------------------------------
// Heartbeat — keeps the backend alive in browser mode
// ---------------------------------------------------------------------------

let heartbeatInterval: ReturnType<typeof setInterval> | null = null;

export function startHeartbeat() {
  if (isTauri || heartbeatInterval) return;
  heartbeatInterval = setInterval(async () => {
    try {
      await fetch("/internal-api/_heartbeat", { method: "POST" });
    } catch {
      // Server unreachable — nothing we can do
    }
  }, 3000);

  // Also send heartbeat on page visibility changes
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") {
      fetch("/internal-api/_heartbeat", { method: "POST" }).catch(() => {});
    }
  });

  // Send heartbeat before unload to give a final ping
  window.addEventListener("beforeunload", () => {
    // Use sendBeacon for reliability during page close
    navigator.sendBeacon("/internal-api/_heartbeat_stop");
  });
}

// ---------------------------------------------------------------------------
// Tauri plugin stubs for browser mode
// ---------------------------------------------------------------------------

/** Browser-compatible file open dialog. Returns a File or null. */
export async function ipcOpenFileDialog(options?: {
  accept?: string;
  multiple?: boolean;
}): Promise<File | null> {
  if (isTauri) {
    // In Tauri mode, callers use the Tauri dialog directly
    return null;
  }
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    if (options?.accept) input.accept = options.accept;
    if (options?.multiple) input.multiple = true;
    input.onchange = () => {
      resolve(input.files?.[0] ?? null);
    };
    input.click();
  });
}

/** Browser-compatible directory picker. Returns null (not supported in all browsers). */
export async function ipcOpenDirectoryDialog(): Promise<string | null> {
  if (isTauri) return null;
  // Use the modern Directory Picker API if available
  if ("showDirectoryPicker" in window) {
    try {
      const handle = await (window as any).showDirectoryPicker();
      return handle.name;
    } catch {
      return null;
    }
  }
  return null;
}

/** Store abstraction — uses localStorage in browser mode. */
export const ipcStore = {
  async get<T>(key: string): Promise<T | undefined> {
    if (isTauri) {
      const { load } = await import("@tauri-apps/plugin-store");
      const store = await load("store.json");
      return store.get<T>(key);
    }
    const raw = localStorage.getItem(`mooshie:${key}`);
    return raw ? JSON.parse(raw) : undefined;
  },
  async set(key: string, value: unknown): Promise<void> {
    if (isTauri) {
      const { load } = await import("@tauri-apps/plugin-store");
      const store = await load("store.json");
      await store.set(key, value);
      await store.save();
      return;
    }
    localStorage.setItem(`mooshie:${key}`, JSON.stringify(value));
  },
};
