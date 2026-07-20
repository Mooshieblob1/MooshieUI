/**
 * Startup interaction lock.
 *
 * Leaf store (no feature-store imports). App.svelte owns the lifecycle: it
 * engages the lock when initApp() begins and releases it once ComfyUI is
 * reachable, startup fails, or auto-start is disabled. The lock never
 * re-engages afterwards, so a mid-session reconnect can't block the UI.
 */
class StartupStore {
  /** Blocks all app content interaction while MooshieUI and ComfyUI start up. */
  locked = $state(false);
}

export const startup = new StartupStore();
