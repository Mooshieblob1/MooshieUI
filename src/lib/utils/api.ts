import { invoke } from "@tauri-apps/api/core";
import type {
  AppConfig,
  GalleryImageEntry,
  GenerationParams,
  OutputImage,
  QueueInfo,
  SamplerInfo,
  SystemStats,
} from "../types/index.js";

export async function getModels(category: string): Promise<string[]> {
  return invoke("get_models", { category });
}

export async function getSamplers(): Promise<SamplerInfo> {
  return invoke("get_samplers");
}

export async function getEmbeddings(): Promise<string[]> {
  return invoke("get_embeddings");
}

export async function generate(params: GenerationParams): Promise<string> {
  return invoke("generate", { params });
}

export async function getHistory(promptId: string): Promise<Record<string, unknown>> {
  return invoke("get_history", { promptId });
}

export async function getQueue(): Promise<QueueInfo> {
  return invoke("get_queue");
}

export async function interruptGeneration(): Promise<void> {
  return invoke("interrupt_generation");
}

export async function deleteQueueItem(promptId: string): Promise<void> {
  return invoke("delete_queue_item", { promptId });
}

export async function uploadImage(imagePath: string): Promise<{
  name: string;
  subfolder: string;
  type: string;
}> {
  return invoke("upload_image", { imagePath });
}

export async function uploadImageBytes(
  imageBytes: number[],
  filename: string
): Promise<{ name: string; subfolder: string; type: string }> {
  return invoke("upload_image_bytes", { imageBytes, filename });
}

export async function getOutputImage(
  filename: string,
  subfolder: string
): Promise<number[]> {
  return invoke("get_output_image", { filename, subfolder });
}

export async function getClientId(): Promise<string> {
  return invoke("get_client_id");
}

export async function startComfyui(): Promise<void> {
  return invoke("start_comfyui");
}

export async function stopComfyui(): Promise<void> {
  return invoke("stop_comfyui");
}

export async function checkServerHealth(): Promise<SystemStats> {
  return invoke("check_server_health");
}

export async function connectWs(): Promise<void> {
  return invoke("connect_ws");
}

export async function disconnectWs(): Promise<void> {
  return invoke("disconnect_ws");
}

export async function downloadModel(
  url: string,
  category: string,
  filename: string
): Promise<void> {
  return invoke("download_model", { url, category, filename });
}

export async function findModelByHash(
  category: string,
  hash: string
): Promise<string | null> {
  return invoke("find_model_by_hash", { category, hash });
}

export async function hashModelFile(
  category: string,
  filename: string
): Promise<{ sha256: string; autov2: string }> {
  return invoke("hash_model_file", { category, filename });
}

export async function civitaiLookupHash(
  hash: string
): Promise<Record<string, unknown>> {
  return invoke("civitai_lookup_hash", { hash });
}

export async function saveImageFile(
  imageBytes: number[],
  path: string
): Promise<void> {
  return invoke("save_image_file", { imageBytes, path });
}

export async function saveToGallery(
  filename: string,
  subfolder: string,
  promptId: string,
  mode?: "txt2img" | "img2img" | "inpainting",
  metadata?: Record<string, string>
): Promise<string> {
  return invoke("save_to_gallery", { filename, subfolder, promptId, mode, metadata });
}

export async function readImageMetadata(
  filename: string
): Promise<Record<string, string> | null> {
  return invoke("read_image_metadata", { filename });
}

export async function readImageMetadataBytes(
  imageBytes: number[]
): Promise<Record<string, string> | null> {
  return invoke("read_image_metadata_bytes", { imageBytes });
}

export async function listGalleryImages(): Promise<string[]> {
  return invoke("list_gallery_images");
}

export async function listGalleryImageEntries(): Promise<GalleryImageEntry[]> {
  return invoke("list_gallery_image_entries");
}

export async function loadGalleryImage(filename: string): Promise<number[]> {
  return invoke("load_gallery_image", { filename });
}

export async function deleteGalleryImage(filename: string): Promise<void> {
  return invoke("delete_gallery_image", { filename });
}

export async function renameGalleryImage(oldFilename: string, newFilename: string): Promise<string> {
  return invoke("rename_gallery_image", { oldFilename, newFilename });
}

export async function copyImageToClipboard(filePath: string): Promise<void> {
  return invoke("copy_image_to_clipboard", { filePath });
}

export async function getGalleryImagePath(filename: string): Promise<string> {
  return invoke("get_gallery_image_path", { filename });
}

export async function getConfig(): Promise<AppConfig> {
  return invoke("get_config");
}

export async function updateConfig(config: AppConfig): Promise<void> {
  return invoke("update_config", { config });
}
