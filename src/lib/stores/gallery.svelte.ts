import type {
  CompareMode,
  CompareOrientation,
  GenerationMode,
  OutputImage,
} from "../types/index.js";
import {
  listGalleryImageEntries,
  loadGalleryImage,
  loadGalleryImageDisplay,
  loadGalleryImagePng,
  saveToGallery,
  saveToGalleryBytes,
  saveToGalleryTemp,
  deleteGalleryImage,
  renameGalleryImage,
  saveImageFile,
  embedImageMetadataBytes,
  getOutputImage,
  copyImageToClipboard,
  copyBytesToClipboard,
  copyGalleryImageToClipboard,
  copyFileToClipboard,
  getGalleryImagePath,
  getStorageInfo,
  readImageMetadata,
  copyGalleryFileTo,
  type StorageInfo,
} from "../utils/api.js";
import { isTauri, isBrowserMode, getAuthToken } from "../utils/ipc.js";
import { locale } from "./locale.svelte.js";
import { generation } from "./generation.svelte.js";
import { progress } from "./progress.svelte.js";
import { createArtistGalleryClient } from "../artist-gallery/client.js";
import { cdnFetch } from "../utils/cdnFetch.js";
import {
  buildArtistTagIndex,
  detectArtistsInPrompt,
  artistBoardName,
  type ArtistTagIndex,
} from "../artist-gallery/detection.js";
import type { ArtistSearchHit } from "../artist-gallery/types.js";

/**
 * Whether a gallery entry is a video. Keyed off the stored filename rather
 * than a flag, because every path that produces an `OutputImage` already
 * knows the filename and `.mp4` is the only video extension the save
 * pipeline writes.
 */
export function isVideoImage(image: OutputImage): boolean {
  return image.gallery_filename?.toLowerCase().endsWith(".mp4") ?? false;
}

/**
 * Mode segment of a modern gallery filename (`{promptId}__{mode}__{name}`),
 * or undefined for legacy names and the backend's "unknown" placeholder.
 */
function parseGenerationMode(value: string): GenerationMode | undefined {
  switch (value) {
    case "txt2img":
    case "img2img":
    case "inpainting":
    case "image_edit":
    case "video":
      return value;
    default:
      return undefined;
  }
}

/** Convert a gallery filename to a thumbnail URL. In Tauri, uses the custom protocol; in browser, uses the HTTP server. */
async function thumbnailUrl(filename: string): Promise<string> {
  if (isTauri) {
    const { convertFileSrc } = await import("@tauri-apps/api/core");
    return convertFileSrc(filename, "thumbnail");
  }
  const token = getAuthToken();
  const base = `/internal-api/_thumbnail/${encodeURIComponent(filename)}`;
  return token ? `${base}?token=${encodeURIComponent(token)}` : base;
}

/** Convert a gallery filename to a full-resolution image URL (serves original PNG with metadata). */
async function fullImageUrl(filename: string): Promise<string> {
  if (isTauri) {
    const { convertFileSrc } = await import("@tauri-apps/api/core");
    return convertFileSrc(filename, "gallery");
  }
  const token = getAuthToken();
  const base = `/internal-api/_gallery/${encodeURIComponent(filename)}`;
  return token ? `${base}?token=${encodeURIComponent(token)}` : base;
}

/** Convert a temp filename to a browser-loadable URL, including auth token in browser mode. */
function tempImageUrl(filename: string, params?: Record<string, string>): string {
  const query = new URLSearchParams(params ?? {});
  const token = getAuthToken();
  if (token) query.set("token", token);
  const suffix = query.toString();
  return `/internal-api/_temp_image/${encodeURIComponent(filename)}${suffix ? `?${suffix}` : ""}`;
}

/** Show a native save dialog. Returns the chosen path, or null. Tauri-only; browser falls back to download. */
async function showSaveDialog(defaultPath: string, extensions: string[]): Promise<string | null> {
  if (isTauri) {
    const { save } = await import("@tauri-apps/plugin-dialog");
    return save({ defaultPath, filters: [{ name: "Images", extensions }] });
  }
  // Browser mode: not used for MVP1 — callers should use browser download instead
  return null;
}

/** Trigger a file download in the browser using a temporary anchor element. */
function triggerBrowserDownload(data: Uint8Array, filename: string, mimeType: string) {
  const buffer = new ArrayBuffer(data.byteLength);
  new Uint8Array(buffer).set(data);
  const blob = new Blob([buffer], { type: mimeType });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

async function blobToBytes(blob: Blob): Promise<number[]> {
  const buffer = await blob.arrayBuffer();
  return Array.from(new Uint8Array(buffer));
}

/** True when bytes are a PNG file (not JPEG/WebP mislabeled as PNG). */
function isPngBytes(bytes: ArrayLike<number>): boolean {
  return (
    bytes.length >= 8
    && bytes[0] === 0x89
    && bytes[1] === 0x50
    && bytes[2] === 0x4e
    && bytes[3] === 0x47
    && bytes[4] === 0x0d
    && bytes[5] === 0x0a
    && bytes[6] === 0x1a
    && bytes[7] === 0x0a
  );
}

function pngBlobFromBytes(bytes: number[]): Blob {
  const buffer = new ArrayBuffer(bytes.length);
  new Uint8Array(buffer).set(bytes);
  return new Blob([buffer], { type: "image/png" });
}

const PNG_SAVE_EXTENSIONS = ["png"];
const PNG_NORMALIZED_EXTENSION_RE = /\.(jxl|webp|jpe?g)$/i;

function pngNormalizedFilename(filename: string): string {
  return filename.replace(PNG_NORMALIZED_EXTENSION_RE, ".png");
}

const WEBP_SAVE_EXTENSIONS = ["webp"];
const MP4_SAVE_EXTENSIONS = ["mp4"];
const WEBP_NORMALIZED_EXTENSION_RE = /\.(jxl|png|jpe?g)$/i;

/**
 * WebP gallery files are exported as-is rather than transcoded to PNG: the
 * format is universally readable and the file already carries its metadata
 * (EXIF UserComment plus, in stealth mode, the alpha LSBs).
 */
function webpNormalizedFilename(filename: string): string {
  return WEBP_NORMALIZED_EXTENSION_RE.test(filename)
    ? filename.replace(WEBP_NORMALIZED_EXTENSION_RE, ".webp")
    : filename;
}

function isWebpGalleryFile(filename: string | undefined): boolean {
  return filename?.toLowerCase().endsWith(".webp") ?? false;
}

const GALLERY_BOARDS_KEY = "mooshieui.gallery.boards.v1";
const GALLERY_BOARD_NAMES_KEY = "mooshieui.gallery.boardNames.v1";
const GALLERY_SHOW_GEN_TIME_KEY = "mooshieui.gallery.showGenerationTime.v1";

type ToastType = "success" | "error" | "info" | "warning";
type ToastOptions = {
  persistent?: boolean;
  actionLabel?: string;
  onAction?: () => void;
  /** Auto-dismiss delay in ms (ignored when persistent). Defaults to 2000. */
  durationMs?: number;
};
type GalleryToast = {
  message: string;
  type: ToastType;
  persistent?: boolean;
  actionLabel?: string;
  onAction?: () => void;
  durationMs?: number;
};

class GalleryStore {
  images = $state<OutputImage[]>([]);
  /** Images generated during this app session (not loaded from disk). */
  sessionImages = $state<OutputImage[]>([]);
  selectedImage = $state<OutputImage | null>(null);
  lastSelectedImage = $state<OutputImage | null>(null);
  /** When set, the lightbox shows this URL instead of selectedImage. */
  lightboxUrl = $state<string | null>(null);
  lightboxOpen = $state(false);
  lightboxLoading = $state(false);
  loading = $state(false);
  /** True while a save/download operation is in progress (prevents double-clicks). */
  saving = $state(false);
  toast = $state<GalleryToast | null>(null);
  boardAssignments = $state<Record<string, string>>({});
  customBoards = $state<string[]>([]);
  /** Shown as a badge on thumbnails and the lightbox when true; persisted across sessions. */
  showGenerationTime = $state(false);
  /** Storage info from the server (browser mode only). */
  storageInfo = $state<StorageInfo | null>(null);
  /** Lookup map for artist-tag detection.  Populated by loadArtistIndex(). */
  artistTagIndex = $state<ArtistTagIndex>(new Map());
  /** True once the artist index has been fetched at least once. */
  artistIndexReady = $state(false);
  /** True while autoSortByArtist is running. */
  autoSorting = $state(false);
  /** A/B comparison viewer (issue #517). */
  compareOpen = $state(false);
  compareA = $state<OutputImage | null>(null);
  compareB = $state<OutputImage | null>(null);
  compareUrlA = $state<string | null>(null);
  compareUrlB = $state<string | null>(null);
  compareLoading = $state(false);
  /** Blend mode and divider axis, kept across opens within a session. */
  compareMode = $state<CompareMode>("slider");
  compareOrientation = $state<CompareOrientation>("horizontal");
  /** Image held as side A while the user picks the second image to compare. */
  comparePin = $state<OutputImage | null>(null);
  /** Blob URLs this store created for the comparison and must revoke itself. */
  private _compareOwnedUrls = new Set<string>();
  /** Bumped on every open/side change so stale async resolves are discarded. */
  private _compareToken = 0;
  private _toastTimer: ReturnType<typeof setTimeout> | null = null;
  /**
   * Per-image persist promises.  Resolves with the gallery filename once
   * the image has been saved to disk and metadata embedded.  Created up-front
   * in persistImages() so callers (copyToClipboard, openLightbox) can await
   * completion even if persist hasn't finished yet.
   */
  private _persistPromises = new Map<string, Promise<string>>();

  private _imageKey(img: { prompt_id: string; filename: string }) {
    return `${img.prompt_id}::${img.filename}`;
  }

  /** Return the in-flight persist promise for an image, if save is still pending. */
  getPersistPromise(img: { prompt_id: string; filename: string }): Promise<string> | undefined {
    return this._persistPromises.get(this._imageKey(img));
  }

  constructor() {
    this.loadBoardAssignments();
    this.loadCustomBoards();
    this.loadShowGenerationTime();
  }

  private loadShowGenerationTime() {
    try {
      this.showGenerationTime = localStorage.getItem(GALLERY_SHOW_GEN_TIME_KEY) === "true";
    } catch (e) {
      console.error("Failed to load showGenerationTime pref:", e);
    }
  }

  setShowGenerationTime(value: boolean) {
    this.showGenerationTime = value;
    try {
      localStorage.setItem(GALLERY_SHOW_GEN_TIME_KEY, String(value));
    } catch (e) {
      console.error("Failed to save showGenerationTime pref:", e);
    }
  }

  private loadBoardAssignments() {
    try {
      const raw = localStorage.getItem(GALLERY_BOARDS_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as Record<string, string>;
      if (!parsed || typeof parsed !== "object") return;
      this.boardAssignments = parsed;
    } catch (e) {
      console.error("Failed to load gallery boards:", e);
    }
  }

  private saveBoardAssignments() {
    try {
      localStorage.setItem(GALLERY_BOARDS_KEY, JSON.stringify(this.boardAssignments));
    } catch (e) {
      console.error("Failed to save gallery boards:", e);
    }
  }

  private loadCustomBoards() {
    try {
      const raw = localStorage.getItem(GALLERY_BOARD_NAMES_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as string[];
      if (!Array.isArray(parsed)) return;
      this.customBoards = parsed.filter((name) => !!name && name !== "Unsorted");
    } catch (e) {
      console.error("Failed to load custom boards:", e);
    }
  }

  private saveCustomBoards() {
    try {
      localStorage.setItem(GALLERY_BOARD_NAMES_KEY, JSON.stringify(this.customBoards));
    } catch (e) {
      console.error("Failed to save custom boards:", e);
    }
  }

  get boards(): string[] {
    const unique = new Set<string>();
    for (const board of this.customBoards) {
      if (board && board !== "Unsorted") unique.add(board);
    }
    for (const board of Object.values(this.boardAssignments)) {
      if (board && board !== "Unsorted") unique.add(board);
    }
    return [...unique].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));
  }

  addBoard(name: string) {
    const normalized = name.trim();
    if (!normalized || normalized === "Unsorted") return;
    if (this.customBoards.includes(normalized)) return;
    this.customBoards = [...this.customBoards, normalized].sort((a, b) =>
      a.localeCompare(b, undefined, { sensitivity: "base" })
    );
    this.saveCustomBoards();
  }

  getBoard(image: OutputImage): string {
    const key = image.gallery_filename ?? `${image.prompt_id}::${image.filename}`;
    return this.boardAssignments[key] || "Unsorted";
  }

  setBoard(image: OutputImage, board: string) {
    const key = image.gallery_filename ?? `${image.prompt_id}::${image.filename}`;
    const value = board.trim() || "Unsorted";
    if (value !== "Unsorted") this.addBoard(value);
    this.boardAssignments = {
      ...this.boardAssignments,
      [key]: value,
    };
    this.saveBoardAssignments();
  }

  // ---------------------------------------------------------------------------
  // Artist tag detection
  // ---------------------------------------------------------------------------

  private _artistIndexPromise: Promise<void> | null = null;

  /** Fetch and cache the artist search index from the given manifest URL. */
  async loadArtistIndex(manifestUrl: string): Promise<void> {
    if (this.artistIndexReady) return;
    if (this._artistIndexPromise) return this._artistIndexPromise;
    this._artistIndexPromise = (async () => {
      try {
        const client = createArtistGalleryClient({ manifestUrl, fetchImpl: cdnFetch });
        const hits = await client.loadSearchIndex();
        this.artistTagIndex = buildArtistTagIndex(hits);
        this.artistIndexReady = true;
        console.debug(`[artist] Index loaded: ${hits.length} entries, map size=${this.artistTagIndex.size}`);
      } catch (e) {
        console.error("Failed to load artist tag index:", e);
        this._artistIndexPromise = null;
      }
    })();
    return this._artistIndexPromise;
  }

  /** Detect artist tags present in an image's positive prompt. */
  detectedArtists(image: OutputImage): ArtistSearchHit[] {
    if (!this.artistIndexReady || this.artistTagIndex.size === 0) {
      console.debug(`[artist] detectedArtists: index not ready (ready=${this.artistIndexReady}, size=${this.artistTagIndex.size})`);
      return [];
    }
    const prompt = image.metadata?.positive_prompt;
    if (!prompt) {
      console.debug(`[artist] detectedArtists: no prompt for ${image.gallery_filename} (metadata=${JSON.stringify(image.metadata)})`);
      return [];
    }
    const hits = detectArtistsInPrompt(prompt, this.artistTagIndex);
    console.debug(`[artist] detectedArtists for ${image.gallery_filename}: prompt snippet="${prompt.slice(0, 80)}", hits=${hits.map(h => h.slug).join(",") || "none"}`);
    return hits;
  }

  /** The primary (first-ranked) artist tag detected in an image, or null. */
  primaryArtist(image: OutputImage): ArtistSearchHit | null {
    const hits = this.detectedArtists(image);
    return hits[0] ?? null;
  }

  /**
   * Scan every gallery image and assign it to a board named `@artist` based
   * on the primary detected artist tag.  Images with no detectable artist
   * tag are left untouched (existing board assignments preserved).  Returns
   * a summary of how many images were sorted.
   */
  async autoSortByArtist(
    manifestUrl: string,
    options: { overwriteExisting?: boolean } = {},
  ): Promise<{ sorted: number; scanned: number; boards: string[] }> {
    this.autoSorting = true;
    try {
      await this.loadArtistIndex(manifestUrl);
      if (!this.artistIndexReady) {
        this.showToast(locale.t("gallery.artist_index_unavailable"), "error");
        return { sorted: 0, scanned: 0, boards: [] };
      }
      // Make sure every image has its metadata loaded before scanning so we
      // don't miss images whose prompts haven't been hydrated yet.
      await this.hydrateMetadataInBackground();
      const overwrite = options.overwriteExisting ?? false;
      const newBoards = new Set<string>();
      const nextAssignments = { ...this.boardAssignments };
      let sorted = 0;
      let scanned = 0;
      for (const image of this.images) {
        scanned++;
        const key = image.gallery_filename ?? `${image.prompt_id}::${image.filename}`;
        if (!overwrite) {
          const existing = this.boardAssignments[key];
          if (existing && existing !== "Unsorted") continue;
        }
        const primary = this.primaryArtist(image);
        if (!primary) continue;
        const boardName = artistBoardName(primary.slug);
        nextAssignments[key] = boardName;
        newBoards.add(boardName);
        sorted++;
      }
      if (newBoards.size > 0) {
        const merged = new Set<string>(this.customBoards);
        for (const b of newBoards) merged.add(b);
        this.customBoards = [...merged].sort((a, b) =>
          a.localeCompare(b, undefined, { sensitivity: "base" }),
        );
        this.saveCustomBoards();
      }
      this.boardAssignments = nextAssignments;
      this.saveBoardAssignments();
      return { sorted, scanned, boards: [...newBoards] };
    } finally {
      this.autoSorting = false;
    }
  }

  addImages(newImages: OutputImage[]) {
    this.images = [...newImages, ...this.images];
    this.sessionImages = [...newImages, ...this.sessionImages];
  }

  async openLightbox(image: OutputImage) {
    this.selectedImage = image;
    this.lastSelectedImage = image;
    this.lightboxOpen = true;
    const isJxl = image.gallery_filename?.endsWith(".jxl") ?? false;
    // Videos always play from the streaming URL. Never blob-ify an mp4: a 15 s
    // H3 clip can be hundreds of MB, and <video> needs Range requests to seek.
    if (isVideoImage(image)) {
      const videoFilename = await this.resolveGalleryFilename(image);
      if (videoFilename) {
        this.lightboxUrl = await fullImageUrl(videoFilename);
        this.lightboxLoading = false;
        return;
      }
    }
    if (image.fullImageUrl && !isJxl) {
      // Serve the real image from backend — supports right-click → Save with metadata.
      // JXL is excluded: WebView2 cannot decode JXL natively, so we always use the
      // blob URL (WebP) for display.
      this.lightboxUrl = image.fullImageUrl;
      this.lightboxLoading = false;
    } else if (image.url) {
      // Session images still have a blob URL — show it immediately.
      // For JXL we keep the WebP blob URL permanently (no upgrade to gallery://
      // URL since WebView2 can't display JXL). For PNG we upgrade to the
      // gallery:// URL once persistence completes — but only if the resolved
      // filename is NOT a JXL file (guard against the race where gallery_filename
      // wasn't set yet when isJxl was computed above).
      this.lightboxUrl = image.url;
      this.lightboxLoading = false;
      if (!isJxl) {
        const key = this._imageKey(image);
        const pending = this._persistPromises.get(key);
        if (pending) {
          const galleryFilename = await pending;
          if (galleryFilename && this.lightboxOpen && this.selectedImage && this._imageKey(this.selectedImage) === key) {
            if (galleryFilename.endsWith(".jxl")) {
              // Persist completed and it turned out to be JXL — don't upgrade to
              // gallery:// (raw JXL, WebView2 can't decode). The WebP blob URL in
              // image.url is already correct.
            } else {
              this.lightboxUrl = await fullImageUrl(galleryFilename);
            }
          }
        }
      }
    } else if (image.gallery_filename) {
      // Persisted images without a blob URL — load full-res from disk.
      // JXL files are transcoded to WebP on the fly by loadFullImage.
      this.lightboxUrl = null;
      this.lightboxLoading = true;
      try {
        const fullUrl = await this.loadFullImage(image.gallery_filename);
        this.lightboxUrl = fullUrl;
      } catch (e) {
        console.error("Failed to load full image:", e);
      } finally {
        this.lightboxLoading = false;
      }
    }
  }

  /** Whether the open lightbox is showing a video rather than a still. */
  get lightboxIsVideo(): boolean {
    return this.selectedImage ? isVideoImage(this.selectedImage) : false;
  }

  /** Open lightbox with a raw image URL (e.g. preview blob). */
  openLightboxUrl(url: string) {
    this.selectedImage = null;
    this.lightboxUrl = url;
    this.lightboxOpen = true;
    // If the URL belongs to a known session image, remember it so features
    // like "use as model thumbnail" can resolve a gallery file later (#232).
    const match = this.sessionImages.find((img) => img.url === url);
    if (match) this.lastSelectedImage = match;
  }

  /**
   * Resolve the persisted gallery filename for an image, waiting for an
   * in-flight persist if necessary. Returns null when the image was never
   * saved to the gallery (e.g. manual-save mode without saving).
   */
  async resolveGalleryFilename(image: OutputImage): Promise<string | null> {
    if (image.gallery_filename) return image.gallery_filename;
    const pending = this._persistPromises.get(this._imageKey(image));
    if (pending) {
      const galleryFilename = await pending;
      if (galleryFilename) return galleryFilename;
    }
    return image.gallery_filename ?? null;
  }

  /**
   * Insert an already-persisted gallery file (e.g. one saved from the Photopea
   * editor) at the top of the gallery. The backend's "image_saved" broadcast is
   * not consumed by the frontend, so newly written files must be surfaced here.
   */
  async addPersistedImage(
    galleryFilename: string,
    videoMeta?: { duration_seconds?: number; fps?: number; generationTimeMs?: number },
    addToSession = false,
  ) {
    // Mirror loadFromDisk's modern-format parse: {promptId}__{mode}__{origFilename}.
    let promptId = "";
    let origFilename = galleryFilename;
    let generationMode: GenerationMode | undefined;
    let isUpscaled = false;
    const modernParts = galleryFilename.split("__");
    if (modernParts.length >= 3) {
      promptId = modernParts[0] ?? "";
      generationMode = parseGenerationMode(modernParts[1] ?? "");
      origFilename = modernParts.slice(2).join("__");
      const lowered = origFilename.toLowerCase();
      isUpscaled = lowered.includes("upscale") || lowered.includes("upscaled");
    }

    const entry: OutputImage = {
      filename: origFilename,
      subfolder: "",
      type: "output",
      prompt_id: promptId,
      generation_mode: generationMode,
      is_upscaled: isUpscaled,
      thumbnailUrl: await thumbnailUrl(galleryFilename),
      fullImageUrl: await fullImageUrl(galleryFilename),
      gallery_filename: galleryFilename,
      generated_at_ms: Date.now(),
      duration_seconds: videoMeta?.duration_seconds,
      fps: videoMeta?.fps,
      generationTimeMs: videoMeta?.generationTimeMs,
    };
    this.images = [entry, ...this.images];
    // The "generated this session" tabs (e.g. BottomPanel's Videos tab) read
    // sessionImages, not images — videos land here instead of addImages()
    // because they're saved server-side rather than streamed as blobs, so
    // they need to be added to both to show up while the app is still open.
    if (addToSession) {
      this.sessionImages = [entry, ...this.sessionImages];
    }
    // Pull in the embedded metadata (artist detection etc.) in the background.
    void this.hydrateMetadataInBackground();
  }

  closeLightbox() {
    if (this.lightboxUrl?.startsWith("blob:")) {
      // Don't revoke a blob URL that is still referenced by a session image,
      // by progress.lastOutputImage, or by the open comparison viewer —
      // revoking would break subsequent lightbox opens, the post-generation
      // preview in PreviewImage, or a comparison side.
      const isShared =
        this.sessionImages.some((img) => img.url === this.lightboxUrl) ||
        progress.lastOutputImage === this.lightboxUrl ||
        progress.previewImage === this.lightboxUrl ||
        this.compareUrlA === this.lightboxUrl ||
        this.compareUrlB === this.lightboxUrl;
      if (!isShared) URL.revokeObjectURL(this.lightboxUrl);
    }
    this.lightboxOpen = false;
    this.selectedImage = null;
    this.lightboxUrl = null;
  }

  // ---------------------------------------------------------------------------
  // A/B comparison (issue #517)
  // ---------------------------------------------------------------------------

  /**
   * Resolve a displayable URL for an image, mirroring openLightbox's source
   * preference: the backend-served gallery file when it is browser-decodable,
   * the session blob URL otherwise, and an on-demand decode (JXL is transcoded
   * to WebP) for persisted files without a blob. `owned` marks URLs created
   * here — the only ones safe to revoke.
   */
  private async resolveCompareUrl(
    image: OutputImage,
  ): Promise<{ url: string | null; owned: boolean }> {
    const galleryFilename = await this.resolveGalleryFilename(image);
    const isJxl = galleryFilename?.endsWith(".jxl") ?? false;
    if (galleryFilename && !isJxl) {
      return { url: await fullImageUrl(galleryFilename), owned: false };
    }
    // Session images (and persisted JXL that already has a WebP blob) display
    // straight from the blob URL, which the session owns — never revoke it here.
    if (image.url) return { url: image.url, owned: false };
    if (galleryFilename) {
      try {
        return { url: await this.loadFullImage(galleryFilename), owned: true };
      } catch (e) {
        console.error("Failed to load full image for compare:", e);
        return { url: null, owned: false };
      }
    }
    return { url: image.fullImageUrl ?? null, owned: false };
  }

  private releaseCompareUrl(url: string | null) {
    if (!url || !this._compareOwnedUrls.has(url)) return;
    this._compareOwnedUrls.delete(url);
    if (url !== this.lightboxUrl) URL.revokeObjectURL(url);
  }

  /** Open the comparison viewer on two images. */
  async openCompare(a: OutputImage, b: OutputImage) {
    const token = ++this._compareToken;
    this.comparePin = null;
    this.releaseCompareUrl(this.compareUrlA);
    this.releaseCompareUrl(this.compareUrlB);
    this.compareUrlA = null;
    this.compareUrlB = null;
    this.compareA = a;
    this.compareB = b;
    this.compareOpen = true;
    this.compareLoading = true;
    try {
      const [resolvedA, resolvedB] = await Promise.all([
        this.resolveCompareUrl(a),
        this.resolveCompareUrl(b),
      ]);
      if (token !== this._compareToken) {
        // Superseded while resolving — drop anything we created.
        for (const r of [resolvedA, resolvedB]) {
          if (r.owned && r.url) URL.revokeObjectURL(r.url);
        }
        return;
      }
      if (resolvedA.owned && resolvedA.url) this._compareOwnedUrls.add(resolvedA.url);
      if (resolvedB.owned && resolvedB.url) this._compareOwnedUrls.add(resolvedB.url);
      this.compareUrlA = resolvedA.url;
      this.compareUrlB = resolvedB.url;
      if (!resolvedA.url || !resolvedB.url) {
        this.showToast(locale.t("gallery.compare.load_failed"), "error");
      }
    } finally {
      if (token === this._compareToken) this.compareLoading = false;
    }
  }

  /** Replace one side of the comparison, keeping the other in place. */
  async setCompareSide(side: "a" | "b", image: OutputImage) {
    if (!this.compareOpen) return;
    const token = ++this._compareToken;
    const previous = side === "a" ? this.compareUrlA : this.compareUrlB;
    if (side === "a") this.compareA = image;
    else this.compareB = image;
    this.compareLoading = true;
    try {
      const resolved = await this.resolveCompareUrl(image);
      if (token !== this._compareToken) {
        if (resolved.owned && resolved.url) URL.revokeObjectURL(resolved.url);
        return;
      }
      if (resolved.owned && resolved.url) this._compareOwnedUrls.add(resolved.url);
      this.releaseCompareUrl(previous);
      if (side === "a") this.compareUrlA = resolved.url;
      else this.compareUrlB = resolved.url;
      if (!resolved.url) this.showToast(locale.t("gallery.compare.load_failed"), "error");
    } finally {
      if (token === this._compareToken) this.compareLoading = false;
    }
  }

  /**
   * Images the comparison viewer can step through: persisted first, then
   * session-only ones. addImages() pushes the same object into both arrays,
   * so identity dedupe is enough.
   */
  private compareNavList(): OutputImage[] {
    const seen = new Set<OutputImage>();
    const list: OutputImage[] = [];
    for (const img of [...this.images, ...this.sessionImages]) {
      if (seen.has(img)) continue;
      seen.add(img);
      list.push(img);
    }
    return list;
  }

  /** Step one side of the comparison to the previous/next image (wraps). */
  async navigateCompare(side: "a" | "b", direction: "prev" | "next") {
    const list = this.compareNavList();
    if (list.length < 2) return;
    const current = side === "a" ? this.compareA : this.compareB;
    const index = current ? list.indexOf(current) : -1;
    if (index === -1) return;
    const nextIndex =
      direction === "next"
        ? (index + 1) % list.length
        : (index - 1 + list.length) % list.length;
    const next = list[nextIndex];
    if (next) await this.setCompareSide(side, next);
  }

  /** Swap which image is A and which is B (URLs move with them). */
  swapCompare() {
    const image = this.compareA;
    const url = this.compareUrlA;
    this.compareA = this.compareB;
    this.compareUrlA = this.compareUrlB;
    this.compareB = image;
    this.compareUrlB = url;
  }

  closeCompare() {
    this._compareToken++;
    this.releaseCompareUrl(this.compareUrlA);
    this.releaseCompareUrl(this.compareUrlB);
    this.compareOpen = false;
    this.compareLoading = false;
    this.compareA = null;
    this.compareB = null;
    this.compareUrlA = null;
    this.compareUrlB = null;
  }

  /**
   * One-button compare flow: the first image is pinned as side A, the second
   * opens the viewer. Re-clicking the pinned image cancels the pick.
   */
  toggleComparePin(image: OutputImage) {
    if (!this.comparePin) {
      this.comparePin = image;
      return;
    }
    if (this.comparePin === image) {
      this.comparePin = null;
      return;
    }
    void this.openCompare(this.comparePin, image);
  }

  cancelComparePin() {
    this.comparePin = null;
  }

  showToast(
    message: string,
    type: ToastType = "info",
    options: boolean | ToastOptions = false,
  ) {
    const toastOptions = typeof options === "boolean" ? { persistent: options } : options;
    this.toast = { message, type, ...toastOptions };
    if (this._toastTimer) clearTimeout(this._toastTimer);
    if (!toastOptions.persistent) {
      this._toastTimer = setTimeout(() => {
        this.toast = null;
        this._toastTimer = null;
      }, toastOptions.durationMs ?? 2000);
    } else {
      this._toastTimer = null;
    }
  }

  clearToast() {
    if (this._toastTimer) clearTimeout(this._toastTimer);
    this._toastTimer = null;
    this.toast = null;
  }

  /** Save generated images to the persistent gallery on disk.
   *  Skipped when manualSaveMode is on — user saves manually via saveImageToDir.
   *  If blobs are provided (from WebSocket delivery), use the bytes-based API
   *  to avoid a round-trip to ComfyUI's output directory.
   *  In browser mode, tempFilenames are used to save directly from server temp storage. */
  async persistImages(
    images: OutputImage[],
    metadata?: Record<string, string>,
    blobs?: Blob[],
    metadataMode?: string,
    tempFilenames?: (string | undefined)[],
  ) {
    if (generation.manualSaveMode) return;

    // Create per-image persist promises up-front so copyToClipboard / openLightbox
    // can await them even if this loop hasn't reached the image yet.
    const resolvers: Array<(gf: string) => void> = [];
    for (const img of images) {
      let resolve!: (gf: string) => void;
      const promise = new Promise<string>((r) => { resolve = r; });
      this._persistPromises.set(this._imageKey(img), promise);
      resolvers.push(resolve);
    }

    for (let i = 0; i < images.length; i++) {
      const img = images[i]!;
      try {
        let galleryFilename: string;
        const blob = blobs?.[i];
        const tempFilename = tempFilenames?.[i];
        // Prefer temp-file save when available — avoids serialising multi-MB
        // images as JSON number arrays through the IPC bridge.
        if (tempFilename) {
          try {
            galleryFilename = await saveToGalleryTemp(
              tempFilename,
              img.filename,
              img.prompt_id,
              img.generation_mode,
              metadata,
              metadataMode,
            );
          } catch (tempError) {
            if (!blob) throw tempError;
            console.warn("[persistImages] temp save failed; falling back to in-memory blob:", tempError);
            const bytes = await blobToBytes(blob);
            galleryFilename = await saveToGalleryBytes(
              bytes,
              img.filename,
              img.prompt_id,
              img.generation_mode,
              metadata,
              metadataMode,
            );
          }
        } else if (blob) {
          const bytes = await blobToBytes(blob);
          console.log("[persistImages] saveToGalleryBytes — filename:", img.filename, "blobType:", blob.type, "bytes:", bytes.length);
          galleryFilename = await saveToGalleryBytes(
            bytes,
            img.filename,
            img.prompt_id,
            img.generation_mode,
            metadata,
            metadataMode,
          );
          console.log("[persistImages] saved → galleryFilename:", galleryFilename);
        } else {
          galleryFilename = await saveToGallery(
            img.filename,
            img.subfolder,
            img.prompt_id,
            img.generation_mode,
            metadata,
            metadataMode,
          );
        }
        img.gallery_filename = galleryFilename;
        img.thumbnailUrl = await thumbnailUrl(galleryFilename);
        img.fullImageUrl = await fullImageUrl(galleryFilename);
        img.tempFilename = undefined;
        img.displayTempFilename = undefined;
        resolvers[i]!(galleryFilename);
      } catch (e) {
        console.error("Failed to save image to gallery:", e);
        resolvers[i]!("");
      } finally {
        this._persistPromises.delete(this._imageKey(img));
      }
    }
    // Trigger reactivity so components re-render with newly assigned thumbnailUrls
    this.sessionImages = [...this.sessionImages];
    // Refresh storage info after saving images
    if (isBrowserMode) {
      this.refreshStorageInfo();
    }
  }

  /** Load previously saved gallery images from disk on startup (metadata only — no image bytes). */
  async loadFromDisk() {
    this.loading = true;
    try {
      const entries = await listGalleryImageEntries();
      const loaded: OutputImage[] = [];
      for (const entry of entries) {
        const filename = entry.filename;
        try {
          // New format: {promptId}__{mode}__{origFilename}; legacy: {promptId}_{origFilename}
          let promptId = "";
          let origFilename = filename;
          let generationMode: GenerationMode | undefined;
          let isUpscaled = false;
          const modernParts = filename.split("__");
          if (modernParts.length >= 3) {
            promptId = modernParts[0] ?? "";
            generationMode = parseGenerationMode(modernParts[1] ?? "");
            origFilename = modernParts.slice(2).join("__");
            if (generationMode === "img2img") {
              const lowered = origFilename.toLowerCase();
              isUpscaled = lowered.includes("upscale") || lowered.includes("upscaled");
            }
          } else {
            const underscoreIdx = filename.indexOf("_");
            promptId = underscoreIdx > 0 ? filename.substring(0, underscoreIdx) : "";
            origFilename = underscoreIdx > 0 ? filename.substring(underscoreIdx + 1) : filename;
            const lowered = origFilename.toLowerCase();
            isUpscaled = lowered.includes("upscale") || lowered.includes("upscaled");
          }

          loaded.push({
            filename: origFilename,
            subfolder: "",
            type: "output",
            prompt_id: promptId,
            generation_mode: generationMode,
            is_upscaled: isUpscaled,
            url: undefined,
            thumbnailUrl: await thumbnailUrl(filename),
            fullImageUrl: await fullImageUrl(filename),
            gallery_filename: filename,
            file_size_bytes: entry.size_bytes,
            generated_at_ms: entry.modified_ms,
            duration_seconds: entry.duration_seconds,
            fps: entry.fps,
          });
        } catch (e) {
          console.error(`Failed to parse gallery entry ${filename}:`, e);
        }
      }
      if (loaded.length > 0) {
        this.images = [...loaded, ...this.images];
      }
    } catch (e) {
      console.error("Failed to list gallery images:", e);
    } finally {
      this.loading = false;
    }
    // Fetch storage info after loading gallery (browser mode)
    if (isBrowserMode) {
      this.refreshStorageInfo();
    }
    // Background: populate metadata for thumbnails so artist-tag detection
    // and other prompt-based UI work without needing the lightbox.
    void this.hydrateMetadataInBackground();
  }

  /** Null until hydration starts; resolves when ALL pending images have metadata. */
  private _metadataHydrationPromise: Promise<void> | null = null;

  /**
   * Gallery filenames that returned no metadata. Without this, anything the
   * backend can't find metadata for stays in `pending` forever and is re-read
   * in full on every gallery load — cheap for a stray PNG, expensive for a
   * multi-megabyte clip saved before videos carried metadata at all.
   * Session-scoped: metadata is written at save time, so a file that has none
   * now will not grow any while the app is running.
   */
  private _metadataMisses = new Set<string>();

  /**
   * Walk the gallery and lazily read embedded metadata for any entry that
   * doesn't have it yet.  Runs in small batches with yields so the UI stays
   * smooth. Safe to call more than once — the second caller awaits the first run.
   */
  async hydrateMetadataInBackground(): Promise<void> {
    if (this._metadataHydrationPromise) return this._metadataHydrationPromise;
    // Reset on completion so images added after this run can be hydrated by a
    // later call. Without the reset the memoized promise stays resolved forever
    // and newly inserted images never get their metadata.
    this._metadataHydrationPromise = this._runMetadataHydration().finally(() => {
      this._metadataHydrationPromise = null;
    });
    return this._metadataHydrationPromise;
  }

  private async _runMetadataHydration(): Promise<void> {
    // Capture object references (not indices) so that later insertions into
    // this.images cannot cause us to skip or mis-target images.
    const pending = this.images.filter(
      (img) =>
        img && !img.metadata && img.gallery_filename && !this._metadataMisses.has(img.gallery_filename),
    );
    // Videos go last and in smaller batches: the backend reads the whole file
    // to find the metadata box, so a batch of clips moves far more bytes than
    // a batch of stills. Images first means artist badges appear promptly
    // instead of queueing behind the expensive reads.
    const stills = pending.filter((img) => !isVideoImage(img));
    const videos = pending.filter((img) => isVideoImage(img));
    console.debug(
      `[artist] Hydrating metadata for ${stills.length} images + ${videos.length} videos / ${this.images.length} entries`,
    );
    const hydrated = (await this._hydrateBatched(stills, 12)) + (await this._hydrateBatched(videos, 3));
    console.debug(`[artist] Hydration complete: ${hydrated} entries loaded`);
  }

  /** Read metadata for `pending` in slices of `batch`, yielding between them. Returns the hydrated count. */
  private async _hydrateBatched(pending: OutputImage[], batch: number): Promise<number> {
    let hydrated = 0;
    for (let b = 0; b < pending.length; b += batch) {
      const slice = pending.slice(b, b + batch);
      await Promise.all(
        slice.map(async (img) => {
          if (!img || img.metadata || !img.gallery_filename) return;
          try {
            const meta = await readImageMetadata(img.gallery_filename);
            if (!meta) {
              console.debug(`[artist] readImageMetadata returned null for ${img.gallery_filename}`);
              this._metadataMisses.add(img.gallery_filename);
              return;
            }
            if (!img.metadata) {
              // Direct property mutation — same pattern as App.svelte's lightbox
              // load path; Svelte 5's deep proxy tracks this correctly.
              img.metadata = meta;
              hydrated++;
            }
          } catch (e) {
            // Non-fatal — just means this entry won't get artist badges.
            console.debug("[artist] Metadata hydration failed for", img.gallery_filename, e);
            this._metadataMisses.add(img.gallery_filename);
          }
        }),
      );
      // Yield to keep the UI responsive between batches
      await new Promise((r) => setTimeout(r, 0));
    }
    return hydrated;
  }

  /** Load full-resolution image data on demand. Returns the blob URL. */
  async loadFullImage(galleryFilename: string): Promise<string> {
    // mp4: hand back the streaming URL. Decoding into a blob would defeat
    // Range requests and pull the whole clip into memory.
    if (galleryFilename.toLowerCase().endsWith(".mp4")) {
      return fullImageUrl(galleryFilename);
    }
    // Use the display variant: JXL files are transcoded to WebP server-side
    // since WebView2 cannot natively decode JXL in <img> tags.
    const bytes = await loadGalleryImageDisplay(galleryFilename);
    const ext = galleryFilename.split(".").pop()?.toLowerCase() ?? "png";
    const mimeType =
      ext === "jxl"
        ? "image/webp"  // JXL was transcoded to WebP by loadGalleryImageDisplay
        : ext === "jpg" || ext === "jpeg"
          ? "image/jpeg"
          : ext === "webp"
            ? "image/webp"
            : "image/png";
    const blob = new Blob([new Uint8Array(bytes)], { type: mimeType });
    return URL.createObjectURL(blob);
  }

  /**
   * Resolve an image's pixels to PNG bytes from whichever source is available:
   * a persisted gallery file, the in-memory session blob, a browser-mode temp
   * file, an object URL, or the ComfyUI output endpoint. Lets just-generated
   * (not-yet-persisted) session images be saved/copied/interrogated without a
   * 404 round-trip to ComfyUI for a filename that only exists in the gallery DB.
   */
  async resolveImagePngBytes(image: OutputImage): Promise<number[]> {
    let bytes: number[] | null = null;
    const isJxlGallery = image.gallery_filename?.endsWith(".jxl") ?? false;
    if (image.gallery_filename) {
      // JXL and WebP are transcoded server-side to PNG — universally compatible,
      // and the Rust path re-embeds the metadata losslessly instead of round-tripping
      // the pixels through a canvas (which would destroy stealth-alpha bits).
      bytes =
        isJxlGallery || isWebpGalleryFile(image.gallery_filename)
          ? await loadGalleryImagePng(image.gallery_filename)
          : await loadGalleryImage(image.gallery_filename);
    } else if (image.sessionBlob && image.sessionBlob.type !== "image/jxl") {
      bytes = await this._blobToPngBytes(image.sessionBlob);
    } else if (isBrowserMode && (image.displayTempFilename || image.tempFilename)) {
      // Browser-mode JXL needs the pre-built display copy. If the temp file
      // has expired, fall back to the session blob/display URL already held by the client.
      const fetchFilename = image.displayTempFilename ?? image.tempFilename!;
      try {
        bytes = await this._tempImageToPngBytes(fetchFilename, fetchFilename);
      } catch (tempError) {
        console.warn("Temp image fetch failed; falling back to session image:", tempError);
        if (image.url) {
          bytes = await this._fetchUrlToPngBytes(image.url);
        } else if (image.sessionBlob) {
          bytes = await this._blobToPngBytes(image.sessionBlob);
        } else {
          throw tempError;
        }
      }
    } else if (image.url) {
      bytes = await this._fetchUrlToPngBytes(image.url);
    } else {
      bytes = await getOutputImage(image.filename, image.subfolder);
    }
    if (!bytes) throw new Error(locale.t("gallery.error.image_bytes_unavailable"));
    return this._ensurePngBytes(bytes);
  }

  /**
   * Raw WebP bytes for an image whose canonical form is WebP, or `null` when it
   * is not a WebP image. WebP is exported natively rather than transcoded: it is
   * universally readable, and re-encoding would strip the stealth-alpha bits.
   */
  private async _resolveWebpBytes(image: OutputImage): Promise<number[] | null> {
    if (image.gallery_filename && isWebpGalleryFile(image.gallery_filename)) {
      // `loadGalleryImageDisplay` returns WebP files verbatim (no transcode).
      return await loadGalleryImageDisplay(image.gallery_filename);
    }
    if (image.sessionBlob?.type === "image/webp") {
      return await blobToBytes(image.sessionBlob);
    }
    return null;
  }

  /** Save an image to a user-chosen location via native file dialog (or browser download). */
  async saveImageAs(image: OutputImage) {
    if (isVideoImage(image)) {
      await this.saveVideoAs(image);
      return;
    }
    if (this.saving) return;
    this.saving = true;
    try {
      const webpBytes = await this._resolveWebpBytes(image);
      let bytes = webpBytes ?? (await this.resolveImagePngBytes(image));
      const isWebp = webpBytes !== null;
      if (image.metadata) {
        try {
          bytes = await embedImageMetadataBytes(bytes, image.metadata, generation.metadataMode);
        } catch (e) {
          console.warn("Failed to embed metadata into export:", e);
        }
      }

      const defaultFilename = isWebp
        ? webpNormalizedFilename(image.filename)
        : pngNormalizedFilename(image.filename);
      const path = await showSaveDialog(
        defaultFilename,
        isWebp ? WEBP_SAVE_EXTENSIONS : PNG_SAVE_EXTENSIONS
      );
      if (path) {
        await saveImageFile(bytes, path);
      } else {
        triggerBrowserDownload(
          new Uint8Array(bytes),
          defaultFilename,
          isWebp ? "image/webp" : "image/png"
        );
      }
      this.showToast(locale.t("gallery.toast.image_saved"), "success");
    } catch (e) {
      console.error("Failed to save image:", e);
    } finally {
      this.saving = false;
    }
  }

  /**
   * Save a gallery video to a user-chosen location.
   *
   * Video bytes are never marshalled through IPC: Tauri v2 serializes
   * `Vec<u8>` as a JSON number array, so a few hundred MB of mp4 would balloon
   * into gigabytes of JSON. On desktop Rust copies the file; in browser mode
   * the anchor downloads straight from the streaming endpoint, so the bytes
   * never pass through JS either.
   */
  async saveVideoAs(image: OutputImage) {
    if (this.saving) return;
    this.saving = true;
    try {
      const videoFilename = await this.resolveGalleryFilename(image);
      if (!videoFilename) {
        this.showToast(locale.t("gallery.toast.not_saved_yet"), "error");
        return;
      }
      if (isTauri) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const path = await save({
          defaultPath: videoFilename,
          filters: [{ name: "Videos", extensions: MP4_SAVE_EXTENSIONS }],
        });
        if (!path) return;
        await copyGalleryFileTo(videoFilename, path);
      } else {
        const url = await fullImageUrl(videoFilename);
        const a = document.createElement("a");
        a.href = url;
        a.download = videoFilename;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
      }
      this.showToast(locale.t("gallery.toast.video_saved"), "success");
    } catch (e) {
      console.error("Failed to save video:", e);
      this.showToast(locale.t("gallery.toast.failed_save_video"), "error");
    } finally {
      this.saving = false;
    }
  }

  /** Save a blob URL image to a user-chosen location (or browser download). */
  async saveBlobAs(blobUrl: string, defaultName: string = "image.png") {
    if (this.saving) return;
    this.saving = true;
    try {
      let saveBytes: number[];
      let blob: Blob | null = null;
      try {
        const response = await fetch(blobUrl);
        if (!response.ok) throw new Error(`Failed to fetch image: ${response.status}`);
        blob = await response.blob();
      } catch {
        blob = null;
      }

      if (!blob && blobUrl.startsWith("blob:")) {
        saveBytes = await this._blobUrlToPngBytes(blobUrl);
      } else if (blob) {
        saveBytes = await this._blobToPngBytes(blob);
      } else {
        throw new Error(locale.t("gallery.error.image_url_unavailable"));
      }

      // Normalise the default filename extension — always saving as PNG.
      const pngName = pngNormalizedFilename(defaultName);
      const path = await showSaveDialog(pngName, PNG_SAVE_EXTENSIONS);
      if (path) {
        await saveImageFile(saveBytes, path);
      } else {
        triggerBrowserDownload(new Uint8Array(saveBytes), pngName, "image/png");
      }
      this.showToast(locale.t("gallery.toast.image_saved"), "success");
    } catch (e) {
      console.error("Failed to save image:", e);
    } finally {
      this.saving = false;
    }
  }

  /** Save an image directly to a specific directory (manual save mode). Embeds metadata. */
  async saveImageToDir(image: OutputImage, dir: string) {
    try {
      const webpBytes = await this._resolveWebpBytes(image);
      let bytes = webpBytes ?? (await this.resolveImagePngBytes(image));
      const rawName = image.filename || `image_${Date.now()}.png`;
      const filename =
        webpBytes !== null ? webpNormalizedFilename(rawName) : pngNormalizedFilename(rawName);
      if (image.metadata) {
        bytes = await embedImageMetadataBytes(bytes, image.metadata, generation.metadataMode);
      }
      await saveImageFile(bytes, `${dir}/${filename}`);
      this.showToast(locale.t("gallery.toast.image_saved"), "success");
    } catch (e) {
      console.error("Failed to save image to directory:", e);
      this.showToast(locale.t("gallery.toast.failed_save"), "error");
    }
  }

  /** Copy a gallery image file to clipboard (as file reference). */
  async copyToClipboard(image: OutputImage) {
    if (isVideoImage(image)) {
      await this.copyVideoToClipboard(image);
      return;
    }
    this.showToast(locale.t("gallery.toast.copying"), "info", true);
    try {
      if (isBrowserMode) {
        // Browser mode: prefer gallery file (has metadata embedded).
        // If persist hasn't finished yet, wait for it (shows "Copying…" meanwhile).
        let galleryFilename = image.gallery_filename;
        if (!galleryFilename) {
          const key = this._imageKey(image);
          const pending = this._persistPromises.get(key);
          if (pending) {
            galleryFilename = await pending;
          }
        }
        // Step 1: Try server-side native clipboard (preserves full PNG with metadata).
        // May fail on headless servers without xclip/wl-copy — fall through to browser API.
        if (galleryFilename) {
          // JXL: the server-side clipboard handler can't paste raw JXL as an
          // image, and the standard fetch path serves WebP (no metadata).
          // Explicitly transcode to PNG + re-embed metadata so paste targets
          // get a usable, metadata-bearing image.
          if (galleryFilename.endsWith(".jxl")) {
            let pngBytes: number[] | null = null;
            try {
              pngBytes = await loadGalleryImagePng(galleryFilename);
            } catch (e) {
              console.warn("JXL gallery transcode failed, falling back to display copy:", e);
              const displayUrl = image.displayTempFilename
                ? tempImageUrl(image.displayTempFilename)
                : (image.url || (this.selectedImage === image ? this.lightboxUrl : null));
              if (displayUrl) {
                try {
                  pngBytes = await this._blobUrlToPngBytes(displayUrl);
                } catch (e2) {
                  console.warn("JXL display copy fallback also failed:", e2);
                }
              }
            }
            if (pngBytes) {
              if (image.metadata) {
                try {
                  pngBytes = await embedImageMetadataBytes(pngBytes, image.metadata, generation.metadataMode);
                } catch (e) {
                  console.warn("Failed to embed PNG metadata into JXL clipboard copy:", e);
                }
              }
              await this.writeBlobToClipboard(pngBlobFromBytes(pngBytes));
              this.showToast(locale.t("gallery.toast.copied"), "success");
              return;
            }
          }
          try {
            const path = await getGalleryImagePath(galleryFilename);
            await copyImageToClipboard(path);
            this.showToast(locale.t("gallery.toast.copied"), "success");
            return;
          } catch {
            // Server-side clipboard unavailable — fall through to browser API
          }
        }
        // Step 2: Try browser Clipboard API, with server-side fallback for insecure (HTTP) contexts.
        // Prefer the server-served gallery URL over blob URLs (blob: URLs fail through Cloudflare proxy).
        let fetchUrl = image.fullImageUrl;
        if (!fetchUrl && galleryFilename) {
          fetchUrl = await fullImageUrl(galleryFilename);
        }
        if (!fetchUrl)
          fetchUrl = (image.url || (this.selectedImage === image ? this.lightboxUrl : null)) ?? undefined;
        if (fetchUrl) {
          try {
            const resp = await fetch(fetchUrl);
            if (!resp.ok) {
              this.showToast(locale.t("gallery.toast.copy_failed"), "error");
              return;
            }
            const blob = await resp.blob();
            const pngBlob = blob.type.startsWith("image/") ? blob : new Blob([blob], { type: "image/png" });
            await this.writeBlobToClipboard(pngBlob);
            this.showToast(locale.t("gallery.toast.copied"), "success");
            return;
          } catch {
            // fetch on blob: URLs can be blocked by CSP (e.g. Cloudflare proxy).
            // Fall back to <img> + canvas approach for blob URLs.
            if (fetchUrl.startsWith("blob:")) {
              try {
                const bytes = await this._blobUrlToPngBytes(fetchUrl);
                const pngBlob = new Blob([new Uint8Array(bytes)], { type: "image/png" });
                await this.writeBlobToClipboard(pngBlob);
                this.showToast(locale.t("gallery.toast.copied"), "success");
                return;
              } catch {
                // Canvas fallback also failed — fall through
              }
            }
          }
        }
        // Step 3: Image genuinely not available yet (no URL or gallery file).
        this.showToast(locale.t("gallery.toast.not_saved_yet"), "info");
        return;
      }
      // Tauri mode: prefer native clipboard
      if (image.gallery_filename) {
        if (image.gallery_filename.endsWith(".jxl")) {
          // JXL can't be pasted as an image from the raw file path — the
          // backend transcodes to PNG (with metadata embedded) and puts it on
          // the clipboard in one call, so no image bytes cross IPC.
          await copyGalleryImageToClipboard(
            image.gallery_filename,
            image.metadata ?? undefined,
            generation.metadataMode,
          );
        } else {
          const path = await getGalleryImagePath(image.gallery_filename);
          await copyImageToClipboard(path);
        }
      } else if (image.url) {
        await this.copyBlobToClipboard(image.url, image.metadata ?? undefined);
        return;
      } else {
        this.showToast(locale.t("gallery.toast.not_saved_yet"), "info");
        return;
      }
      this.showToast(locale.t("gallery.toast.copied"), "success");
    } catch (e) {
      console.error("Failed to copy to clipboard:", e);
      this.showToast(locale.t("gallery.toast.failed_copy"), "error");
    }
  }

  /**
   * Copy a gallery video to the clipboard as a file reference.
   *
   * Nothing here decodes or re-encodes: an mp4 goes on the clipboard the same
   * way a file manager puts one there, so pasting into a chat client or a
   * folder yields the original file, audio included.
   *
   * Browser mode has no equivalent - the clipboard lives on the server, not on
   * the machine looking at the page - so it keeps the unsupported toast.
   */
  private async copyVideoToClipboard(image: OutputImage) {
    if (isBrowserMode) {
      this.showToast(locale.t("gallery.toast.copy_video_unsupported"), "error");
      return;
    }
    this.showToast(locale.t("gallery.toast.copying"), "info", true);
    try {
      const videoFilename = await this.resolveGalleryFilename(image);
      if (!videoFilename) {
        this.showToast(locale.t("gallery.toast.not_saved_yet"), "info");
        return;
      }
      const path = await getGalleryImagePath(videoFilename);
      await copyFileToClipboard(path);
      this.showToast(locale.t("gallery.toast.copied"), "success");
    } catch (e) {
      console.error("Failed to copy video to clipboard:", e);
      this.showToast(locale.t("gallery.toast.failed_copy"), "error");
    }
  }

  /** Write an image blob to the clipboard, with fallback for non-secure contexts (HTTP LAN). */
  private async writeBlobToClipboard(blob: Blob) {
    // navigator.clipboard.write requires a secure context (HTTPS or localhost).
    if (navigator.clipboard?.write) {
      try {
        await navigator.clipboard.write([new ClipboardItem({ [blob.type]: blob })]);
        return;
      } catch {
        // Clipboard API blocked — fall through to server fallback
      }
    }
    // Browser Clipboard API unavailable (HTTP context) — route through backend
    // which uses native OS clipboard tools (xclip/wl-copy/pbcopy/PowerShell).
    if (isBrowserMode) {
      const buf = await blob.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buf));
      const ext = blob.type === "image/jpeg" ? "jpg"
        : blob.type === "image/webp" ? "webp"
        : "png";
      await copyBytesToClipboard(bytes, ext);
      return;
    }
    throw new Error(locale.t("common.clipboard_unavailable"));
  }

  /** Copy a blob URL image to clipboard via native Tauri clipboard or browser Clipboard API. */
  async copyBlobToClipboard(blobUrl: string, metadata?: Record<string, string>) {
    this.showToast(locale.t("gallery.toast.copying"), "info", true);
    try {
      let bytes: number[];
      let mimeType = "image/png";
      try {
        const response = await fetch(blobUrl);
        if (!response.ok) throw new Error(`Failed to fetch image: ${response.status}`);
        const blob = await response.blob();
        mimeType = blob.type || "image/png";
        const arrayBuf = await blob.arrayBuffer();
        bytes = Array.from(new Uint8Array(arrayBuf));
      } catch {
        // fetch on blob: URLs can be blocked by CSP (e.g. Cloudflare proxy).
        // Fall back to drawing through <img> + canvas to extract PNG bytes.
        bytes = await this._blobUrlToPngBytes(blobUrl);
        mimeType = "image/png";
      }

      // Always export as PNG — convert WebP blobs (JXL display copies) via canvas
      // so metadata embedding works and the image pastes correctly in all apps.
      if (mimeType !== "image/png") {
        bytes = await this._blobUrlToPngBytes(blobUrl);
        mimeType = "image/png";
      }

      if (metadata) {
        bytes = await embedImageMetadataBytes(bytes, metadata);
      }

      if (isBrowserMode) {
        const pngBlob = new Blob([new Uint8Array(bytes)], { type: "image/png" });
        await this.writeBlobToClipboard(pngBlob);
      } else {
        const ext = mimeType === "image/jpeg" ? "jpg"
          : mimeType === "image/webp" ? "webp"
          : "png";
        await copyBytesToClipboard(bytes, ext);
      }
      this.showToast(locale.t("gallery.toast.copied"), "success");
    } catch (e) {
      console.error("Failed to copy blob to clipboard:", e);
      this.showToast(locale.t("gallery.toast.failed_copy"), "error");
    }
  }

  /** Convert a blob URL to PNG bytes via <img> + canvas (CSP-safe). */
  private _blobUrlToPngBytes(blobUrl: string): Promise<number[]> {
    return new Promise((resolve, reject) => {
      const img = new Image();
      img.onload = () => {
        const canvas = document.createElement("canvas");
        canvas.width = img.naturalWidth;
        canvas.height = img.naturalHeight;
        const ctx = canvas.getContext("2d");
        if (!ctx) { reject(new Error("Canvas 2D context unavailable")); return; }
        ctx.drawImage(img, 0, 0);
        canvas.toBlob((blob) => {
          if (!blob) { reject(new Error("Canvas toBlob failed")); return; }
          blob.arrayBuffer().then(
            (buf) => resolve(Array.from(new Uint8Array(buf))),
            reject,
          );
        }, "image/png");
      };
      img.onerror = () => reject(new Error("Failed to load blob URL as image"));
      img.src = blobUrl;
    });
  }

  /** Fetch a URL and return PNG bytes (canvas-converts WebP/JPEG/mislabeled blobs). */
  private async _fetchUrlToPngBytes(url: string): Promise<number[]> {
    try {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`Failed to fetch image: ${response.status}`);
      return this._blobToPngBytes(await response.blob());
    } catch {
      return this._blobUrlToPngBytes(url);
    }
  }

  /** Ensure bytes are PNG before metadata embed (re-encode JPEG/WebP/mislabeled PNG). */
  private async _ensurePngBytes(bytes: number[]): Promise<number[]> {
    if (isPngBytes(bytes)) return bytes;
    const blob = new Blob([new Uint8Array(bytes)]);
    return this._blobToPngBytes(blob);
  }

  private async _blobToPngBytes(blob: Blob): Promise<number[]> {
    if (blob.type === "image/png") {
      const bytes = await blobToBytes(blob);
      if (isPngBytes(bytes)) return bytes;
    }
    const url = URL.createObjectURL(blob);
    try {
      return await this._blobUrlToPngBytes(url);
    } finally {
      URL.revokeObjectURL(url);
    }
  }

  private async _tempImageToPngBytes(tempFilename: string, outputFilename: string): Promise<number[]> {
    const isJxl = tempFilename.toLowerCase().endsWith(".jxl") || outputFilename.toLowerCase().endsWith(".jxl");
    const resp = await fetch(tempImageUrl(tempFilename, isJxl ? { format: "webp" } : undefined));
    if (!resp.ok) throw new Error(`Temp image fetch failed: ${resp.status}`);
    const blob = await resp.blob();
    return this._blobToPngBytes(blob);
  }

  /** Delete an image from the gallery. */
  async deleteImage(image: OutputImage) {
    try {
      if (image.gallery_filename) {
        await deleteGalleryImage(image.gallery_filename);
        const nextAssignments = { ...this.boardAssignments };
        delete nextAssignments[image.gallery_filename];
        this.boardAssignments = nextAssignments;
        this.saveBoardAssignments();
      }
      if (image.url) {
        URL.revokeObjectURL(image.url);
      }
      this.images = this.images.filter((i) => i !== image);
      this.sessionImages = this.sessionImages.filter((i) => i !== image);
      if (this.selectedImage === image) {
        this.closeLightbox();
      }
      if (this.lastSelectedImage === image) {
        this.lastSelectedImage = null;
      }
    } catch (e) {
      console.error("Failed to delete image:", e);
    }
  }

  /**
   * Delete every image generated during this session — removes each from disk
   * (and the persistent gallery), revokes its blob URL, and clears the session
   * list. Mirrors deleteImage() applied to all session images at once.
   *
   * Pass `only` to restrict the sweep to a subset — video mode uses it so
   * "Delete all" clears the session's videos without touching its images.
   */
  async deleteAllSessionImages(only?: (image: OutputImage) => boolean) {
    const targets = new Set(only ? this.sessionImages.filter(only) : this.sessionImages);
    if (targets.size === 0) return;

    const nextAssignments = { ...this.boardAssignments };
    let assignmentsChanged = false;
    for (const image of targets) {
      try {
        if (image.gallery_filename) {
          await deleteGalleryImage(image.gallery_filename);
          if (nextAssignments[image.gallery_filename] !== undefined) {
            delete nextAssignments[image.gallery_filename];
            assignmentsChanged = true;
          }
        }
        if (image.url) {
          URL.revokeObjectURL(image.url);
        }
      } catch (e) {
        console.error("Failed to delete session image:", e);
      }
    }

    if (assignmentsChanged) {
      this.boardAssignments = nextAssignments;
      this.saveBoardAssignments();
    }
    this.images = this.images.filter((i) => !targets.has(i));
    this.sessionImages = this.sessionImages.filter((i) => !targets.has(i));
    if (this.selectedImage && targets.has(this.selectedImage)) {
      this.closeLightbox();
    }
    if (this.lastSelectedImage && targets.has(this.lastSelectedImage)) {
      this.lastSelectedImage = null;
    }
  }

  private inferModeFromFilename(image: OutputImage): GenerationMode {
    const n = `${image.filename} ${image.gallery_filename ?? ""}`.toLowerCase();
    if (n.includes("image_edit")) return "image_edit";
    if (n.includes("inpaint") || n.includes("mask")) return "inpainting";
    if (n.includes("img2img") || n.includes("upscale")) return "img2img";
    return "txt2img";
  }

  /** Re-scan legacy gallery metadata and migrate old filenames to include mode metadata. */
  async rescanMetadata() {
    try {
      let migrated = 0;
      for (const image of this.images) {
        const current = image.gallery_filename;
        if (!current) continue;
        if (current.includes("__")) continue;

        const mode = image.generation_mode ?? this.inferModeFromFilename(image);
        const promptId = image.prompt_id || "unknown";
        const newFilename = `${promptId}__${mode}__${image.filename}`;

        try {
          const renamed = await renameGalleryImage(current, newFilename);
          image.gallery_filename = renamed;
          image.generation_mode = mode;
          migrated += 1;
        } catch (e) {
          // Keep scanning remaining files even if one rename fails.
          console.error(`Failed to migrate gallery filename ${current}:`, e);
        }
      }

      if (migrated > 0) {
        // Close lightbox before revoking blob URLs — the lightbox may be
        // displaying one of these blobs, which would cause ERR_FILE_NOT_FOUND.
        if (this.lightboxOpen) this.closeLightbox();
        for (const image of this.images) {
          if (image.url) URL.revokeObjectURL(image.url);
        }
        this.images = [];
        this.sessionImages = [];
        await this.loadFromDisk();
      }

      this.showToast(
        migrated > 0
          ? locale.t("gallery.toast.rescan_migrated", { count: String(migrated) })
          : locale.t("gallery.toast.rescan_none"),
        migrated > 0 ? "success" : "info"
      );
    } catch (e) {
      console.error("Failed to re-scan gallery metadata:", e);
      this.showToast(locale.t("gallery.toast.rescan_failed"), "error");
    }
  }

  // ---------------------------------------------------------------------------
  // Storage info & expiry
  // ---------------------------------------------------------------------------

  /** Whether images have an expiry policy (browser mode with limits). */
  get hasExpiry(): boolean {
    return !!this.storageInfo && this.storageInfo.expiry_secs > 0;
  }

  /** Storage usage as a percentage (0-100). 0 if unlimited. */
  get storagePercent(): number {
    if (!this.storageInfo || this.storageInfo.limit_bytes === 0) return 0;
    return Math.min(100, (this.storageInfo.usage_bytes / this.storageInfo.limit_bytes) * 100);
  }

  /** Human-readable storage usage string, e.g., "1.2 GB / 2.0 GB". */
  get storageLabel(): string {
    if (!this.storageInfo) return "";
    const fmt = (b: number) => locale.formatBytes(b);
    if (this.storageInfo.limit_bytes === 0) return fmt(this.storageInfo.usage_bytes);
    return `${fmt(this.storageInfo.usage_bytes)} / ${fmt(this.storageInfo.limit_bytes)}`;
  }

  /** Number of images expiring within 24 hours. */
  get expiringWithin24h(): number {
    if (!this.storageInfo) return 0;
    return this.storageInfo.images.filter(
      (img) => img.expires_in_secs > 0 && img.expires_in_secs <= 86400,
    ).length;
  }

  /** Fetch storage info from the server. Call after login and after saves. */
  async refreshStorageInfo() {
    try {
      this.storageInfo = await getStorageInfo();
    } catch (e) {
      console.error("Failed to fetch storage info:", e);
    }
  }

  /** Collect gallery board preferences for server-side sync. */
  collectPrefs(): unknown {
    return {
      boardAssignments: this.boardAssignments,
      customBoards: this.customBoards,
    };
  }

  /** Apply gallery board preferences fetched from server. */
  applyServerPrefs(data: any): void {
    try {
      if (data?.boardAssignments && typeof data.boardAssignments === "object") {
        this.boardAssignments = data.boardAssignments as Record<string, string>;
        this.saveBoardAssignments();
      }
      if (Array.isArray(data?.customBoards)) {
        this.customBoards = data.customBoards.filter(
          (name: unknown) => typeof name === "string" && !!name && name !== "Unsorted",
        );
        this.saveCustomBoards();
      }
    } catch (e) {
      console.error("gallery: applyServerPrefs failed", e);
    }
  }
}

export const gallery = new GalleryStore();
