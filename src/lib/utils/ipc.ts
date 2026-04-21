/**
 * IPC abstraction layer — routes calls to either Tauri IPC or HTTP
 * depending on whether we're running inside the Tauri webview or a browser.
 */

/** True when running inside the Tauri desktop shell. */
export const isTauri: boolean = !!(window as any).__TAURI_INTERNALS__;

/**
 * True when served by the embedded axum server in browser mode.
 * The axum server injects `__MOOSHIE_BROWSER_MODE__` into the HTML.
 * This is false during normal Vite dev (`pnpm dev`) to avoid
 * hitting non-existent `/internal-api/` endpoints.
 */
export const isBrowserMode: boolean =
  !isTauri && !!(window as any).__MOOSHIE_BROWSER_MODE__;

type UnlistenFn = () => void;
type EventCallback = (event: { payload: any }) => void;

// ---------------------------------------------------------------------------
// Auth token management (browser mode only)
// ---------------------------------------------------------------------------

const AUTH_TOKEN_KEY = "mooshie:auth_token";
const AUTH_USER_KEY = "mooshie:auth_user";
const AUTH_REMEMBER_KEY = "mooshie:remember_me";

/** Returns the active storage backend (localStorage if "remember me", sessionStorage otherwise). */
function authStorage(): Storage {
  // If a token exists in localStorage, always use localStorage (user chose "remember me")
  if (localStorage.getItem(AUTH_TOKEN_KEY)) return localStorage;
  // If a token exists in sessionStorage, use that
  if (sessionStorage.getItem(AUTH_TOKEN_KEY)) return sessionStorage;
  // Default to sessionStorage (no remembered session)
  return sessionStorage;
}

export function getAuthToken(): string | null {
  return localStorage.getItem(AUTH_TOKEN_KEY) ?? sessionStorage.getItem(AUTH_TOKEN_KEY);
}

export function setAuthToken(token: string, rememberMe = false) {
  if (rememberMe) {
    localStorage.setItem(AUTH_TOKEN_KEY, token);
    localStorage.setItem(AUTH_REMEMBER_KEY, "1");
    sessionStorage.removeItem(AUTH_TOKEN_KEY);
  } else {
    sessionStorage.setItem(AUTH_TOKEN_KEY, token);
    localStorage.removeItem(AUTH_TOKEN_KEY);
    localStorage.removeItem(AUTH_REMEMBER_KEY);
  }
}

export function clearAuthToken() {
  localStorage.removeItem(AUTH_TOKEN_KEY);
  localStorage.removeItem(AUTH_REMEMBER_KEY);
  localStorage.removeItem(AUTH_USER_KEY);
  sessionStorage.removeItem(AUTH_TOKEN_KEY);
  sessionStorage.removeItem(AUTH_USER_KEY);
}

/** Store the currently logged-in username (for per-user localStorage isolation). */
export function setAuthUser(username: string) {
  const storage = authStorage();
  storage.setItem(AUTH_USER_KEY, username);
  // Clear from the other backend to prevent stale cross-user reads
  const other = storage === localStorage ? sessionStorage : localStorage;
  other.removeItem(AUTH_USER_KEY);
}

export function getAuthUser(): string | null {
  return localStorage.getItem(AUTH_USER_KEY) ?? sessionStorage.getItem(AUTH_USER_KEY);
}

/** Returns true if the user previously selected "Remember me". */
export function wasRememberMe(): boolean {
  return localStorage.getItem(AUTH_REMEMBER_KEY) === "1";
}

/** Build headers with auth token if present. */
export function authHeaders(extra?: Record<string, string>): Record<string, string> {
  const h: Record<string, string> = { ...extra };
  const token = getAuthToken();
  if (token) h["Authorization"] = `Bearer ${token}`;
  return h;
}

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
  if (!isBrowserMode) {
    throw new Error(`ipcInvoke("${command}"): no backend available (not Tauri, not browser mode)`);
  }
  const resp = await fetch(`/internal-api/${command}`, {
    method: "POST",
    headers: authHeaders({ "Content-Type": "application/json" }),
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
  let url = "/internal-api/_events";
  const token = getAuthToken();
  if (token) url += `?token=${encodeURIComponent(token)}`;
  browserEventSource = new EventSource(url);
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
  browserEventSource.onopen = () => {
    // Notify the app that SSE reconnected so it can immediately re-sync state
    window.dispatchEvent(new CustomEvent("mooshie:sse-reconnected"));
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
  if (!isBrowserMode) {
    // No backend — return a no-op unlisten
    return () => {};
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
  if (!isBrowserMode || heartbeatInterval) return;
  heartbeatInterval = setInterval(async () => {
    try {
      await fetch("/internal-api/_heartbeat", { method: "POST" });
    } catch {
      // Server unreachable — nothing we can do
    }
  }, 3000);

  // Also send heartbeat on page visibility changes
  document.addEventListener("visibilitychange", () => {
    // Send on both hide and show: when hiding, reset the timestamp so the
    // 120s watchdog clock starts fresh *before* browser throttling kicks in.
    fetch("/internal-api/_heartbeat", { method: "POST" }).catch(() => {});
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

/** Store abstraction — uses localStorage in browser mode, scoped per-user. */
export const ipcStore = {
  async get<T>(key: string): Promise<T | undefined> {
    if (isTauri) {
      const { load } = await import("@tauri-apps/plugin-store");
      const store = await load("store.json");
      return store.get<T>(key);
    }
    const prefix = getAuthUser() ? `mooshie:${getAuthUser()}:` : "mooshie:";
    const raw = localStorage.getItem(`${prefix}${key}`);
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
    const prefix = getAuthUser() ? `mooshie:${getAuthUser()}:` : "mooshie:";
    localStorage.setItem(`${prefix}${key}`, JSON.stringify(value));
  },
};
