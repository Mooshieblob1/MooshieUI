const EXTERNAL_PROTOCOLS = new Set(["http:", "https:", "mailto:"]);

/**
 * The URL a click on `anchor` should hand to the OS browser, or null when the
 * webview should handle it. Opens links that ask for a new tab, and any web
 * link that would otherwise navigate the app's own window away.
 */
export function externalLinkTarget(
  anchor: { href: string; target: string; hasAttribute(name: string): boolean },
  appOrigin: string,
): string | null {
  if (!anchor.href || anchor.hasAttribute("download")) return null;
  let url: URL;
  try {
    url = new URL(anchor.href);
  } catch {
    return null;
  }
  if (!EXTERNAL_PROTOCOLS.has(url.protocol)) return null;
  if (url.protocol === "mailto:") return url.href;
  if (anchor.target === "_blank" || url.origin !== appOrigin) return url.href;
  return null;
}
