/**
 * Device detection helpers.
 *
 * Mobile UI is gated by:
 *   1. running in browser mode (`isBrowserMode`), AND
 *   2. a phone/tablet User-Agent, AND
 *   3. user has not toggled the desktop-layout override.
 */

import { isBrowserMode } from "./ipc.js";

const FORCE_DESKTOP_KEY = "mooshie:forceDesktopLayout";

/** Heuristic UA test. Matches phones + tablets (incl. iPadOS faking desktop Safari). */
export function isMobileUA(): boolean {
  if (typeof navigator === "undefined") return false;
  const ua = navigator.userAgent || "";
  const phoneRe =
    /Android.*Mobile|iPhone|iPod|webOS|BlackBerry|IEMobile|Opera Mini|Mobile Safari/i;
  const tabletRe = /iPad|Android(?!.*Mobile)|Tablet|PlayBook|Silk/i;
  if (phoneRe.test(ua) || tabletRe.test(ua)) return true;
  // iPadOS 13+ reports desktop Safari UA; detect via touch + macOS.
  if (
    /Macintosh/.test(ua) &&
    typeof navigator.maxTouchPoints === "number" &&
    navigator.maxTouchPoints > 1
  ) {
    return true;
  }
  return false;
}

export function getForceDesktopOverride(): boolean {
  try {
    return localStorage.getItem(FORCE_DESKTOP_KEY) === "1";
  } catch {
    return false;
  }
}

export function setForceDesktopOverride(value: boolean): void {
  try {
    if (value) localStorage.setItem(FORCE_DESKTOP_KEY, "1");
    else localStorage.removeItem(FORCE_DESKTOP_KEY);
  } catch {
    /* ignore quota / privacy mode errors */
  }
}

/** Resolved at module load — mobile shell only renders when true. */
export const useMobileLayout: boolean =
  isBrowserMode && isMobileUA() && !getForceDesktopOverride();
