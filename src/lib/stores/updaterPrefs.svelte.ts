import type { Update } from "@tauri-apps/plugin-updater";
import {
  loadAutoUpdateMode,
  parseAutoUpdateMode,
  saveAutoUpdateMode,
  type AutoUpdateMode,
} from "../utils/autoUpdate.js";

/**
 * Reactive desktop auto-update preferences and shared session download cache
 * so UpdateNotification and SettingsPage stay synchronized without
 * double-downloading the same release package.
 */
class UpdaterPrefsStore {
  autoUpdateMode = $state<AutoUpdateMode>(loadAutoUpdateMode());
  downloadedUpdate = $state<Update | null>(null);
  downloadedVersion = $state<string | null>(null);
  downloadingVersion = $state<string | null>(null);
  installedVersion = $state<string | null>(null);
  downloadProgress = $state(0);
  downloadTotal = $state(0);
  inFlightPromise: Promise<void> | null = null;

  setAutoUpdateMode(mode: AutoUpdateMode): void {
    const normalized = parseAutoUpdateMode(mode);
    this.autoUpdateMode = normalized;
    saveAutoUpdateMode(normalized);
  }

  startDownload(version: string, promise: Promise<void>): void {
    this.downloadingVersion = version;
    this.downloadProgress = 0;
    this.downloadTotal = 0;
    this.inFlightPromise = promise;
  }

  updateProgress(downloaded: number, total?: number): void {
    this.downloadProgress = downloaded;
    if (typeof total === "number") {
      this.downloadTotal = total;
    }
  }

  setDownloaded(update: Update): void {
    this.downloadingVersion = null;
    this.inFlightPromise = null;
    this.downloadedUpdate = update;
    this.downloadedVersion = update.version;
  }

  markInstalled(version: string): void {
    this.downloadingVersion = null;
    this.inFlightPromise = null;
    if (this.downloadedVersion === version) {
      this.downloadedUpdate = null;
      this.downloadedVersion = null;
    }
    this.installedVersion = version;
  }

  clearDownloaded(version?: string): void {
    if (!version || this.downloadedVersion === version) {
      this.downloadedUpdate = null;
      this.downloadedVersion = null;
    }
    if (!version || this.downloadingVersion === version) {
      this.downloadingVersion = null;
      this.inFlightPromise = null;
    }
  }

  getDownloadedFor(version: string): Update | null {
    return this.downloadedVersion === version ? this.downloadedUpdate : null;
  }
}

export const updaterPrefs = new UpdaterPrefsStore();
