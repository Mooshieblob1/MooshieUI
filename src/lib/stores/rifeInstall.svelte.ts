import { checkNodeAvailable, installRife, isRifeInstalled } from "../utils/api.js";
import { ipcListen } from "../utils/ipc.js";
import { restartComfyuiAndWait } from "../utils/comfyuiRestart.js";
import { connection } from "./connection.svelte.js";
import { locale } from "./locale.svelte.js";
import { models } from "./models.svelte.js";

export const RIFE_PACKAGE_NAME = "ComfyUI-Frame-Interpolation";
export const RIFE_NODE_CLASS = "RIFE VFI";

/**
 * Lazy-install state for the frame-interpolation pack, shared by the video
 * settings panel and the player's interpolate popover so both entry points
 * install the pack identically and see the same progress.
 */
class RifeInstallStore {
  /** null until the first disk check resolves. */
  installed = $state<boolean | null>(null);
  installing = $state(false);
  step = $state("");
  message = $state("");
  error = $state<string | null>(null);
  #listening = false;

  get percent(): number {
    const stops: Record<string, number> = {
      clone: 20,
      download: 50,
      done: 65,
      restart: 80,
      verify: 95,
    };
    return stops[this.step] ?? 10;
  }

  /** A disk check rather than a cached flag, so deleting the pack re-arms it. */
  async refresh(): Promise<void> {
    this.installed = await isRifeInstalled().catch(() => false);
  }

  /** Subscribe once, however many components mount. */
  listen(): void {
    if (this.#listening) return;
    this.#listening = true;
    void ipcListen("install:progress", (event: { payload: unknown }) => {
      const data = event.payload as { node_name: string; step: string; message: string };
      if (data.node_name !== RIFE_PACKAGE_NAME) return;
      this.step = data.step;
      this.message = data.message;
    });
  }

  /** Clone the pack and fetch the checkpoint, then restart so it loads. */
  async install(): Promise<boolean> {
    if (this.installing) return false;
    this.installing = true;
    this.error = null;
    this.step = "clone";
    this.message = locale.t("generation.video.rife_install_starting");
    try {
      await installRife();

      this.step = "restart";
      this.message = locale.t("generation.video.rife_install_restarting");
      connection.connected = false;
      await restartComfyuiAndWait(
        locale.t("generation.video.rife_install_timeout"),
        locale.t("generation.video.rife_install_failed_start"),
      );

      this.step = "verify";
      this.message = locale.t("generation.video.rife_install_verifying");
      const available = await checkNodeAvailable(RIFE_NODE_CLASS).catch(() => false);
      if (!available) throw new Error(locale.t("generation.video.rife_install_not_loaded"));

      this.installed = true;
      return true;
    } catch (e) {
      this.error = String(e);
      this.installed = await isRifeInstalled().catch(() => false);
      return false;
    } finally {
      await models.refresh().catch(() => {});
      this.installing = false;
      this.step = "";
      this.message = "";
    }
  }
}

export const rifeInstall = new RifeInstallStore();
