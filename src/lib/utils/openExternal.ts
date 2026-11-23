import { isTauri } from "./ipc.js";
import { externalLinkTarget } from "./externalLink.js";

/** Open a URL in the OS browser (Tauri) or a new tab (browser mode). */
export async function openExternalUrl(url: string): Promise<void> {
  if (isTauri) {
    const { open } = await import("@tauri-apps/plugin-shell");
    await open(url);
  } else {
    window.open(url, "_blank", "noopener,noreferrer");
  }
}

/**
 * The desktop webview ignores `<a target="_blank">` (nothing opens), so route
 * clicks on external links through the OS browser. Runs in the capture phase so
 * a link's own `stopPropagation()` (card links inside clickable cards) can't
 * hide the click. Browser mode keeps the browser's own link handling.
 */
export function installExternalLinkHandler(): void {
  if (!isTauri) return;
  const handle = (event: MouseEvent) => {
    if (event.defaultPrevented) return;
    if (event.type === "click" && event.button !== 0) return;
    if (event.type === "auxclick" && event.button !== 1) return;
    const anchor = (event.target as Element | null)?.closest?.("a[href]") as HTMLAnchorElement | null;
    if (!anchor) return;
    const url = externalLinkTarget(anchor, window.location.origin);
    if (!url) return;
    event.preventDefault();
    openExternalUrl(url).catch((err) => console.error("[openExternal] failed to open", url, err));
  };
  document.addEventListener("click", handle, true);
  document.addEventListener("auxclick", handle, true);
}
