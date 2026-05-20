/**
 * A lightweight callback registration module to avoid circular imports
 * between individual stores and the central prefsSync hub.
 *
 * Stores call triggerSync() after every local save.
 * prefsSync registers itself as the handler on initialisation.
 *
 * In Tauri desktop mode (no shared server) the handler is never registered
 * so triggerSync() is effectively a no-op — local persistence still works
 * exactly as before via ipcStore / localStorage.
 */

let _handler: (() => void) | null = null;

/** Called by prefsSync to register its debounced server-push callback. */
export function registerSyncHandler(handler: () => void): void {
  _handler = handler;
}

/**
 * Trigger a debounced push of all store state to the server.
 * Safe to call unconditionally — is a no-op until registerSyncHandler() has
 * been called, and always a no-op in pure Tauri / offline mode.
 */
export function triggerSync(): void {
  _handler?.();
}
