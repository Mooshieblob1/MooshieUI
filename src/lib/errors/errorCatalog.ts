export interface CatalogEntry {
  id: string;
  match: (raw: string) => boolean;
}

const has = (needle: string) => (raw: string) => raw.toLowerCase().includes(needle);
const re = (pattern: RegExp) => (raw: string) => pattern.test(raw);

/** Ordered: specific matchers first, broad ones last. First match wins. */
export const CATALOG: CatalogEntry[] = [
  // Connectivity
  { id: "comfyui_not_running", match: re(/comfyui.*(not running|not started|unavailable)|failed to connect to comfyui/i) },
  { id: "connection_failed", match: re(/connection failed|connection refused|failed to connect|could not connect/i) },
  { id: "websocket_dropped", match: re(/websocket error|websocket.*(closed|dropped|disconnect)/i) },
  { id: "api_error_5xx", match: re(/api error \(5\d\d\)|http error.*5\d\d/i) },

  // Downloads / models
  { id: "download_404", match: re(/\b404\b|not found/i) },
  { id: "disk_full", match: re(/no space left|not enough space|disk full|insufficient disk/i) },
  { id: "checksum_mismatch", match: re(/checksum|sha256.*mismatch|hash mismatch/i) },
  { id: "civitai_auth", match: re(/civitai.*(401|403|unauthor|api key|token)/i) },
  { id: "hf_page_url", match: re(/huggingface.*(page url|\/blob\/)|not a direct file/i) },
  { id: "model_not_found", match: re(/model.*(not found|missing)|no such model|checkpoint.*not found/i) },
  { id: "download_network", match: re(/http error|network error|timed out|timeout|reqwest/i) },

  // Setup / runtime
  { id: "comfyui_launch_failed", match: re(/failed to start comfyui|process.*spawn|failed to spawn/i) },
  { id: "python_env_broken", match: re(/python.*(not found|missing)|venv|virtualenv|no module named/i) },
  { id: "attention_backend_install", match: re(/attention backend|flash.?attn|sage.?attn|xformers/i) },
  { id: "out_of_memory", match: re(/out of memory|cuda.*memory|oom|allocat.*fail/i) },
  { id: "unsupported_gpu", match: re(/unsupported gpu|no cuda|no gpu|device.*not supported/i) },

  // Generation
  { id: "missing_node", match: re(/missing node|node type.*not found|unknown node|no node named/i) },
  { id: "invalid_workflow", match: re(/invalid workflow|malformed workflow|workflow.*invalid/i) },
  { id: "generation_interrupted", match: re(/interrupted|cancell?ed|aborted/i) },

  // IO / misc
  { id: "io_permission", match: re(/permission denied|access is denied|io error/i) },
  { id: "serialization", match: has("serialization error") },
];

export function catalogIds(): string[] {
  return CATALOG.map((e) => e.id);
}
