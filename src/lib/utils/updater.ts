import type { Update } from "@tauri-apps/plugin-updater";

/** GitHub releases page, offered when the in-app updater can't reach GitHub. */
export const RELEASES_PAGE_URL = "https://github.com/Mooshieblob1/MooshieUI/releases/latest";

/** Waits between update check attempts. One entry per retry. */
const RETRY_DELAYS_MS = [2000, 5000];

/**
 * Check for a desktop update, retrying a few times.
 *
 * The updater fetches latest.json through github.com redirects. A single
 * non-2xx answer on the way (a rate limit, a CDN hiccup) makes the plugin
 * fail with "Could not fetch a valid release JSON from the remote", even
 * though the file is fine a moment later.
 */
export async function checkForAppUpdate(): Promise<Update | null> {
  const { check } = await import("@tauri-apps/plugin-updater");
  let lastError: unknown;
  for (let attempt = 0; attempt <= RETRY_DELAYS_MS.length; attempt++) {
    if (attempt > 0) {
      await new Promise((r) => setTimeout(r, RETRY_DELAYS_MS[attempt - 1]));
    }
    try {
      return await check();
    } catch (e) {
      lastError = e;
      console.warn(`[Updater] Update check attempt ${attempt + 1} failed:`, e);
    }
  }
  throw lastError;
}

/** True when the updater could not get latest.json from GitHub at all. */
export function isUpdateServerUnreachable(error: unknown): boolean {
  const text = String(error).toLowerCase();
  return (
    text.includes("valid release json") ||
    text.includes("error sending request") ||
    text.includes("timed out") ||
    text.includes("dns error") ||
    text.includes("connection")
  );
}
