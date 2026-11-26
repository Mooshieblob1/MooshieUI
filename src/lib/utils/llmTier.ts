/**
 * Prompt-assistant model tiering and JSON capability helpers.
 *
 * Keeps compact (4B-14B / budget APIs), standard (27B-35B MoE / fast cloud),
 * and full (frontier models) prompt strategies separate without forcing every
 * model through a 51 KB spec or giving small Ollama models frontier budgets.
 *
 * Leaf util with no store imports.
 */

export type LlmModelTierSetting = "auto" | "compact" | "standard" | "full";
export type EffectiveLlmTier = "compact" | "standard" | "full";

const VALID_SETTINGS = new Set<string>(["auto", "compact", "standard", "full"]);

/** Normalize user-configured tier string to a valid setting (`"auto"` fallback). */
export function normalizeLlmModelTierSetting(setting?: string | null): LlmModelTierSetting {
  const s = (setting ?? "").trim().toLowerCase();
  return VALID_SETTINGS.has(s) ? (s as LlmModelTierSetting) : "auto";
}

const COMPACT_LOCAL_IDS = new Set([
  "dantaggen-l",
  "qwen3-4b-instruct",
  "qwen25-7b-instruct",
  "qwen3-vl-4b-instruct",
  "qwen3-vl-8b-instruct",
]);

const STANDARD_LOCAL_IDS = new Set([
  "qwen3-30b-a3b-instruct",
  "qwen36-35b-a3b",
  "qwen38-27b",
]);

function isLoopbackUrl(url?: string | null): boolean {
  const u = (url ?? "").trim().toLowerCase();
  if (!u) return false;
  return (
    u.includes("://localhost") ||
    u.includes("://127.0.0.1") ||
    u.includes("://[::1]") ||
    u.includes("://0.0.0.0")
  );
}

/**
 * Extract largest parameter count mentioned in a model id (e.g. `qwen3:8b` -> 8,
 * `qwen3.6-35b-a3b` -> 35, `hermes-4-405b` -> 405). Returns `null` when absent.
 */
export function parseModelBillions(model?: string | null): number | null {
  const m = (model ?? "").toLowerCase();
  if (!m) return null;
  let best: number | null = null;
  for (const match of m.matchAll(/(?:^|[^a-z0-9])(\d+(?:\.\d+)?)\s*b(?:[^a-z0-9]|$)/g)) {
    const n = Number.parseFloat(match[1]);
    if (Number.isFinite(n) && n > 0 && (best === null || n > best)) {
      best = n;
    }
  }
  return best;
}

function tierForBillions(b: number): EffectiveLlmTier {
  if (b <= 14) return "compact";
  if (b < 70) return "standard";
  return "full";
}

/**
 * Resolve the effective tier (`compact` | `standard` | `full`) for the active
 * prompt assistant model.
 */
export function resolveLlmModelTier(opts: {
  setting?: string | null;
  usesExternal: boolean;
  provider?: string | null;
  baseUrl?: string | null;
  model?: string | null;
  localModelId?: string | null;
}): EffectiveLlmTier {
  const explicit = normalizeLlmModelTierSetting(opts.setting);
  if (explicit !== "auto") return explicit;

  if (!opts.usesExternal) {
    const id = (opts.localModelId ?? "").trim().toLowerCase();
    if (STANDARD_LOCAL_IDS.has(id)) return "standard";
    if (COMPACT_LOCAL_IDS.has(id)) return "compact";
    const localB = parseModelBillions(id);
    return localB !== null ? tierForBillions(localB) : "compact";
  }

  const provider = (opts.provider ?? "").trim().toLowerCase();
  const model = (opts.model ?? "").trim().toLowerCase();

  // Lightweight / mini cloud models benefit from compact/standard rule slicing.
  if (
    /\b(?:haiku|flash-lite|nano)\b/.test(model) ||
    model.includes("-mini") && !/\bo[134]-mini\b/.test(model)
  ) {
    return "standard";
  }

  // Known parameter size in model id (e.g. Ollama `qwen3:8b`, OpenRouter `:70b`, `:32b`).
  const paramsB = parseModelBillions(model);
  if (paramsB !== null) {
    return tierForBillions(paramsB);
  }

  // Loopback custom endpoint (Ollama / LM Studio / llama.cpp) with an unnumbered tag defaults conservative.
  if (provider === "custom" && isLoopbackUrl(opts.baseUrl)) {
    return "standard";
  }

  return "full";
}

/**
 * Whether this external provider accepts OpenAI `response_format: { type: "json_object" }`
 * (paired with a Rust 400/422 fallback if an individual server rejects it).
 * Companions and Anthropic omit `response_format` and rely on prompt contract + parser.
 */
export function supportsExternalJsonMode(provider?: string | null): boolean {
  const p = (provider ?? "").trim().toLowerCase();
  if (!p) return true;
  return p !== "anthropic" && p !== "chatgpt" && p !== "gemini-cli";
}
