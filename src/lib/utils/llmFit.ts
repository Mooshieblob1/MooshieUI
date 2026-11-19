/**
 * Whether this machine can run a bundled prompt assistant model variant, and
 * how. Mirrors the backend's offload choice (`pick_offload` in
 * src-tauri/src/prompt_assistant/mod.rs) closely enough to grey out what
 * will not run.
 *
 * Leaf util with no imports, so it runs under plain Node for checking.
 */

export interface FitVariant {
  size_mb: number;
  vram_mb: number;
  cpu_moe_vram_mb?: number | null;
}

export interface FitHardware {
  total_vram_mb: number;
  system_ram_mb: number;
}

/** "gpu": all on the GPU. "experts_in_ram": a mixture of experts with its experts in RAM. "cpu": no usable GPU, runs from RAM. */
export type LlmFit = "gpu" | "experts_in_ram" | "cpu";

/** Below this much VRAM the host counts as having no usable GPU. */
const MIN_GPU_MB = 2000;
/** The share of system RAM a model may take. */
const RAM_SHARE = 0.7;
/** VRAM kept spare over `cpu_moe_vram_mb` for the 16k context's KV cache. */
const MOE_CONTEXT_MB = 1500;

/** How the variant would run here, or null when it would not run usably. */
export function llmFit(v: FitVariant, hw: FitHardware | null | undefined): LlmFit | null {
  const vram = hw?.total_vram_mb ?? 0;
  const ram = (hw?.system_ram_mb ?? 0) * RAM_SHARE;
  if (vram < MIN_GPU_MB) return ram >= v.vram_mb ? "cpu" : null;
  if (v.vram_mb <= vram) return "gpu";
  if (v.cpu_moe_vram_mb != null && vram >= v.cpu_moe_vram_mb + MOE_CONTEXT_MB && ram >= v.size_mb) {
    return "experts_in_ram";
  }
  return null;
}
