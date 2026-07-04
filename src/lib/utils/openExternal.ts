import { isTauri } from "./ipc.js";

/** Open a URL in the OS browser (Tauri) or a new tab (browser mode). */
export async function openExternalUrl(url: string): Promise<void> {
  if (isTauri) {
    const { open } = await import("@tauri-apps/plugin-shell");
    await open(url);
  } else {
    window.open(url, "_blank", "noopener,noreferrer");
  }
}
