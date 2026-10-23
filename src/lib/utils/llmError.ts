import { locale } from "../stores/locale.svelte.js";

/**
 * Turn a thrown prompt-assistant error into a toast the user can act on.
 *
 * The named cases are conditions the user can act on: a busy or missing model,
 * a request MooshieUI refused to send, and a hosted provider's own content
 * filter declining one, which reads as MooshieUI blocking it unless it says
 * otherwise. Everything else keeps the backend's own words: the llama-server
 * crash tail, the missing shared library, the health timeout. That detail is what makes a failed
 * enhance diagnosable, especially on headless server deployments where this
 * toast is the only signal anyone sees.
 */
export function mapLlmError(msg: string): string {
  if (msg.includes("busy_generation")) return locale.t("prompt_assistant.busy_generation");
  if (msg.includes("no_vision_model")) return locale.t("prompt_assistant.no_vision_model");
  if (msg.includes("provider_no_vision")) return locale.t("prompt_assistant.provider_no_vision");
  if (msg.includes("describe_expired")) return locale.t("prompt_assistant.describe_expired");
  if (msg.includes("no_model")) return locale.t("prompt_assistant.no_model");
  if (msg.includes("blocked_minor_content")) return locale.t("prompt_assistant.blocked_minor_content");
  if (isProviderRefusal(msg)) return locale.t("prompt_assistant.provider_refused");
  const detail = msg.replace(/^Error:\s*/, "").trim();
  return detail
    ? `${locale.t("prompt_assistant.error_generic")}: ${detail}`
    : locale.t("prompt_assistant.error_generic");
}

/**
 * A hosted provider declining a request on content grounds: xAI answers 403
 * with `permission-denied` and "I'm sorry, I can't help with that request."
 */
function isProviderRefusal(msg: string): boolean {
  return /\b403\b/.test(msg) && /permission-denied|can'?t help|cannot help|content polic/i.test(msg);
}
