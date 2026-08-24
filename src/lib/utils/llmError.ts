import { locale } from "../stores/locale.svelte.js";

/**
 * Turn a thrown prompt-assistant error into a toast the user can act on.
 *
 * The two named cases are conditions the user can fix themselves. Everything
 * else keeps the backend's own words: the llama-server crash tail, the missing
 * shared library, the health timeout. That detail is what makes a failed
 * enhance diagnosable, especially on headless server deployments where this
 * toast is the only signal anyone sees.
 */
export function mapLlmError(msg: string): string {
  if (msg.includes("busy_generation")) return locale.t("prompt_assistant.busy_generation");
  if (msg.includes("no_model")) return locale.t("prompt_assistant.no_model");
  const detail = msg.replace(/^Error:\s*/, "").trim();
  return detail
    ? `${locale.t("prompt_assistant.error_generic")}: ${detail}`
    : locale.t("prompt_assistant.error_generic");
}
