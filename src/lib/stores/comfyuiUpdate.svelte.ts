import {
  getComfyuiVersion,
  startComfyui,
  updateComfyui,
  type ComfyUiVersionInfo,
} from "../utils/api.js";
import { ipcListen, isBrowserMode } from "../utils/ipc.js";
import { locale } from "./locale.svelte.js";

/** Target ref the sidebar bubble was last dismissed for. */
const BUBBLE_DISMISSED_KEY = "mooshieui_comfyui_bubble_dismissed";
/**
 * Target ref whose startup auto-update already failed once. Without this an
 * offline user would sit through a doomed git fetch on every single launch.
 */
const AUTO_UPDATE_FAILED_KEY = "mooshieui_comfyui_autoupdate_failed";

function readKey(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeKey(key: string, value: string | null): void {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    // Private-mode / blocked storage: dismissal just doesn't persist.
  }
}

/**
 * Installed-vs-pinned ComfyUI version state, shared by the sidebar badge and
 * the startup auto-update in `App.svelte`.
 *
 * MooshieUI pins the exact ComfyUI tag it was tested against (`COMFYUI_REF`),
 * so a MooshieUI release that bumps the pin leaves every existing install
 * behind. Startup closes that gap on its own; the badge and bubble are the
 * fallback for the cases it can't cover (manual-start users, a failed update).
 */
class ComfyuiUpdateStore {
  /** null until the first version check resolves. */
  info = $state<ComfyUiVersionInfo | null>(null);
  updating = $state(false);
  /** Live `setup:progress` message while an update runs. */
  progress = $state<string | null>(null);
  error = $state<string | null>(null);
  /**
   * Bubble visibility. Kept separate from `dismissedFor` so clicking the badge
   * can reopen a bubble the user dismissed without clearing the dismissal.
   */
  bubbleOpen = $state(false);
  /** Target ref the bubble was dismissed for; null when never dismissed. */
  dismissedFor = $state<string | null>(readKey(BUBBLE_DISMISSED_KEY));
  #autoOpened = false;

  /**
   * The in-app updater is desktop-only: hosted deployments ship ComfyUI baked
   * into the image and update by pulling a newer one, so an update affordance
   * there would be a dead end.
   */
  get updateAvailable(): boolean {
    return !isBrowserMode && this.info?.update_available === true;
  }

  get installed(): string | null {
    return this.info?.installed ?? null;
  }

  get target(): string {
    return this.info?.target ?? "";
  }

  get showBubble(): boolean {
    return this.updateAvailable && (this.bubbleOpen || this.updating);
  }

  /** True once a startup auto-update for this exact target has already failed. */
  get autoUpdateBlocked(): boolean {
    return !!this.info && readKey(AUTO_UPDATE_FAILED_KEY) === this.info.target;
  }

  /** Whether startup should block to bring ComfyUI up to the pinned tag. */
  get shouldAutoUpdate(): boolean {
    return this.updateAvailable && !this.autoUpdateBlocked && !this.updating;
  }

  /** Re-read the installed version. Never throws; a failure leaves the badge hidden. */
  async refresh(): Promise<void> {
    try {
      this.info = await getComfyuiVersion();
    } catch {
      this.info = null;
      return;
    }
    // Pop the bubble once per session, unless it was dismissed for this exact
    // target. A later refresh must not re-open what the user just closed.
    if (this.updateAvailable && !this.#autoOpened && this.dismissedFor !== this.info.target) {
      this.#autoOpened = true;
      this.bubbleOpen = true;
    }
  }

  /** Reopen (or hide) the bubble from the badge, leaving any dismissal intact. */
  toggleBubble(): void {
    if (this.updating) return;
    this.bubbleOpen = !this.bubbleOpen;
  }

  /**
   * Close the bubble and keep it closed until the pinned target moves again —
   * the dismissal is stored as the ref itself, not a boolean.
   */
  dismissBubble(): void {
    this.bubbleOpen = false;
    const target = this.info?.target ?? null;
    this.dismissedFor = target;
    writeKey(BUBBLE_DISMISSED_KEY, target);
  }

  /**
   * User-initiated update from the bubble. `update_comfyui` leaves ComfyUI
   * stopped, so restart it here to get the full websocket wiring back.
   */
  async update(): Promise<boolean> {
    return this.#run(true, false);
  }

  /**
   * Startup update, run while the app is already interaction-locked. ComfyUI is
   * left stopped for `initApp` to start as it normally would.
   */
  async autoUpdateOnStartup(onProgress?: (message: string) => void): Promise<boolean> {
    return this.#run(false, true, onProgress);
  }

  /**
   * Mirror progress into the store (for the bubble) and, when the startup path
   * supplied one, into the caller's banner as well.
   */
  #setProgress(message: string, onProgress?: (message: string) => void): void {
    this.progress = message;
    onProgress?.(message);
  }

  async #run(
    restart: boolean,
    auto: boolean,
    onProgress?: (message: string) => void,
  ): Promise<boolean> {
    if (isBrowserMode) return false;
    if (this.updating) return false;
    this.updating = true;
    this.error = null;
    this.#setProgress(locale.t("settings.performance.comfyui_update_starting"), onProgress);
    const unlisten = await ipcListen("setup:progress", (event: { payload: unknown }) => {
      const data = event.payload as { message?: string };
      if (data?.message) this.#setProgress(data.message, onProgress);
    });
    try {
      await updateComfyui();
      writeKey(AUTO_UPDATE_FAILED_KEY, null);
      if (restart) {
        this.#setProgress(locale.t("comfyui_update.restarting"), onProgress);
        await startComfyui();
      }
      await this.refresh();
      this.bubbleOpen = false;
      return true;
    } catch (e) {
      this.error =
        (typeof e === "string" ? e : e instanceof Error ? e.message : null) ||
        locale.t("settings.performance.comfyui_update_failed");
      // Only the automatic path arms the skip flag; a failed manual retry
      // should never disable the next launch's attempt.
      if (auto) writeKey(AUTO_UPDATE_FAILED_KEY, this.info?.target ?? null);
      // Surface the failure in the bubble rather than silently swallowing it.
      this.bubbleOpen = true;
      return false;
    } finally {
      this.updating = false;
      this.progress = null;
      unlisten();
    }
  }
}

export const comfyuiUpdate = new ComfyuiUpdateStore();
