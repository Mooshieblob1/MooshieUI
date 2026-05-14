import { ipcInvoke, isBrowserMode, isTauri } from "./ipc.js";
import { getLogSnapshot } from "./log-buffer.js";
import { locale } from "../stores/locale.svelte.js";
import type {
  AppConfig,
  GalleryImageEntry,
  GenerationParams,
  GpuStats,
  InterrogationResult,
  OutputImage,
  QueueInfo,
  SamplerInfo,
  SystemStats,
} from "../types/index.js";

export async function getModels(category: string): Promise<string[]> {
  return ipcInvoke("get_models", { category });
}

export async function getSamplers(): Promise<SamplerInfo> {
  return ipcInvoke("get_samplers");
}

export async function getEmbeddings(): Promise<string[]> {
  return ipcInvoke("get_embeddings");
}

export interface GenerateResponse {
  prompt_id: string;
  seed: number;
  queue_position?: number;
  queue_total?: number;
}

export async function generate(params: GenerationParams): Promise<GenerateResponse> {
  return ipcInvoke("generate", { params });
}

export interface ControlNetPreprocessorPreviewResponse {
  prompt_id: string;
}

export async function generateControlnetPreprocessorPreview(
  image: string,
  preprocessor: string,
): Promise<ControlNetPreprocessorPreviewResponse> {
  return ipcInvoke("generate_controlnet_preprocessor_preview", { image, preprocessor });
}

export async function getHistory(promptId: string): Promise<Record<string, unknown>> {
  return ipcInvoke("get_history", { promptId });
}

export async function recoverPromptOutputs(
  promptId: string,
): Promise<{ images: Array<{ temp_filename: string }> }> {
  return ipcInvoke("recover_prompt_outputs", { promptId });
}

export async function getQueue(): Promise<QueueInfo> {
  return ipcInvoke("get_queue");
}

export async function interruptGeneration(): Promise<void> {
  return ipcInvoke("interrupt_generation");
}

export async function deleteQueueItem(promptId: string): Promise<void> {
  return ipcInvoke("delete_queue_item", { promptId });
}

export async function clearAllQueues(): Promise<void> {
  return ipcInvoke("clear_all_queues");
}

export async function uploadImage(imagePath: string): Promise<{
  name: string;
  subfolder: string;
  type: string;
}> {
  return ipcInvoke("upload_image", { imagePath });
}

export async function uploadImageBytes(
  imageBytes: number[],
  filename: string
): Promise<{ name: string; subfolder: string; type: string }> {
  return ipcInvoke("upload_image_bytes", { imageBytes, filename });
}

export async function getOutputImage(
  filename: string,
  subfolder: string
): Promise<number[]> {
  return ipcInvoke("get_output_image", { filename, subfolder });
}

export async function getClientId(): Promise<string> {
  return ipcInvoke("get_client_id");
}

export type StartComfyuiResult = "spawned" | "already_running" | "skipped";

export async function startComfyui(): Promise<StartComfyuiResult> {
  return ipcInvoke("start_comfyui");
}

export async function stopComfyui(): Promise<void> {
  return ipcInvoke("stop_comfyui");
}

export async function checkServerHealth(): Promise<SystemStats> {
  return ipcInvoke("check_server_health");
}

export async function connectWs(): Promise<void> {
  return ipcInvoke("connect_ws");
}

export async function disconnectWs(): Promise<void> {
  return ipcInvoke("disconnect_ws");
}

export async function downloadModel(
  url: string,
  category: string,
  filename: string,
  installDir?: string,
  expectedSha256?: string,
): Promise<void> {
  return ipcInvoke("download_model", { url, category, filename, installDir, expectedSha256 });
}

export interface ModelInstallDir {
  path: string;
  label: string;
}

export async function getModelInstallDirs(
  category: string,
): Promise<ModelInstallDir[]> {
  return ipcInvoke("get_model_install_dirs", { category });
}

export async function openDirectory(path: string): Promise<void> {
  return ipcInvoke("open_directory", { path });
}

export async function findModelByHash(
  category: string,
  hash: string
): Promise<string | null> {
  return ipcInvoke("find_model_by_hash", { category, hash });
}

export async function hashModelFile(
  category: string,
  filename: string
): Promise<{ sha256: string; autov2: string }> {
  return ipcInvoke("hash_model_file", { category, filename });
}

export async function civitaiLookupHash(
  hash: string
): Promise<Record<string, unknown>> {
  return ipcInvoke("civitai_lookup_hash", { hash });
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
  return ipcInvoke("civitai_search_models", { params });
}

export async function listCivitaiArchitectures(
  apiKey?: string
): Promise<string[]> {
  return ipcInvoke("civitai_list_architectures", { apiKey });
}

export async function saveImageFile(
  imageBytes: number[],
  path: string
): Promise<void> {
  return ipcInvoke("save_image_file", { imageBytes, path });
}

export async function embedPngMetadataBytes(
  imageBytes: number[],
  metadata: Record<string, string>,
  metadataMode?: string
): Promise<number[]> {
  return ipcInvoke("embed_png_metadata_bytes", { imageBytes, metadata, metadataMode });
}

export async function saveToGallery(
  filename: string,
  subfolder: string,
  promptId: string,
  mode?: "txt2img" | "img2img" | "inpainting",
  metadata?: Record<string, string>,
  metadataMode?: string,
): Promise<string> {
  return ipcInvoke("save_to_gallery", { filename, subfolder, promptId, mode, metadata, metadataMode });
}

export async function saveToGalleryBytes(
  imageBytes: number[],
  filename: string,
  promptId: string,
  mode?: "txt2img" | "img2img" | "inpainting",
  metadata?: Record<string, string>,
  metadataMode?: string,
): Promise<string> {
  return ipcInvoke("save_to_gallery_bytes", { imageBytes, filename, promptId, mode, metadata, metadataMode });
}

export async function saveToGalleryTemp(
  tempFilename: string,
  filename: string,
  promptId: string,
  mode?: "txt2img" | "img2img" | "inpainting",
  metadata?: Record<string, string>,
  metadataMode?: string,
): Promise<string> {
  return ipcInvoke("save_to_gallery_temp", { tempFilename, filename, promptId, mode, metadata, metadataMode });
}

export async function readImageMetadata(
  filename: string
): Promise<Record<string, string> | null> {
  return ipcInvoke("read_image_metadata", { filename });
}

export async function readImageMetadataBytes(
  imageBytes: number[]
): Promise<Record<string, string> | null> {
  return ipcInvoke("read_image_metadata_bytes", { imageBytes });
}

export async function readImageMetadataPath(
  path: string
): Promise<Record<string, string> | null> {
  return ipcInvoke("read_image_metadata_path", { path });
}

export interface ReleaseNote {
  version: string;
  body: string;
  published_at: string;
}

export async function fetchReleaseNotes(): Promise<ReleaseNote[]> {
  return ipcInvoke("fetch_release_notes");
}

export async function listGalleryImages(): Promise<string[]> {
  return ipcInvoke("list_gallery_images");
}

export async function listGalleryImageEntries(): Promise<GalleryImageEntry[]> {
  return ipcInvoke("list_gallery_image_entries");
}

export interface ImportResult {
  imported: number;
  skipped: number;
  failed: number;
}

export async function importImageDirectory(directory: string): Promise<ImportResult> {
  return ipcInvoke("import_image_directory", { directory });
}

export async function loadGalleryImage(filename: string): Promise<number[]> {
  return ipcInvoke("load_gallery_image", { filename });
}

/** Load a gallery image transcoded to WebP for display (JXL → WebP in Rust). */
export async function loadGalleryImageDisplay(filename: string): Promise<number[]> {
  return ipcInvoke("load_gallery_image_display", { filename });
}

/** Load a gallery image encoded as PNG (JXL → PNG in Rust). Used for copy/save/download. */
export async function loadGalleryImagePng(filename: string): Promise<number[]> {
  return ipcInvoke("load_gallery_image_png", { filename });
}

/** Read a file from the temp_images directory by filename (no path traversal). */
export async function readTempImage(filename: string): Promise<number[]> {
  return ipcInvoke("read_temp_image", { filename });
}


export async function deleteGalleryImage(filename: string): Promise<void> {
  return ipcInvoke("delete_gallery_image", { filename });
}

export async function renameGalleryImage(oldFilename: string, newFilename: string): Promise<string> {
  return ipcInvoke("rename_gallery_image", { oldFilename, newFilename });
}

export async function copyImageToClipboard(filePath: string): Promise<void> {
  return ipcInvoke("copy_image_to_clipboard", { filePath });
}

export async function copyBytesToClipboard(bytes: number[], ext: string): Promise<void> {
  return ipcInvoke("copy_bytes_to_clipboard", { bytes, ext });
}

export async function getGalleryImagePath(filename: string): Promise<string> {
  return ipcInvoke("get_gallery_image_path", { filename });
}

// ---------------------------------------------------------------------------
// Storage management (browser mode only — uses direct HTTP endpoints)
// ---------------------------------------------------------------------------

export interface StorageImageInfo {
  filename: string;
  size_bytes: number;
  age_secs: number;
  expires_in_secs: number;
}

export interface StorageInfo {
  usage_bytes: number;
  limit_bytes: number;
  expiry_secs: number;
  image_count: number;
  images: StorageImageInfo[];
}

export async function getStorageInfo(): Promise<StorageInfo> {
  const { isBrowserMode, authHeaders } = await import("./ipc.js");
  if (!isBrowserMode) {
    // Desktop mode: no storage limits
    return { usage_bytes: 0, limit_bytes: 0, expiry_secs: 0, image_count: 0, images: [] };
  }
  const resp = await fetch("/internal-api/_storage/info", { headers: authHeaders() });
  if (!resp.ok) throw new Error(`Storage info request failed: ${resp.status}`);
  return resp.json();
}

export async function setStorageLimit(username: string, limitBytes: number): Promise<void> {
  const { isBrowserMode, authHeaders } = await import("./ipc.js");
  if (!isBrowserMode) return;
  const resp = await fetch("/internal-api/_storage/set_limit", {
    method: "POST",
    headers: { ...authHeaders(), "Content-Type": "application/json" },
    body: JSON.stringify({ username, limit_bytes: limitBytes }),
  });
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}));
    throw new Error(data.error || `Failed to set storage limit: ${resp.status}`);
  }
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
  return ipcInvoke("read_modelspec", { category, filename });
}

export interface LoraCivitaiImage {
  url: string;
  width?: number;
  height?: number;
  nsfw?: string;
}

export interface LoraCivitaiInfo {
  filename: string;
  hash?: string;
  civitai_name?: string;
  civitai_description?: string;
  civitai_model_id?: number;
  civitai_version_id?: number;
  civitai_base_model?: string;
  civitai_images: LoraCivitaiImage[];
  civitai_trigger_words: string[];
  civitai_download_count?: number;
  civitai_thumbs_up_count?: number;
  civitai_creator?: string;
  modelspec_title?: string;
  modelspec_author?: string;
  modelspec_architecture?: string;
  modelspec_trigger_phrase?: string;
  modelspec_description?: string;
  modelspec_tags?: string;
}

export interface CheckpointCivitaiInfo {
  filename: string;
  hash?: string;
  display_name?: string;
  base_model?: string;
  /** "data:<mime>;base64,..." for local sidecar, "https://..." for CivitAI, or undefined. */
  thumbnail_url?: string;
  civitai_model_id?: number;
  civitai_version_id?: number;
  civitai_description?: string;
  civitai_images: LoraCivitaiImage[];
  civitai_download_count?: number;
  civitai_thumbs_up_count?: number;
  civitai_creator?: string;
  modelspec_title?: string;
  modelspec_author?: string;
  modelspec_architecture?: string;
  modelspec_description?: string;
  modelspec_tags?: string;
}

export async function getLoraCivitaiInfo(
  filename: string
): Promise<LoraCivitaiInfo> {
  return ipcInvoke("get_lora_civitai_info", { filename });
}

export async function getCheckpointCivitaiInfo(
  filename: string
): Promise<CheckpointCivitaiInfo> {
  return ipcInvoke("get_checkpoint_civitai_info", { filename });
}

/**
 * Fetch a remote image through the Rust backend so CivitAI auth headers
 * are applied and the result is cached to disk per-user.
 * Returns a "data:<mime>;base64,..." string ready for use in <img src>.
 */
export async function fetchCachedImage(url: string): Promise<string> {
  return ipcInvoke("fetch_cached_image", { url });
}

export async function checkNodeAvailable(nodeClass: string): Promise<boolean> {
  return ipcInvoke("check_node_available", { nodeClass });
}

export async function isCustomNodeInstalled(nodeName: string): Promise<boolean> {
  return ipcInvoke("is_custom_node_installed", { nodeName });
}

export async function installCustomNode(gitUrl: string, nodeName: string): Promise<void> {
  return ipcInvoke("install_custom_node", { gitUrl, nodeName });
}

export async function installPipPackage(packageName: string): Promise<void> {
  return ipcInvoke("install_pip_package", { package: packageName });
}

export interface AttentionBackendStatus {
  current: string;
  venv_packages: string[];
  compute_capability: number | null;
}

export async function checkAttentionBackend(): Promise<AttentionBackendStatus> {
  return ipcInvoke("check_attention_backend");
}

export async function getComputeCapability(): Promise<number | null> {
  const status = await checkAttentionBackend();
  return status.compute_capability;
}

export async function installAttentionBackend(backend: string): Promise<void> {
  return ipcInvoke("install_attention_backend", { backend });
}

export async function getConfig(): Promise<AppConfig> {
  return ipcInvoke("get_config");
}

export async function updateConfig(config: AppConfig): Promise<void> {
  return ipcInvoke("update_config", { config });
}

export async function getGalleryPath(): Promise<string> {
  return ipcInvoke("get_gallery_path");
}

export async function setGalleryPath(path: string): Promise<string> {
  return ipcInvoke("set_gallery_path", { path });
}

export async function interrogateImage(imageBase64: string): Promise<InterrogationResult> {
  return ipcInvoke("interrogate_image", { imageBase64 });
}

export async function interrogateImagePath(path: string): Promise<InterrogationResult> {
  return ipcInvoke("interrogate_image_path", { path });
}

export async function interrogateGalleryImage(filename: string): Promise<InterrogationResult> {
  return ipcInvoke("interrogate_gallery_image", { filename });
}

export async function interrogateClipboard(): Promise<InterrogationResult> {
  return ipcInvoke("interrogate_clipboard");
}

export async function readClipboardImage(): Promise<number[]> {
  return ipcInvoke("read_clipboard_image");
}

/**
 * Read an image from the clipboard, with browser-mode fallback.
 * In Tauri: uses the native clipboard command.
 * In browser mode: uses the Web Clipboard API (navigator.clipboard.read()).
 * Returns raw image bytes as a number array.
 */
export async function readClipboardImageSafe(): Promise<number[]> {
  if (!isBrowserMode) {
    return readClipboardImage();
  }
  // Try the browser Clipboard API first (requires HTTPS or localhost)
  if (navigator.clipboard?.read) {
    try {
      const items = await navigator.clipboard.read();
      for (const item of items) {
        for (const type of item.types) {
          if (type.startsWith("image/")) {
            const blob = await item.getType(type);
            const buffer = await blob.arrayBuffer();
            return [...new Uint8Array(buffer)];
          }
        }
      }
    } catch {
      // Clipboard API blocked — fall through to server fallback
    }
  }
  // Fallback: ask the server to read from the host OS clipboard
  return readClipboardImage();
}

export async function exportLogs(destination: string): Promise<void> {
  return ipcInvoke("export_logs", {
    destination,
    frontendLogs: getLogSnapshot(),
  });
}

export async function getGpuStats(): Promise<GpuStats[]> {
  if (isTauri) {
    return ipcInvoke("get_gpu_stats");
  }
  if (!isBrowserMode) return [];
  const { getAuthToken } = await import("./ipc.js");
  const headers: Record<string, string> = {};
  const token = getAuthToken();
  if (token) headers["Authorization"] = `Bearer ${token}`;
  const resp = await fetch("/internal-api/_gpu_stats", { headers });
  if (!resp.ok) throw new Error(await resp.text());
  return resp.json();
}
