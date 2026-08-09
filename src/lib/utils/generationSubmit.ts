import { generate, type GenerateResponse } from "./api.js";
import { progress } from "../stores/progress.svelte.js";
import type { GenerationParams } from "../types/index.js";

/**
 * Submit + queue-tracking helpers shared by the main Generate button and any
 * other caller that needs a generation to run through the normal progress
 * queue (currently the artist gallery's local preview action).
 *
 * This lives in utils rather than a store: it only orchestrates an existing
 * API wrapper and an existing store, and a store importing it would create a
 * cycle with `progress`.
 */

export async function requestGeneration(params: GenerationParams): Promise<GenerateResponse> {
  const result = await generate(params);
  // The backend resolves seed "-1" to a concrete value; write it back so a
  // caller reusing the same params object keeps the resolved seed.
  params.seed = result.seed;
  return result;
}

export function trackGeneration(params: GenerationParams, result: GenerateResponse): string {
  progress.enqueue(result.prompt_id, params.upscale_enabled, params.mode, params);
  if (result.queue_position != null && result.queue_total != null) {
    progress.updateQueuePosition(result.prompt_id, result.queue_position, result.queue_total);
  }
  return result.prompt_id;
}

export async function submitGeneration(params: GenerationParams): Promise<string> {
  return trackGeneration(params, await requestGeneration(params));
}
