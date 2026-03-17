import { invoke } from "@tauri-apps/api/core";
import type {
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

export async function saveImageFile(
  imageBytes: number[],
  path: string
): Promise<void> {
  return invoke("save_image_file", { imageBytes, path });
}

export async function saveToGallery(
  filename: string,
  subfolder: string,
  promptId: string
): Promise<string> {
  return invoke("save_to_gallery", { filename, subfolder, promptId });
}

export async function listGalleryImages(): Promise<string[]> {
  return invoke("list_gallery_images");
}

export async function loadGalleryImage(filename: string): Promise<number[]> {
  return invoke("load_gallery_image", { filename });
}

export async function deleteGalleryImage(filename: string): Promise<void> {
  return invoke("delete_gallery_image", { filename });
}

export async function copyImageToClipboard(imageBytes: number[]): Promise<void> {
  return invoke("copy_image_to_clipboard", { imageBytes });
}
