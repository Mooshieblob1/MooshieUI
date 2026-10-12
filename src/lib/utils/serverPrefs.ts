/**
 * Per-user preferences, stored by the backend in one file per account.
 *
 * Desktop mode reads and writes it through Tauri commands; browser mode
 * through `/internal-api/_user/prefs`. On one machine both land on the same
 * owner file, so characters, presets, styles and settings are one copy
 * whichever mode is open. The webview's own storage (localStorage / the Tauri
 * store) is only a local cache of it.
 */

import { authHeaders, ipcInvoke, isBrowserMode, isTauri } from "./ipc.js";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface UserPrefsData {
  generation?: Record<string, unknown>;
  prompt_history?: unknown[];
  prompt_presets?: unknown;
  styles?: unknown;
  lora_presets?: unknown;
  artist_favourites?: unknown;
  gallery_boards?: unknown;
  autocomplete?: unknown;
  accessibility?: unknown;
  notes?: unknown;
  characters?: unknown;
  video_timeline?: unknown;
  locale?: string;
  artist_local_previews?: unknown;
  enhancer_sessions?: unknown;
  style_creator?: unknown;
  anime_scene?: unknown;
  /** Set once the desktop app has merged its own data in (see `prefsSync`). */
  desktop_merged?: boolean;
  /** Set by the server on every save. */
  updated_at?: string;
  /**
   * Marks the one-time merge push, so the server keeps a copy of the file as
   * it was before. Never stored.
   */
  merge_source?: "desktop" | "browser";
}

/** Outcome of a fetch: `empty` means nothing saved yet, `unavailable` that we could not tell. */
export type FetchPrefsResult =
  | { status: "ok"; prefs: UserPrefsData }
  | { status: "empty" }
  | { status: "unavailable" };

/** Whether this build talks to a backend that stores prefs at all. */
export const serverPrefsAvailable: boolean = isTauri || isBrowserMode;

// ---------------------------------------------------------------------------
// API helpers
// ---------------------------------------------------------------------------

/**
 * Fetch the current user's saved preferences. `unavailable` covers a network
 * failure and a LAN visitor who has not signed in, so callers never mistake
 * an outage for "nothing saved" and seed over real data.
 */
export async function fetchServerPrefs(): Promise<FetchPrefsResult> {
  if (isTauri) {
    try {
      const prefs = await ipcInvoke<UserPrefsData | null>("get_user_prefs");
      return prefs ? { status: "ok", prefs } : { status: "empty" };
    } catch (e) {
      console.warn("[prefsSync] could not read saved prefs:", e);
      return { status: "unavailable" };
    }
  }
  if (!isBrowserMode) return { status: "unavailable" };
  try {
    // No token is fine: the server treats localhost as the machine owner and
    // answers 403 for an anonymous LAN visitor.
    const resp = await fetch("/internal-api/_user/prefs", { headers: authHeaders() });
    if (resp.status === 204 || resp.status === 404) return { status: "empty" };
    if (!resp.ok) return { status: "unavailable" };
    return { status: "ok", prefs: (await resp.json()) as UserPrefsData };
  } catch {
    return { status: "unavailable" };
  }
}

/**
 * Save preferences. Categories left out of `prefs` keep their stored value.
 * Never throws; returns the server's new `updated_at`, or null on failure.
 */
export async function pushServerPrefs(
  prefs: UserPrefsData,
  options: { keepalive?: boolean } = {},
): Promise<string | null> {
  if (isTauri) {
    try {
      return await ipcInvoke<string>("save_user_prefs", { prefs });
    } catch (e) {
      console.warn("[prefsSync] save failed:", e);
      return null;
    }
  }
  if (!isBrowserMode) return null;
  try {
    const resp = await fetch("/internal-api/_user/prefs", {
      method: "PUT",
      headers: authHeaders({ "Content-Type": "application/json" }),
      body: JSON.stringify(prefs),
      // Lets a push started from `pagehide` outlive the page. Browsers cap
      // keepalive bodies (64 KB), so it is opt-in for that one case rather
      // than the default.
      keepalive: options.keepalive === true,
    });
    if (!resp.ok) return null;
    const body = (await resp.json().catch(() => null)) as { updated_at?: unknown } | null;
    return typeof body?.updated_at === "string" ? body.updated_at : "";
  } catch (e) {
    console.warn("[prefsSync] server push failed:", e);
    return null;
  }
}
