/**
 * Server-side per-user preferences API.
 *
 * Only active in browser / LAN mode.  In Tauri desktop mode all functions
 * are no-ops and return null — local persistence via ipcStore / localStorage
 * continues unchanged.
 */

import { isBrowserMode, getAuthToken } from "./ipc.js";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface UserPrefsData {
  generation?: Record<string, unknown>;
  prompt_history?: unknown[];
  prompt_presets?: unknown;
  styles?: unknown;
  artist_favourites?: unknown;
  gallery_boards?: unknown;
  autocomplete?: unknown;
  accessibility?: unknown;
  locale?: string;
  updated_at?: string;
}

// ---------------------------------------------------------------------------
// API helpers
// ---------------------------------------------------------------------------

/**
 * Fetch the current user's saved preferences from the server.
 * Returns `null` when in Tauri mode, when unauthenticated, when no prefs
 * are saved yet (204), or on network failure.
 */
export async function fetchServerPrefs(): Promise<UserPrefsData | null> {
  if (!isBrowserMode) return null;
  const token = getAuthToken();
  // In LAN mode without a token we're anonymous — skip silently.
  if (!token) return null;
  try {
    const resp = await fetch("/internal-api/_user/prefs", {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (resp.status === 204 || resp.status === 404) return null;
    if (!resp.ok) return null;
    return (await resp.json()) as UserPrefsData;
  } catch {
    return null;
  }
}

/**
 * Push the current user's preferences to the server.
 * Fire-and-forget — never throws; failures are logged but do not block the UI.
 */
export async function pushServerPrefs(prefs: UserPrefsData): Promise<void> {
  if (!isBrowserMode) return;
  const token = getAuthToken();
  if (!token) return;
  try {
    await fetch("/internal-api/_user/prefs", {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify(prefs),
    });
  } catch (e) {
    console.warn("[prefsSync] server push failed:", e);
  }
}
