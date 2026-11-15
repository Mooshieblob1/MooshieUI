import type { Update } from "@tauri-apps/plugin-updater";
import { getAppPackageManager } from "./api.js";

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

/** What to run to update an install the system package manager owns. */
const PACKAGE_MANAGER_UPDATE_COMMANDS: Record<string, string> = {
  apt: "sudo apt update && sudo apt upgrade",
  dnf: "sudo dnf upgrade --refresh mooshieui",
  zypper: "sudo zypper refresh && sudo zypper update mooshieui",
  pacman: "yay -Syu mooshieui-bin",
};

let packageManagerCommand: Promise<string | null> | null = null;

/**
 * The update command for an install from the MooshieUI apt/rpm repo or the
 * AUR, where the in-app updater must stay out of the package manager's way.
 * Null when the app updates itself.
 */
export function packageManagerUpdateCommand(): Promise<string | null> {
  packageManagerCommand ??= getAppPackageManager()
    .then((manager) => (manager && PACKAGE_MANAGER_UPDATE_COMMANDS[manager]) || null)
    .catch((e) => {
      console.warn("[Updater] Could not tell whether a package manager owns this install:", e);
      return null;
    });
  return packageManagerCommand;
}
