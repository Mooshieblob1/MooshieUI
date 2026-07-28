import { isStyleTransferExecutionError } from "./styleTransferNodes.js";

/**
 * Result of classifying a ComfyUI generation/execution failure into a
 * user-actionable message. Callers translate `messageKey` with `locale.t`.
 */
export interface ClassifiedGenerationError {
  /** i18n key for the toast/inline message. */
  messageKey: string;
  /** Interpolation params for the i18n key. */
  params?: Record<string, string | number>;
  /** Suggested display duration (ms). Longer for detailed, actionable text. */
  durationMs?: number;
  /** True when the failure is style-transfer related and that state should be auto-cleared. */
  clearStyleTransfer?: boolean;
}

/** Longer dwell time so users can actually read the actionable guidance. */
const ACTIONABLE_MS = 8000;

/**
 * Accepts either a raw ComfyUI WebSocket `execution_error` payload (desktop:
 * { exception_message, exception_type, node_type, traceback, ... }), a
 * server-synthesized payload (browser: { error }), or a raw error string
 * (HTTP /prompt submission failures surfaced as AppError strings).
 */
type ErrorInput = string | Record<string, unknown> | null | undefined;

interface Extracted {
  /** Combined, lowercased searchable text from all available fields. */
  haystack: string;
  /** Original-case combined text (for value extraction). */
  raw: string;
  nodeType: string;
}

function asText(v: unknown): string {
  if (v == null) return "";
  if (typeof v === "string") return v;
  if (Array.isArray(v)) return v.map(asText).join("\n");
  if (typeof v === "object") {
    try {
      return JSON.stringify(v);
    } catch {
      return "";
    }
  }
  return String(v);
}

function extract(input: ErrorInput): Extracted {
  if (input == null) return { haystack: "", raw: "", nodeType: "" };
  if (typeof input === "string") {
    return { haystack: input.toLowerCase(), raw: input, nodeType: "" };
  }
  const parts = [
    asText(input.error),
    asText(input.exception_message),
    asText(input.exception_type),
    asText(input.node_type),
    asText(input.traceback),
    asText(input.details),
    asText(input.message),
  ].filter(Boolean);
  const raw = parts.join("\n");
  return {
    haystack: raw.toLowerCase(),
    raw,
    nodeType: asText(input.node_type),
  };
}

/**
 * Pulls the offending value out of a ComfyUI "value not in list" validation
 * error, e.g. `ckpt_name: 'sih-v1.5.safetensors' not in [...]` -> the filename.
 */
function extractMissingModel(raw: string): string | undefined {
  const m = raw.match(/(\w+):\s*'([^']+)'\s+not in/i);
  if (m) return m[2];
  // Also handle the node_errors JSON `details` form without quotes.
  const m2 = raw.match(/(?:ckpt_name|vae_name|lora_name|control_net_name|unet_name|clip_name|model_name|upscale_model)[^A-Za-z0-9]+([^'"\r\n,;]+?\.(?:safetensors|ckpt|pt|pth|bin|gguf))/i);
  if (m2) return m2[1].trim();
  return undefined;
}

/**
 * Quantization formats ComfyUI has shipped in `QUANT_ALGOS`. Used to recognise a
 * format name in a bare `KeyError` when no traceback is attached, without
 * swallowing unrelated `KeyError`s.
 */
const KNOWN_QUANT_FORMATS = new Set([
  "float8_e4m3fn",
  "float8_e5m2",
  "nvfp4",
  "mxfp8",
  "int8_tensorwise",
  "convrot_w4a4",
]);

/** Shape of a quant-format name, so formats added upstream still match. */
const QUANT_FORMAT_SHAPE = /^(?:int|uint|fp|float|nv|mx|convrot)[\w.]*$/i;

/**
 * Detects a checkpoint whose quantization format the installed ComfyUI is too
 * old to understand, and returns that format name.
 *
 * ComfyUI dereferences `QUANT_ALGOS[module.quant_format]` in `comfy/ops.py`
 * before the dispatch chain that would raise a readable
 * `ValueError: Unsupported quantization format: X`, so an unknown format
 * surfaces as a bare `KeyError` whose entire message is the format name -- e.g.
 * an INT8 checkpoint on ComfyUI older than v0.27.0 reports just
 * `'int8_tensorwise'`. Both forms mean the same thing to a user, so match the
 * explicit one first and fall back to the KeyError.
 */
function extractUnsupportedQuantFormat(raw: string, haystack: string): string | undefined {
  const explicit = raw.match(/unsupported quantization format:\s*'?([\w.\-]+)'?/i);
  if (explicit) return explicit[1];

  const key = raw.match(/KeyError:\s*'([^']+)'/) || raw.match(/(?:^|\n)\s*'([^'\s]+)'\s*(?:\n|$)/);
  if (!key) return undefined;
  const candidate = key[1];

  // A traceback through the quant tables is proof on its own. Without one
  // (browser mode can relay a thinner payload) require the key to actually look
  // like a quantization format, so a stray KeyError is not misreported.
  const fromQuantTable = haystack.includes("quant_algos") || haystack.includes("quant_format");
  if (fromQuantTable || KNOWN_QUANT_FORMATS.has(candidate.toLowerCase())) return candidate;
  return QUANT_FORMAT_SHAPE.test(candidate) && candidate.length >= 4 ? candidate : undefined;
}

/** Pulls a missing/unknown node class name out of the error text. */
function extractMissingNode(raw: string): string | undefined {
  const m =
    raw.match(/node type[: ]+'?([\w.\-]+)'?\s+(?:does not exist|not found|is not)/i) ||
    raw.match(/'?([\w.\-]+)'?\s+is not (?:installed|registered|a recognized node)/i) ||
    raw.match(/cannot find (?:the )?node[: ]+'?([\w.\-]+)'?/i);
  return m ? m[1] : undefined;
}

/**
 * Classify a generation failure into the most specific, actionable message we
 * can. Order matters: most specific signatures first, generic fallbacks last.
 */
export function classifyGenerationError(input: ErrorInput): ClassifiedGenerationError {
  const { haystack, raw, nodeType } = extract(input);
  const node = nodeType.toLowerCase();

  // 1. Style transfer (missing/broken Untwisting RoPE nodes).
  if (isStyleTransferExecutionError(raw)) {
    return {
      messageKey: "generation.style_transfer.execution_failed",
      clearStyleTransfer: true,
      durationMs: ACTIONABLE_MS,
    };
  }

  // 2. Out of GPU memory. Highest-value actionable error.
  if (
    haystack.includes("out of memory") ||
    haystack.includes("outofmemoryerror") ||
    haystack.includes("cuda error: out of memory") ||
    (haystack.includes("alloc") && haystack.includes("memory") && haystack.includes("cuda"))
  ) {
    return { messageKey: "generation.error.out_of_memory", durationMs: ACTIONABLE_MS };
  }

  // 3. VAE incompatible with the checkpoint's latent format. The signature is a
  // VAEDecode/VAEEncode node raising an IndexError inside the memory-estimation
  // lambda (e.g. a 3D/video VAE like qwen_image_vae paired with an SD/SDXL
  // checkpoint, where shape[4] does not exist).
  const isVaeNode = node.includes("vae") || haystack.includes("vaedecode") || haystack.includes("vaeencode");
  if (
    (isVaeNode &&
      (haystack.includes("tuple index out of range") ||
        haystack.includes("memory_used_decode") ||
        haystack.includes("memory_used_encode"))) ||
    haystack.includes("memory_used_decode") ||
    haystack.includes("memory_used_encode")
  ) {
    return { messageKey: "generation.error.vae_incompatible", durationMs: ACTIONABLE_MS };
  }

  // 4. A selected model file is no longer available to ComfyUI (renamed, moved,
  // or the validation cache is stale).
  if (
    haystack.includes("value_not_in_list") ||
    haystack.includes("value not in list") ||
    haystack.includes("prompt_outputs_failed_validation") ||
    haystack.includes("not in list")
  ) {
    const model = extractMissingModel(raw);
    if (model) {
      return {
        messageKey: "generation.error.model_not_found",
        params: { model },
        durationMs: ACTIONABLE_MS,
      };
    }
    return { messageKey: "generation.error.model_not_found_generic", durationMs: ACTIONABLE_MS };
  }

  // 5. A required custom node isn't installed.
  if (
    haystack.includes("does not exist") ||
    haystack.includes("is not installed") ||
    haystack.includes("is not a recognized node") ||
    (haystack.includes("node type") && haystack.includes("not found"))
  ) {
    const nodeName = extractMissingNode(raw);
    if (nodeName) {
      return {
        messageKey: "generation.error.missing_node",
        params: { node: nodeName },
        durationMs: ACTIONABLE_MS,
      };
    }
  }

  // 6. Tensor shape mismatch: usually mixing model components from different
  // families (e.g. an SDXL VAE/LoRA with an SD1.5 checkpoint).
  if (
    haystack.includes("shapes cannot be multiplied") ||
    haystack.includes("size mismatch") ||
    (haystack.includes("expected") && haystack.includes("channels")) ||
    haystack.includes("dimension out of range") ||
    haystack.includes("must match the size")
  ) {
    return { messageKey: "generation.error.component_mismatch", durationMs: ACTIONABLE_MS };
  }

  // 7. ComfyUI HTTP /prompt returned a structured error body. Surface its
  // message if we can parse it.
  const apiMatch = raw.match(/API error \(\d+\): ([\s\S]+)/);
  if (apiMatch) {
    try {
      const parsed = JSON.parse(apiMatch[1]);
      // Prefer the most specific detail available.
      const nodeErrors = parsed?.node_errors;
      if (nodeErrors && typeof nodeErrors === "object") {
        for (const key of Object.keys(nodeErrors)) {
          const errs = nodeErrors[key]?.errors;
          if (Array.isArray(errs) && errs.length > 0) {
            const detail = errs[0]?.details || errs[0]?.message;
            if (detail) {
              const model = extractMissingModel(String(detail));
              if (model) {
                return {
                  messageKey: "generation.error.model_not_found",
                  params: { model },
                  durationMs: ACTIONABLE_MS,
                };
              }
              return {
                messageKey: "generation.toast.failed_detail",
                params: { message: String(detail) },
                durationMs: ACTIONABLE_MS,
              };
            }
          }
        }
      }
      if (parsed?.error?.message) {
        return {
          messageKey: "generation.toast.failed_detail",
          params: { message: String(parsed.error.message) },
          durationMs: ACTIONABLE_MS,
        };
      }
    } catch {
      /* fall through to generic */
    }
  }

  // 8. The checkpoint is quantized in a format the pinned ComfyUI predates
  // (e.g. int8_tensorwise landed in ComfyUI v0.27.0). Left unclassified this
  // reaches the user as a bare Python KeyError like `'int8_tensorwise'`, which
  // says nothing about what to do. Updating ComfyUI is the fix.
  const quantFormat = extractUnsupportedQuantFormat(raw, haystack);
  if (quantFormat) {
    return {
      messageKey: "generation.error.quant_format_unsupported",
      params: { format: quantFormat },
      durationMs: ACTIONABLE_MS,
    };
  }

  // 9. Generic fallback. If we have any exception message, show it rather than
  // a bare "Generation failed".
  if (typeof input === "object" && input) {
    const detail = asText(input.exception_message) || asText(input.error);
    const trimmed = detail.trim();
    if (trimmed && !trimmed.startsWith("{") && trimmed.length <= 200) {
      return {
        messageKey: "generation.toast.failed_detail",
        params: { message: trimmed },
        durationMs: ACTIONABLE_MS,
      };
    }
  }

  return { messageKey: "generation.toast.failed" };
}
