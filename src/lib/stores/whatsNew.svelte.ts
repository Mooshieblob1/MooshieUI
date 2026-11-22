import allReleases from "virtual:changelog-releases";
import { isValidSemver, releasesSince, type ChangelogRelease } from "../utils/changelog.js";

const LAST_SEEN_KEY = "mooshieui.whatsNew.lastSeenVersion.v1";

function readLastSeenVersion(): string | null {
  try {
    return localStorage.getItem(LAST_SEEN_KEY);
  } catch {
    return null;
  }
}

function writeLastSeenVersion(version: string): void {
  try {
    localStorage.setItem(LAST_SEEN_KEY, version);
  } catch {
    // Ignore storage quota or disabled-storage errors.
  }
}

class WhatsNewStore {
  open = $state(false);
  manual = $state(false);
  releases = $state<ChangelogRelease[]>([]);

  checkOnStartup(currentVersion: string): void {
    const trimmed = currentVersion?.trim() ?? "";
    if (!trimmed || trimmed === "dev" || !isValidSemver(trimmed)) return;

    const lastSeen = readLastSeenVersion();
    const unseen = releasesSince(allReleases, lastSeen, trimmed);
    if (unseen.length === 0) return;

    this.releases = unseen;
    this.manual = false;
    this.open = true;
  }

  openManual(): void {
    this.releases = allReleases.slice(0, 15);
    this.manual = true;
    this.open = true;
  }

  dismiss(currentVersion: string): void {
    this.open = false;
    this.manual = false;
    const trimmed = currentVersion?.trim() ?? "";
    if (trimmed && trimmed !== "dev" && isValidSemver(trimmed)) {
      writeLastSeenVersion(trimmed);
    }
  }
}

export const whatsNew = new WhatsNewStore();
