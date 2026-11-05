import { generate, novelaiGenerate, type GenerateResponse } from "./api.js";
import { isNovelAiModel } from "./novelaiModels.js";
import { locale } from "../stores/locale.svelte.js";
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
  // The checkpoint is the backend switch. NovelAI reports through the same
  // events and prompt-id contract, so everything downstream is unchanged.
  // Video is the exception: NovelAI has no video endpoint, and video loads its
  // own models from the `video_*` fields, so an image checkpoint from NovelAI
  // left selected must not send it there.
  const toNovelAi = params.mode !== "video" && isNovelAiModel(params.checkpoint);
  // NovelAI has no instruction-edit model either. A plain Image Edit run would
  // reach it as a fresh text-to-image request and spend Anlas on an image that
  // ignores the reference, so stop it here and say which models it needs. The
  // Enhance passes also tag themselves image_edit, but they set their own
  // NovelAI action and so get through.
  if (toNovelAi && params.mode === "image_edit" && params.novelai?.action === "generate") {
    throw new Error(locale.t("generation.image_edit.wrong_model"));
  }
  const result = toNovelAi ? await novelaiGenerate(params) : await generate(params);
  // The backend resolves seed "-1" to a concrete value; write it back so a
  // caller reusing the same params object keeps the resolved seed.
  params.seed = result.seed;
  return result;
}

export function trackGeneration(params: GenerationParams, result: GenerateResponse): string {
  progress.enqueue(result.prompt_id, params.upscale_enabled, params.mode, params, result.worker_id);
  if (result.queue_position != null && result.queue_total != null) {
    progress.updateQueuePosition(result.prompt_id, result.queue_position, result.queue_total);
  }
  return result.prompt_id;
}

export async function submitGeneration(params: GenerationParams): Promise<string> {
  return trackGeneration(params, await requestGeneration(params));
}
