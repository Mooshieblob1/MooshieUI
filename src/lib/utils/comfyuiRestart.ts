import { ipcListen } from "./ipc.js";
import { startComfyui, stopComfyui } from "./api.js";

/**
 * Bounce ComfyUI and resolve once it reports ready.
 *
 * A freshly cloned custom-node pack is not importable until the server
 * re-scans `custom_nodes`, so every lazy node install ends here. The caller
 * clears its own connection state first; this stays free of store imports so
 * any installer can reuse it.
 */
export async function restartComfyuiAndWait(
  timeoutMessage: string,
  startFailedMessage: string,
): Promise<void> {
  await stopComfyui();
  await startComfyui();

  await new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error(timeoutMessage)), 120_000);
    const unlistenReady = ipcListen("comfyui:server_ready", () => {
      clearTimeout(timeout);
      void unlistenReady.then((fn) => fn());
      void unlistenError.then((fn) => fn());
      resolve();
    });
    const unlistenError = ipcListen("comfyui:server_error", (event: any) => {
      clearTimeout(timeout);
      void unlistenReady.then((fn) => fn());
      void unlistenError.then((fn) => fn());
      reject(new Error(event.payload?.error || startFailedMessage));
    });
  });
}
