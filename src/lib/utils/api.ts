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

export interface GenerateResponse {
  prompt_id: string;
  seed: number;
}

export async function generate(params: GenerationParams): Promise<GenerateResponse> {
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

export type CivitaiModelType =
  | "Checkpoint"
  | "LORA"
  | "Controlnet"
  | "Upscaler"
  | "VAE"
  | "TextualInversion";

export type CivitaiSort = "Highest Rated" | "Most Downloaded" | "Newest";

export type CivitaiPeriod = "AllTime" | "Month" | "Week" | "Day";

export type CivitaiFileFormat =
  | "SafeTensor"
  | "PickleTensor"
  | "GGUF"
  | "Diffusers"
  | "Core ML"
  | "ONNX"
  | "Other";

export type CivitaiModelStatus =
  | "Published"
  | "Draft"
  | "Training"
  | "Scheduled"
  | "Unpublished"
  | "UnpublishedViolation"
  | "GatherInterest"
  | "Deleted";

export interface CivitaiModelFile {
  name: string;
  sizeKB: number;
  downloadUrl: string;
  type: string;
  metadata?: Record<string, unknown>;
  hashes?: Record<string, string>;
}

export interface CivitaiModel {
  id: number;
  name: string;
  type: string;
  nsfw: boolean;
  tags?: string[];
  creator?: { username: string; image?: string };
  stats?: { downloadCount?: number; thumbsUpCount?: number; commentCount?: number; rating?: number; ratingCount?: number };
  modelVersions: Array<{
    id: number;
    name: string;
    baseModel?: string;
    files: CivitaiModelFile[];
    images: Array<{ url: string; nsfw?: string; width?: number; height?: number }>;
  }>;
}

export interface CivitaiSearchResponse {
  items: CivitaiModel[];
  metadata: {
    currentPage?: number;
    totalPages?: number;
    totalItems?: number;
    nextCursor?: string;
  };
}

export async function searchCivitaiModels(params: {
  query?: string;
  type?: string;
  baseModel?: string;
  fileFormat?: string;
  status?: string;
  sort?: string;
  period?: string;
  nsfw?: boolean;
  page?: number;
  cursor?: string;
  limit?: number;
  apiKey?: string;
}): Promise<CivitaiSearchResponse> {
  return invoke("civitai_search_models", { params });
}

export async function listCivitaiArchitectures(
  apiKey?: string
): Promise<string[]> {
  return invoke("civitai_list_architectures", { apiKey });
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
  metadata?: Record<string, string>,
  metadataMode?: string,
): Promise<string> {
  return invoke("save_to_gallery", { filename, subfolder, promptId, mode, metadata, metadataMode });
}

export async function saveToGalleryBytes(
  imageBytes: number[],
  filename: string,
  promptId: string,
  mode?: "txt2img" | "img2img" | "inpainting",
  metadata?: Record<string, string>,
  metadataMode?: string,
): Promise<string> {
  return invoke("save_to_gallery_bytes", { imageBytes, filename, promptId, mode, metadata, metadataMode });
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

export interface ModelSpec {
  architecture?: string;
  title?: string;
  description?: string;
  author?: string;
  resolution?: string;
  trigger_phrase?: string;
  usage_hint?: string;
  tags?: string;
  date?: string;
  license?: string;
  prediction_type?: string;
  thumbnail?: string;
  merge_recipe?: string;
  [key: string]: string | undefined;
}

export async function readModelSpec(
  category: string,
  filename: string
): Promise<ModelSpec | null> {
  return invoke("read_modelspec", { category, filename });
}

export async function checkNodeAvailable(nodeClass: string): Promise<boolean> {
  return invoke("check_node_available", { nodeClass });
}

export async function isCustomNodeInstalled(nodeName: string): Promise<boolean> {
  return invoke("is_custom_node_installed", { nodeName });
}

export async function installCustomNode(gitUrl: string, nodeName: string): Promise<void> {
  return invoke("install_custom_node", { gitUrl, nodeName });
}

export async function getConfig(): Promise<AppConfig> {
  return invoke("get_config");
}

export async function updateConfig(config: AppConfig): Promise<void> {
  return invoke("update_config", { config });
}
