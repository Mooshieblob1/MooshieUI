import { ipcInvoke, ipcListen, isBrowserMode, isTauri } from "./ipc.js";
import { getLogSnapshot } from "./log-buffer.js";
import type { ExportFormat } from "./videoExport.js";
import { locale } from "../stores/locale.svelte.js";
import type {
  AppConfig,
  GalleryImageEntry,
  GenerationMode,
  GenerationParams,
  GpuStats,
  InterrogationResult,
  LlmCatalogEntry,
  LlmHardware,
  LlmProviderState,
  LlmStatus,
  OutputImage,
  PromptAssistantOpts,
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
  /** Decimal string — 63-bit seeds exceed JS's safe-integer range. */
  seed: string;
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

export async function interruptGeneration(promptId?: string): Promise<void> {
  return promptId
    ? ipcInvoke("interrupt_generation", { promptId })
    : ipcInvoke("interrupt_generation");
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

export interface ComfyUiVersionInfo {
  /** Version installed on disk, if detectable. */
  installed: string | null;
  /** The pinned ComfyUI tag this MooshieUI build targets (e.g. "v0.26.0"). */
  target: string;
  /** True when the installed version is older than the pinned target. */
  update_available: boolean;
}

export async function getComfyuiVersion(): Promise<ComfyUiVersionInfo> {
  return ipcInvoke("get_comfyui_version");
}

export interface ComfyUiUpdateResult {
  updated: boolean;
  /** The tag the install was moved to. */
  target_ref: string;
  /** "git-fetch" or "git-init" (zip install converted to a managed checkout). */
  method: string;
}

export async function updateComfyui(): Promise<ComfyUiUpdateResult> {
  return ipcInvoke("update_comfyui");
}

export async function killPortProcess(): Promise<number> {
  return ipcInvoke("kill_port_process");
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

/** Cancel an in-progress model download by filename. The backend deletes the partial file. */
export async function cancelDownload(filename: string): Promise<void> {
  return ipcInvoke("cancel_download", { filename });
}

/**
 * Ask the backend for the real filename a download URL resolves to, read from the
 * server's Content-Disposition header (no file is downloaded). Returns null when
 * the server reports no usable name. Used to autopopulate the Model Hub direct
 * download filename, including for CivitAI links whose name is not in the URL.
 */
export async function resolveDownloadFilename(url: string): Promise<string | null> {
  return ipcInvoke("resolve_download_filename", { url });
}

export interface ModelInstallDir {
  path: string;
  label: string;
}

export interface ManagedModelFile {
  category: string;
  filename: string;
  directory: string;
  directory_label: string;
  path: string;
  size_bytes: number;
  modified_ms: number;
}

export async function getModelInstallDirs(
  category: string,
): Promise<ModelInstallDir[]> {
  return ipcInvoke("get_model_install_dirs", { category });
}

export async function listModelFiles(category: string): Promise<ManagedModelFile[]> {
  return ipcInvoke("list_model_files", { category });
}

export interface ManagedModelFolder {
  category: string;
  path: string;
  directory: string;
  directory_label: string;
}

/** Lists every subfolder (any depth, including empty ones) under a category's install dirs. */
export async function listModelFolders(category: string): Promise<ManagedModelFolder[]> {
  return ipcInvoke("list_model_folders", { category });
}

/** Creates a (possibly nested, e.g. "characters/anime") subfolder under a known install directory. */
export async function createModelFolder(
  category: string,
  directory: string,
  folderPath: string,
): Promise<void> {
  return ipcInvoke("create_model_folder", { category, directory, folderPath });
}

export async function deleteModelFile(
  category: string,
  filename: string,
  directory: string,
): Promise<void> {
  return ipcInvoke("delete_model_file", { category, filename, directory });
}

export async function moveModelFile(
  category: string,
  filename: string,
  sourceDirectory: string,
  targetDirectory: string,
  targetFilename?: string,
  targetCategory?: string,
): Promise<void> {
  return ipcInvoke("move_model_file", {
    category,
    filename,
    sourceDirectory,
    targetDirectory,
    targetFilename: targetFilename ?? filename,
    targetCategory: targetCategory ?? category,
  });
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

/** CivitAI image page URL or numeric image id. Returns API payload with `meta` when available. */
export async function civitaiLookupImage(
  imageRef: string,
): Promise<{ items?: Array<{ meta?: Record<string, unknown>; url?: string }> }> {
  return ipcInvoke("civitai_lookup_image", { imageRef });
}

export async function saveModelSidecarThumbnail(opts: {
  category: "checkpoints" | "loras";
  filename: string;
  imageUrl?: string;
  galleryFilename?: string;
}): Promise<void> {
  return ipcInvoke("save_model_sidecar_thumbnail", {
    category: opts.category,
    filename: opts.filename,
    imageUrl: opts.imageUrl,
    galleryFilename: opts.galleryFilename,
  });
}

/** CivitAI `baseModel` for a version hash, or null if not found / lookup failed. */
export async function lookupCivitaiBaseModel(hash: string): Promise<string | null> {
  try {
    const data = await civitaiLookupHash(hash);
    const bm = data.baseModel;
    return typeof bm === "string" ? bm : null;
  } catch {
    return null;
  }
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

export async function getCivitaiModel(
  modelId: number,
  apiKey?: string
): Promise<CivitaiModel> {
  return ipcInvoke("civitai_get_model", { modelId, apiKey });
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

export async function saveTextFile(
  content: string,
  path: string
): Promise<void> {
  return ipcInvoke("save_text_file", { content, path });
}

/**
 * Embed generation metadata into raw image bytes, returning bytes in the *same*
 * container format (PNG text chunk, JXL `xml ` box, or WebP EXIF/stealth alpha).
 */
export async function embedImageMetadataBytes(
  imageBytes: number[],
  metadata: Record<string, string>,
  metadataMode?: string
): Promise<number[]> {
  return ipcInvoke("embed_image_metadata_bytes", { imageBytes, metadata, metadataMode });
}

export async function saveToGallery(
  filename: string,
  subfolder: string,
  promptId: string,
  mode?: GenerationMode,
  metadata?: Record<string, string>,
  metadataMode?: string,
): Promise<string> {
  return ipcInvoke("save_to_gallery", { filename, subfolder, promptId, mode, metadata, metadataMode });
}

export async function saveToGalleryBytes(
  imageBytes: number[],
  filename: string,
  promptId: string,
  mode?: GenerationMode,
  metadata?: Record<string, string>,
  metadataMode?: string,
): Promise<string> {
  return ipcInvoke("save_to_gallery_bytes", { imageBytes, filename, promptId, mode, metadata, metadataMode });
}

export async function saveToGalleryTemp(
  tempFilename: string,
  filename: string,
  promptId: string,
  mode?: GenerationMode,
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

/**
 * Copy a gallery file to `destPath`. Desktop only: browser mode downloads
 * straight from the gallery URL instead. Used by save-video-as, because an
 * mp4 is too large to return through IPC as a byte array.
 */
export async function copyGalleryFileTo(filename: string, destPath: string): Promise<void> {
  return ipcInvoke("copy_gallery_file_to", { filename, destPath });
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

/** Copy a gallery image to the clipboard fully Rust-side (no image bytes cross IPC). */
export async function copyGalleryImageToClipboard(
  filename: string,
  metadata?: Record<string, string>,
  metadataMode?: string,
): Promise<void> {
  return ipcInvoke("copy_gallery_image_to_clipboard", { filename, metadata, metadataMode });
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
  // Derived by the backend, not declared in the file.
  base_model?: string;
  family?: string;
  is_sdxl_like?: string;
  turbo_model_variant?: string;
  recommended_vae?: string;
  recommended_clip_model?: string;
  recommended_clip_type?: string;
  hash?: string;
  filename_family_mismatch?: string;
  gguf_architecture?: string;
  /**
   * Which loader the weights actually need, regardless of the folder the file is
   * in: `"checkpoint"` (baked CLIP + VAE) or `"diffusion_model"` (unet/DiT only).
   * Absent when detection was inconclusive.
   */
  model_kind?: string;
  /** How `model_kind` was determined: `"tensor_keys"`, `"gguf"`, or `"family"`. */
  model_kind_source?: string;
  /** "true" when `architecture` came from tensor-key inference, not the file. */
  architecture_inferred?: string;
  /** Comma-joined names of the `modelspec.*` fields actually declared in the file. */
  modelspec_keys?: string;
  header_v_pred?: string;
  // SAI ModelSpec fields (`modelspec.*`, prefix stripped).
  sai_model_spec?: string;
  architecture?: string;
  implementation?: string;
  title?: string;
  description?: string;
  author?: string;
  date?: string;
  hash_sha256?: string;
  license?: string;
  usage_hint?: string;
  /** base64 data URI. */
  thumbnail?: string;
  tags?: string;
  merged_from?: string;
  prediction_type?: string;
  predict_key?: string;
  resolution?: string;
  trigger_phrase?: string;
  preprocessor?: string;
  encoder_layer?: string;
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
  family?: string;
  /** "data:<mime>;base64,..." for local sidecar, "https://..." for CivitAI, or undefined. */
  thumbnail_url?: string;
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
  family?: string;
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

/**
 * Is a ComfyUI node class registered? Pass `requiredInputs` when the class name
 * alone is ambiguous: ComfyUI lets a core node and a custom node claim the same
 * name (core wins), so the registered class can have an entirely different input
 * shape from the one the workflow builder emits.
 */
export async function checkNodeAvailable(
  nodeClass: string,
  requiredInputs?: string[],
): Promise<boolean> {
  return ipcInvoke("check_node_available", { nodeClass, requiredInputs });
}

export async function isCustomNodeInstalled(nodeName: string): Promise<boolean> {
  return ipcInvoke("is_custom_node_installed", { nodeName });
}

export async function installCustomNode(gitUrl: string, nodeName: string): Promise<void> {
  return ipcInvoke("install_custom_node", { gitUrl, nodeName });
}

/**
 * Are the RIFE frame-interpolation nodes and the `rife49.pth` checkpoint both on
 * disk? A disk check rather than a stored flag, so deleting the pack or pointing
 * the app at a different ComfyUI install re-arms the installer.
 */
export async function isRifeInstalled(): Promise<boolean> {
  return ipcInvoke("is_rife_installed", {});
}

/** Clone the frame-interpolation pack and download its checkpoint. Reports through `install:progress`. */
export async function installRife(): Promise<void> {
  return ipcInvoke("install_rife", {});
}

/**
 * Queue a RIFE pass over a finished gallery clip. Returns the prompt id; the
 * smoothed clip arrives as a new gallery entry through the normal video output
 * path, so the source is never overwritten.
 */
export async function interpolateVideo(
  filename: string,
  multiplier: number,
  scaleFactor: number,
  fastMode: boolean,
  ensemble: boolean,
): Promise<{ prompt_id: string }> {
  return ipcInvoke("interpolate_video", {
    filename,
    multiplier,
    scaleFactor,
    fastMode,
    ensemble,
  });
}

/**
 * Is the MiniMax-H3 Turbo node pack on disk? Only the pack: the adapter is a
 * regular LoRA download, so the panel checks `models.loras` for that half.
 */
export async function isH3TurboInstalled(): Promise<boolean> {
  return ipcInvoke("is_h3_turbo_installed", {});
}

/** Clone the MiniMax-H3 Turbo node pack. Reports through `install:progress`. */
export async function installH3Turbo(): Promise<void> {
  return ipcInvoke("install_h3_turbo", {});
}

/** Is the MiniMax-H3 TeaCache node pack on disk? */
export async function isH3TeacacheInstalled(): Promise<boolean> {
  return ipcInvoke("is_h3_teacache_installed", {});
}

/** Clone the MiniMax-H3 TeaCache node pack. Reports through `install:progress`. */
export async function installH3Teacache(): Promise<void> {
  return ipcInvoke("install_h3_teacache", {});
}

export async function installPipPackage(packageName: string): Promise<void> {
  return ipcInvoke("install_pip_package", { package: packageName });
}

export async function checkPythonImport(module: string): Promise<boolean> {
  return ipcInvoke("check_python_import", { module });
}

export interface BackendSupport {
  backend: string;
  supported: boolean;
  /** Reason code when unsupported: "no_nvidia_gpu" | "compute_capability" | "nvcc_missing". */
  reason: string | null;
  min_cc: number | null;
}

export interface AttentionBackendStatus {
  current: string;
  venv_packages: string[];
  compute_capability: number | null;
  os: string;
  nvcc_available: boolean;
  support: BackendSupport[];
}

export async function checkAttentionBackend(): Promise<AttentionBackendStatus> {
  return ipcInvoke("check_attention_backend");
}

export async function getComputeCapability(): Promise<number | null> {
  return ipcInvoke("get_compute_capability");
}

export async function installAttentionBackend(backend: string): Promise<void> {
  return ipcInvoke("install_attention_backend", { backend });
}

let configCache: AppConfig | null = null;
let configLoadPromise: Promise<AppConfig> | null = null;
let configUpdateChain: Promise<void> = Promise.resolve();
let pendingConfigWrite: AppConfig | null = null;

function cloneConfig(config: AppConfig): AppConfig {
  try {
    return structuredClone(config);
  } catch {
    // Fallback for non-cloneable reactive/proxy values.
    return JSON.parse(JSON.stringify(config)) as AppConfig;
  }
}

/** Last known config (clone). Used for instant settings UI on remount. */
export function getCachedConfig(): AppConfig | null {
  return configCache ? cloneConfig(configCache) : null;
}

export function primeConfigCache(config: AppConfig): void {
  configCache = cloneConfig(config);
}

async function flushPendingConfigWrite(): Promise<void> {
  while (pendingConfigWrite) {
    const toSave = cloneConfig(pendingConfigWrite);
    pendingConfigWrite = null;
    await ipcInvoke("update_config", { config: toSave });
    configCache = toSave;
  }
}

export async function getConfig(options?: { force?: boolean }): Promise<AppConfig> {
  const force = options?.force ?? false;
  if (!force && configCache) {
    return cloneConfig(configCache);
  }

  await configUpdateChain.catch(() => {});

  if (!force && configLoadPromise) {
    return configLoadPromise.then((c) => cloneConfig(c));
  }

  const load = ipcInvoke<AppConfig>("get_config")
    .then((c) => {
      const plain = cloneConfig(c);
      configCache = plain;
      return plain;
    })
    .finally(() => {
      if (configLoadPromise === load) {
        configLoadPromise = null;
      }
    });

  configLoadPromise = load;
  return load.then((c) => cloneConfig(c));
}

export async function updateConfig(config: AppConfig): Promise<void> {
  const plain = cloneConfig(config);
  configCache = plain;
  pendingConfigWrite = plain;
  configUpdateChain = configUpdateChain
    .catch(() => {})
    .then(() => flushPendingConfigWrite());
  return configUpdateChain;
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

export interface VideoExportResult {
  path: string;
  size_bytes: number;
  frame_count: number;
  /** 0-100, measured on the source frames. */
  seam_delta: number;
  /** What "auto" resolved to; echoes the request for the other modes. */
  applied_loop_mode: string;
  /**
   * Whether an audio track actually landed in the file. MP4 only, and asking is
   * not the same as getting: a source without audio degrades to a silent export
   * rather than failing.
   */
  has_audio: boolean;
}

export interface ExportCapability {
  available: boolean;
  reason: string | null;
  /** Whether this venv's PyAV can encode H.264. Gates the MP4 tab on its own. */
  mp4: boolean;
}

export async function exportVideoAnimation(args: {
  filename: string;
  format: ExportFormat;
  fps: number;
  width: number;
  quality: number;
  loopCount: number;
  loopMode: string;
  crossfadeFrames: number;
  keepAudio: boolean;
}): Promise<VideoExportResult> {
  return ipcInvoke("export_video_animation", {
    filename: args.filename,
    format: args.format,
    fps: args.fps,
    width: args.width,
    quality: args.quality,
    loopCount: args.loopCount,
    loopMode: args.loopMode,
    crossfadeFrames: args.crossfadeFrames,
    keepAudio: args.keepAudio,
  });
}

export async function probeVideoExport(): Promise<ExportCapability> {
  return ipcInvoke("probe_video_export", {});
}

export async function copyFileToClipboard(path: string): Promise<void> {
  return ipcInvoke("copy_file_to_clipboard", { path });
}

/**
 * Copy a file produced by the export pipeline to a caller-chosen destination.
 *
 * Desktop only: browser mode downloads straight from the export URL instead.
 * There is deliberately no webserver dispatch arm - this mirrors the reasoning
 * in copy_gallery_file_to, which is the exact analogue for gallery files.
 */
export async function copyFileTo(srcPath: string, destPath: string): Promise<void> {
  return ipcInvoke("copy_file_to", { srcPath, destPath });
}

/**
 * Browser-mode download URL for an export. The encode ran on the server, so the
 * browser fetches the produced file by basename out of the export temp dir.
 */
export function exportDownloadUrl(path: string): string {
  const name = path.split(/[\\/]/).pop() ?? "";
  return `/internal-api/_export/${encodeURIComponent(name)}`;
}

export async function interrogateGalleryImage(filename: string): Promise<InterrogationResult> {
  return ipcInvoke("interrogate_gallery_image", { filename });
}

export async function interrogateClipboard(): Promise<InterrogationResult> {
  return ipcInvoke("interrogate_clipboard");
}

export async function detectLlmHardware(): Promise<LlmHardware> {
  return ipcInvoke("detect_llm_hardware");
}

export async function listLlmCatalog(): Promise<LlmCatalogEntry[]> {
  return ipcInvoke("list_llm_catalog");
}

export async function llmStatus(): Promise<LlmStatus> {
  return ipcInvoke("llm_status");
}

export async function downloadLlmModel(id: string, variant: string): Promise<void> {
  return ipcInvoke("download_llm_model", { id, variant });
}

export async function deleteLlmModel(id: string): Promise<void> {
  return ipcInvoke("delete_llm_model", { id });
}

export async function unloadLlm(): Promise<void> {
  return ipcInvoke("unload_llm");
}

/**
 * Run a prompt-assistant command (enhance/compose/raw external call).
 *
 * Desktop (Tauri) calls the command synchronously — there is no proxy in front.
 * Browser mode is served through a reverse proxy (Cloudflare on the hosted
 * deployment) that aborts any request running past ~100s with a 524. Loading a
 * local LLM and generating can easily outlast that, so rather than block on the
 * POST body we fire the request with a correlation id and await the result over
 * the SSE event channel, which keep-alives and isn't bound by the proxy timeout.
 */
async function runPromptAssistant(
  command: "enhance_prompt" | "compose_prompt" | "call_external_llm",
  args: Record<string, unknown>,
): Promise<string> {
  if (!isBrowserMode) {
    return ipcInvoke<string>(command, args);
  }
  const requestId =
    globalThis.crypto?.randomUUID?.() ??
    `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  return new Promise<string>((resolve, reject) => {
    let settled = false;
    let unlistenResult: (() => void) | null = null;
    let unlistenError: (() => void) | null = null;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const finish = (action: () => void) => {
      if (settled) return;
      settled = true;
      if (timer) clearTimeout(timer);
      unlistenResult?.();
      unlistenError?.();
      action();
    };
    Promise.all([
      ipcListen("llm:result", (e: { payload: any }) => {
        if (e.payload?.request_id !== requestId) return;
        finish(() => resolve(String(e.payload.result ?? "")));
      }),
      ipcListen("llm:error", (e: { payload: any }) => {
        if (e.payload?.request_id !== requestId) return;
        finish(() =>
          reject(new Error(e.payload.error || "Prompt assistant failed")),
        );
      }),
    ])
      .then(([ur, ue]) => {
        // The request settled before listeners attached — clean up immediately.
        if (settled) {
          ur();
          ue();
          return;
        }
        unlistenResult = ur;
        unlistenError = ue;
        // Guard against a dropped SSE result so the UI can't hang forever.
        timer = setTimeout(
          () => finish(() => reject(new Error("Prompt assistant timed out"))),
          5 * 60 * 1000,
        );
        // Kick off the job; the backend returns { queued: true } right away and
        // broadcasts the real result/error over SSE when generation completes.
        ipcInvoke(command, { ...args, requestId }).catch((err) =>
          finish(() =>
            reject(err instanceof Error ? err : new Error(String(err))),
          ),
        );
      })
      .catch((err) =>
        finish(() =>
          reject(err instanceof Error ? err : new Error(String(err))),
        ),
      );
  });
}

export async function enhancePrompt(
  prompt: string,
  family: string,
  opts?: PromptAssistantOpts,
): Promise<string> {
  return runPromptAssistant("enhance_prompt", { prompt, family, opts });
}

export async function composePrompt(
  description: string,
  family: string,
  opts?: PromptAssistantOpts,
): Promise<string> {
  return runPromptAssistant("compose_prompt", { description, family, opts });
}

/**
 * Current external LLM provider settings. The API key never crosses this
 * boundary — only whether one is configured.
 */
export async function getLlmProvider(): Promise<LlmProviderState> {
  return ipcInvoke("get_llm_provider");
}

/** Switch providers. Clears the stored key when the provider actually changes. */
export async function setLlmProvider(provider: string): Promise<LlmProviderState> {
  return ipcInvoke("set_llm_provider", { provider });
}

/** Store (or clear) the key in Rust config. An empty key disables the external path. */
export async function setLlmApiKey(apiKey: string): Promise<LlmProviderState> {
  return ipcInvoke("set_llm_api_key", { apiKey });
}

export async function setLlmModel(model: string): Promise<LlmProviderState> {
  return ipcInvoke("set_llm_model", { model });
}

/** Only meaningful for the self-hosted "custom" provider; the rest pin their own. */
export async function setLlmBaseUrl(baseUrl: string): Promise<LlmProviderState> {
  return ipcInvoke("set_llm_base_url", { baseUrl });
}

/** `GET {base_url}/models` against the configured provider, ids only. */
export async function listExternalLlmModels(): Promise<string[]> {
  return ipcInvoke("list_external_llm_models");
}

/**
 * Desktop-only OAuth sign-in (PKCE, loopback redirect). Browser mode has no arm
 * for this: the loopback listener binds on the server, not the user's machine.
 */
export async function connectLlmOauth(provider: string): Promise<LlmProviderState> {
  return ipcInvoke("connect_llm_oauth", { provider });
}

/**
 * One system+user turn against whichever backend is configured — the external
 * provider when one is set up, otherwise the bundled local model. Used by the
 * H3 prompt rewrite, which needs its own system prompt instead of the booru
 * grounding enhance/compose apply.
 *
 * `imageFilename` names a frame already uploaded to ComfyUI's input folder.
 * Rust fetches, downscales, and inlines it, so the caller passes the name it
 * already has rather than shipping megabytes of base64 across the IPC boundary.
 * A model that cannot see one still answers from the text.
 */
export async function callExternalLlm(
  system: string,
  prompt: string,
  maxTokens?: number,
  imageFilename?: string | null,
): Promise<string> {
  return runPromptAssistant("call_external_llm", {
    system,
    prompt,
    maxTokens,
    imageFilename: imageFilename || undefined,
  });
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
  await ipcInvoke("export_logs", {
    destination,
    frontendLogs: getLogSnapshot(),
  });
}

// Omitting `destination` returns the diagnostic text instead of writing a file.
// Used by the diagnostics copy button and error reports on desktop, and in
// browser mode where the backend has no meaningful local path to write to for a
// remote browser (the caller triggers a client-side download).
export async function exportLogsContent(): Promise<string> {
  const res = await ipcInvoke<{ content: string }>("export_logs", {
    frontendLogs: getLogSnapshot(),
  });
  return res.content;
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
