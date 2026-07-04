import { isTauri } from "./ipc.js";

declare const __APP_VERSION__: string;

export function appVersion(): string {
  return typeof __APP_VERSION__ !== "undefined" ? __APP_VERSION__ : "dev";
}

export function appMode(): "desktop" | "browser" {
  return isTauri ? "desktop" : "browser";
}

/** Best-effort OS/arch from the browser environment. No new Tauri plugin needed. */
export function platformInfo(): { os: string; arch: string } {
  const nav = globalThis.navigator;
  const os = nav?.platform || nav?.userAgent || "unknown";
  const uaArch = (nav as unknown as { userAgentData?: { platform?: string } })?.userAgentData?.platform;
  const arch = uaArch || (/(x86_64|x64|amd64|arm64|aarch64)/i.exec(nav?.userAgent ?? "")?.[0]) || "unknown";
  return { os, arch };
}
