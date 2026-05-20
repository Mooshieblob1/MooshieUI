/** Payload from backend `comfyui:server_error` or structured startup failures. */
export interface ComfyServerErrorPayload {
  error?: string;
  kind?:
    | "missing_mooshie_nodes"
    | "missing_controlnet_nodes"
    | "crashed"
    | "generic"
    | string;
  missing_nodes?: string[];
  log_excerpt?: string | null;
  port?: number;
  crashed?: boolean;
}

const MISSING_MOOSHIE_MARKER = "has not loaded required MooshieUI custom nodes";
const MISSING_CONTROLNET_MARKER = "Required ControlNet custom nodes failed to load";

export function isMissingMooshieNodesMessage(message: string): boolean {
  return message.includes(MISSING_MOOSHIE_MARKER);
}

export function isMissingControlNetNodesMessage(message: string): boolean {
  return message.includes(MISSING_CONTROLNET_MARKER);
}

export function isNodeLoadFailurePayload(payload: ComfyServerErrorPayload): boolean {
  return (
    payload.kind === "missing_mooshie_nodes" ||
    payload.kind === "missing_controlnet_nodes" ||
    (payload.missing_nodes?.length ?? 0) > 0 ||
    isMissingMooshieNodesMessage(payload.error ?? "") ||
    isMissingControlNetNodesMessage(payload.error ?? "")
  );
}

export function parseComfyServerError(
  raw: unknown,
  fallbackMessage = "",
): ComfyServerErrorPayload {
  if (raw && typeof raw === "object" && !Array.isArray(raw)) {
    const o = raw as Record<string, unknown>;
    const error =
      typeof o.error === "string"
        ? o.error
        : fallbackMessage || String(raw);
    return {
      error,
      kind: typeof o.kind === "string" ? o.kind : undefined,
      missing_nodes: Array.isArray(o.missing_nodes)
        ? o.missing_nodes.filter((n): n is string => typeof n === "string")
        : isMissingMooshieNodesMessage(error)
          ? parseMissingNodesFromMessage(error)
          : [],
      log_excerpt:
        typeof o.log_excerpt === "string" ? o.log_excerpt : null,
      port: typeof o.port === "number" ? o.port : undefined,
      crashed: o.crashed === true,
    };
  }
  const message =
    typeof raw === "string" ? raw : fallbackMessage || String(raw ?? "");
  return {
    error: message,
    kind: isMissingMooshieNodesMessage(message)
      ? "missing_mooshie_nodes"
      : isMissingControlNetNodesMessage(message)
        ? "missing_controlnet_nodes"
        : undefined,
    missing_nodes: parseMissingNodesFromMessage(message),
    log_excerpt: null,
  };
}

export function parseMissingNodesFromMessage(message: string): string[] {
  const idx = message.indexOf(MISSING_MOOSHIE_MARKER);
  if (idx < 0) return [];
  const after = message.slice(idx + MISSING_MOOSHIE_MARKER.length);
  const colon = after.indexOf(":");
  if (colon < 0) return [];
  const rest = after.slice(colon + 1).trimStart();
  const dot = rest.indexOf(".");
  const list = dot >= 0 ? rest.slice(0, dot) : rest;
  return list
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}
