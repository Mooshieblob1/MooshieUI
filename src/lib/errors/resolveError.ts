import { locale } from "../stores/locale.svelte.js";
import type { FriendlyError } from "./types.js";
import { CATALOG } from "./errorCatalog.js";

/** Coerce any thrown value to a display string. */
function toRawString(raw: unknown): string {
  if (raw == null) return "";
  if (typeof raw === "string") return raw;
  if (raw instanceof Error) return raw.message;
  if (typeof raw === "object" && "message" in raw && typeof (raw as { message: unknown }).message === "string") {
    return (raw as { message: string }).message;
  }
  return String(raw);
}

/** Read errors.<id>.fixes as an array; tolerate a single string. */
function fixesFor(id: string): string[] {
  const key = `errors.${id}.fixes`;
  const joined = locale.t(key);
  // Fixes are authored as a single string with " || " separators to keep i18n flat.
  if (!joined || joined === key) return [];
  return joined.split(" || ").map((s) => s.trim()).filter(Boolean);
}

function buildFriendly(id: string, raw: string, reportable: boolean): FriendlyError {
  return {
    code: id,
    title: locale.t(`errors.${id}.title`),
    what: locale.t(`errors.${id}.what`),
    why: locale.t(`errors.${id}.why`),
    fixes: fixesFor(id),
    reportable,
    raw,
  };
}

/** Resolve any error into user-facing guidance. Never throws. */
export function resolveError(raw: unknown): FriendlyError {
  const text = toRawString(raw);
  for (const entry of CATALOG) {
    try {
      if (entry.match(text)) return buildFriendly(entry.id, text, true);
    } catch {
      // A broken matcher must never break resolution.
    }
  }
  // Generic fallback. `what` shows the raw text so nothing is hidden.
  return {
    code: "unknown",
    title: locale.t("errors.generic.title"),
    what: text || locale.t("errors.generic.what"),
    why: locale.t("errors.generic.why"),
    fixes: fixesFor("generic"),
    reportable: true,
    raw: text,
  };
}
