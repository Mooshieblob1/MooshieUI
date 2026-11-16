import { isTauri } from "./ipc.js";

/**
 * Plain-text clipboard for in-app menus (the prompt box's Cut / Copy /
 * Paste). The web Clipboard API first; in the desktop app the clipboard
 * plugin is the fallback for webviews that refuse it (WebKitGTK, or macOS
 * asking for a paste confirmation that the menu click does not count for).
 */

export async function writeClipboardText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    /* fall through */
  }
  if (isTauri) {
    try {
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(text);
      return true;
    } catch {
      /* fall through */
    }
  }
  return false;
}

/** The clipboard's text, or null when it cannot be read here. */
export async function readClipboardText(): Promise<string | null> {
  try {
    if (navigator.clipboard?.readText) return await navigator.clipboard.readText();
  } catch {
    /* fall through */
  }
  if (isTauri) {
    try {
      const { readText } = await import("@tauri-apps/plugin-clipboard-manager");
      return await readText();
    } catch {
      /* fall through */
    }
  }
  return null;
}
