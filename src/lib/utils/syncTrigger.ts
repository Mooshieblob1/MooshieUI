/**
 * A lightweight callback registration module to avoid circular imports
 * between individual stores and the central prefsSync hub.
 *
 * Stores call triggerSync() after every local save.
 * prefsSync registers itself as the handler on initialisation.
 *
 * Desktop and browser mode both register it; the pushes land in the one
 * shared copy of the user's data that both modes read.
 */

let _handler: (() => void) | null = null;

/** Called by prefsSync to register its debounced server-push callback. */
export function registerSyncHandler(handler: () => void): void {
  _handler = handler;
}

/**
 * Trigger a debounced push of changed store state to the shared copy.
 * Safe to call unconditionally — is a no-op until registerSyncHandler() has
 * been called (and in plain Vite dev, where there is no backend).
 */
export function triggerSync(): void {
  _handler?.();
}
