export type AutoUpdateMode = "off" | "download" | "auto";

export const AUTO_UPDATE_STORAGE_KEY = "mooshieui.updater.autoUpdate.v1";

/** Normalize an untrusted or persisted auto-update preference value. */
export function parseAutoUpdateMode(raw: unknown): AutoUpdateMode {
  if (raw === "download" || raw === "auto") return raw;
  return "off";
}

/** Read the persisted auto-update preference, defaulting to "off". */
export function loadAutoUpdateMode(storage?: Pick<Storage, "getItem"> | null): AutoUpdateMode {
  try {
    const store = storage ?? globalThis.localStorage;
    return parseAutoUpdateMode(store?.getItem(AUTO_UPDATE_STORAGE_KEY));
  } catch {
    return "off";
  }
}

/** Persist the validated auto-update preference to localStorage. */
export function saveAutoUpdateMode(
  mode: AutoUpdateMode,
  storage?: Pick<Storage, "setItem"> | null,
): void {
  try {
    const store = storage ?? globalThis.localStorage;
    store?.setItem(AUTO_UPDATE_STORAGE_KEY, parseAutoUpdateMode(mode));
  } catch {
    // Ignore storage write failures (e.g. restricted webview contexts).
  }
}

/** True when the desktop updater should download an update in the background. */
export function shouldAutoDownload(mode: AutoUpdateMode, managedCommand: string): boolean {
  if (managedCommand) return false;
  return mode === "download" || mode === "auto";
}

/** True when generation queues are empty and nothing is actively running. */
export function isGenerationIdle(pendingCount: number, isGenerating: boolean): boolean {
  return pendingCount === 0 && !isGenerating;
}

/** True when the updater should start the auto-install countdown right now. */
export function shouldAutoInstallWhenIdle(
  mode: AutoUpdateMode,
  managedCommand: string,
  isIdle: boolean,
  cancelledOrDismissed: boolean,
): boolean {
  if (managedCommand) return false;
  return mode === "auto" && isIdle && !cancelledOrDismissed;
}
