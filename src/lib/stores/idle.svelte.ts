/**
 * Whether anyone is using the app right now.
 *
 * Decorative work (the tips carousel, CSS animations) stops while the app is
 * resting, so a window left open in the background costs next to no CPU or
 * GPU. App.svelte combines `resting` with what is running (a generation keeps
 * the UI awake) and applies the result. Leaf store: imports nothing.
 */

/** No input for this long and the app counts as idle. */
export const IDLE_AFTER_MS = 2 * 60_000;

const ACTIVITY_EVENTS = ["pointerdown", "pointermove", "keydown", "wheel", "touchstart"] as const;

class IdleStore {
  /** No pointer, key or wheel input for {@link IDLE_AFTER_MS}. */
  inactive = $state(false);
  /** The page is not visible (window minimised or tab in the background). */
  hidden = $state(typeof document !== "undefined" && document.visibilityState === "hidden");

  private lastActivity = Date.now();
  private timer: ReturnType<typeof setTimeout> | null = null;
  private started = false;

  /** True while nobody is looking at or using the app. */
  get resting(): boolean {
    return this.inactive || this.hidden;
  }

  start() {
    if (this.started || typeof window === "undefined") return;
    this.started = true;
    // Activity only stamps a time; the single timer below checks it, so a
    // stream of pointermove events never touches reactive state or timers.
    const onActivity = () => this.markActive();
    for (const type of ACTIVITY_EVENTS) {
      window.addEventListener(type, onActivity, { capture: true, passive: true });
    }
    window.addEventListener("focus", onActivity);
    document.addEventListener("visibilitychange", () => {
      this.hidden = document.visibilityState === "hidden";
      if (!this.hidden) this.markActive();
    });
    this.schedule(IDLE_AFTER_MS);
  }

  private markActive() {
    this.lastActivity = Date.now();
    if (this.inactive) {
      this.inactive = false;
      this.schedule(IDLE_AFTER_MS);
    }
  }

  private schedule(ms: number) {
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = null;
      const remaining = IDLE_AFTER_MS - (Date.now() - this.lastActivity);
      if (remaining > 0) this.schedule(remaining);
      else this.inactive = true;
    }, ms);
  }
}

export const idle = new IdleStore();
