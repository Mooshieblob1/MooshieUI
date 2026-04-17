import type { OutputImage } from "../types/index.js";
import {
  listGalleryImageEntries,
  loadGalleryImage,
  saveToGallery,
  saveToGalleryBytes,
  saveToGalleryTemp,
  deleteGalleryImage,
  renameGalleryImage,
  saveImageFile,
  embedPngMetadataBytes,
  getOutputImage,
  copyImageToClipboard,
  copyBytesToClipboard,
  getGalleryImagePath,
  getStorageInfo,
  readImageMetadata,
  type StorageInfo,
} from "../utils/api.js";
import { isTauri, isBrowserMode, getAuthToken } from "../utils/ipc.js";
import { locale } from "./locale.svelte.js";
import { generation } from "./generation.svelte.js";
import { createArtistGalleryClient } from "../artist-gallery/client.js";
import {
  buildArtistTagIndex,
  detectArtistsInPrompt,
  artistBoardName,
  type ArtistTagIndex,
} from "../artist-gallery/detection.js";
import type { ArtistSearchHit } from "../artist-gallery/types.js";

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
  const blob = new Blob([data], { type: mimeType });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

const GALLERY_BOARDS_KEY = "mooshieui.gallery.boards.v1";
const GALLERY_BOARD_NAMES_KEY = "mooshieui.gallery.boardNames.v1";

class GalleryStore {
  images = $state<OutputImage[]>([]);
  /** Images generated during this app session (not loaded from disk). */
  sessionImages = $state<OutputImage[]>([]);
  selectedImage = $state<OutputImage | null>(null);
  /** When set, the lightbox shows this URL instead of selectedImage. */
  lightboxUrl = $state<string | null>(null);
  lightboxOpen = $state(false);
  lightboxLoading = $state(false);
  loading = $state(false);
  /** True while a save/download operation is in progress (prevents double-clicks). */
  saving = $state(false);
  toast = $state<{ message: string; type: "success" | "error" | "info" } | null>(null);
  boardAssignments = $state<Record<string, string>>({});
  customBoards = $state<string[]>([]);
  /** Storage info from the server (browser mode only). */
  storageInfo = $state<StorageInfo | null>(null);
  /** Lookup map for artist-tag detection.  Populated by loadArtistIndex(). */
  artistTagIndex = $state<ArtistTagIndex>(new Map());
  /** True once the artist index has been fetched at least once. */
  artistIndexReady = $state(false);
  /** True while autoSortByArtist is running. */
  autoSorting = $state(false);
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

  constructor() {
    this.loadBoardAssignments();
    this.loadCustomBoards();
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
        const client = createArtistGalleryClient({ manifestUrl });
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
        this.showToast("Artist index unavailable — check your connection.", "error");
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
    this.lightboxOpen = true;
    if (image.fullImageUrl) {
      // Serve the real image from backend — supports right-click → Save with metadata
      this.lightboxUrl = image.fullImageUrl;
      this.lightboxLoading = false;
    } else if (image.url) {
      // Session images still have a blob URL (pre-persistence) — show it
      // immediately, then upgrade to the gallery URL once persist finishes
      // so right-click → Copy Image preserves stealth-alpha metadata.
      this.lightboxUrl = image.url;
      this.lightboxLoading = false;
      const key = this._imageKey(image);
      const pending = this._persistPromises.get(key);
      if (pending) {
        const galleryFilename = await pending;
        if (galleryFilename && this.lightboxOpen && this.selectedImage && this._imageKey(this.selectedImage) === key) {
          this.lightboxUrl = await fullImageUrl(galleryFilename);
        }
      }
    } else if (image.gallery_filename) {
      // Persisted images without a fullImageUrl — load full-res from disk
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

  /** Open lightbox with a raw image URL (e.g. preview blob). */
  openLightboxUrl(url: string) {
    this.selectedImage = null;
    this.lightboxUrl = url;
    this.lightboxOpen = true;
  }

  closeLightbox() {
    if (this.lightboxUrl?.startsWith("blob:")) URL.revokeObjectURL(this.lightboxUrl);
    this.lightboxOpen = false;
    this.selectedImage = null;
    this.lightboxUrl = null;
  }

  showToast(message: string, type: "success" | "error" | "info" = "info", persistent = false) {
    this.toast = { message, type };
    if (this._toastTimer) clearTimeout(this._toastTimer);
    if (!persistent) {
      this._toastTimer = setTimeout(() => {
        this.toast = null;
        this._toastTimer = null;
      }, 2000);
    } else {
      this._toastTimer = null;
    }
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
        // In browser mode, prefer temp-file save (zero extra data transfer —
        // the full-res image already lives on the server as a temp file).
        if (isBrowserMode && tempFilename) {
          galleryFilename = await saveToGalleryTemp(
            tempFilename,
            img.filename,
            img.prompt_id,
            img.generation_mode,
            metadata,
            metadataMode,
          );
        } else if (blob && !isBrowserMode) {
          const buf = await blob.arrayBuffer();
          const bytes = Array.from(new Uint8Array(buf));
          galleryFilename = await saveToGalleryBytes(
            bytes,
            img.filename,
            img.prompt_id,
            img.generation_mode,
            metadata,
            metadataMode,
          );
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
          let generationMode: "txt2img" | "img2img" | "inpainting" | undefined;
          let isUpscaled = false;
          const modernParts = filename.split("__");
          if (modernParts.length >= 3) {
            promptId = modernParts[0] ?? "";
            const mode = modernParts[1] ?? "";
            if (mode === "txt2img" || mode === "img2img" || mode === "inpainting") {
              generationMode = mode;
            }
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
   * Walk the gallery and lazily read PNG metadata for any image that doesn't
   * have it yet.  Runs in small batches with yields so the UI stays smooth.
   * Safe to call more than once — the second caller awaits the first run.
   */
  async hydrateMetadataInBackground(): Promise<void> {
    if (this._metadataHydrationPromise) return this._metadataHydrationPromise;
    this._metadataHydrationPromise = this._runMetadataHydration();
    return this._metadataHydrationPromise;
  }

  private async _runMetadataHydration(): Promise<void> {
    const BATCH = 12;
    // Capture object references (not indices) so that later insertions into
    // this.images cannot cause us to skip or mis-target images.
    const pending = this.images.filter((img) => img && !img.metadata && img.gallery_filename);
    console.debug(`[artist] Hydrating metadata for ${pending.length} / ${this.images.length} images`);
    let hydrated = 0;
    for (let b = 0; b < pending.length; b += BATCH) {
      const slice = pending.slice(b, b + BATCH);
      await Promise.all(
        slice.map(async (img) => {
          if (!img || img.metadata || !img.gallery_filename) return;
          try {
            const meta = await readImageMetadata(img.gallery_filename);
            if (!meta) {
              console.debug(`[artist] readImageMetadata returned null for ${img.gallery_filename}`);
              return;
            }
            if (!img.metadata) {
              // Direct property mutation — same pattern as App.svelte's lightbox
              // load path; Svelte 5's deep proxy tracks this correctly.
              img.metadata = meta;
              hydrated++;
            }
          } catch (e) {
            // Non-fatal — just means this image won't get artist badges.
            console.debug("[artist] Metadata hydration failed for", img.gallery_filename, e);
          }
        }),
      );
      // Yield to keep the UI responsive between batches
      await new Promise((r) => setTimeout(r, 0));
    }
    console.debug(`[artist] Hydration complete: ${hydrated} images loaded`);
  }

  /** Load full-resolution image data on demand. Returns the blob URL. */
  async loadFullImage(galleryFilename: string): Promise<string> {
    const bytes = await loadGalleryImage(galleryFilename);
    const ext = galleryFilename.split(".").pop()?.toLowerCase() ?? "png";
    const mimeType =
      ext === "jpg" || ext === "jpeg"
        ? "image/jpeg"
        : ext === "webp"
          ? "image/webp"
          : "image/png";
    const blob = new Blob([new Uint8Array(bytes)], { type: mimeType });
    return URL.createObjectURL(blob);
  }

  /** Save an image to a user-chosen location via native file dialog (or browser download). */
  async saveImageAs(image: OutputImage) {
    if (this.saving) return;
    this.saving = true;
    try {
      let bytes: number[];
      if (image.gallery_filename) {
        bytes = await loadGalleryImage(image.gallery_filename);
      } else if (image.url) {
        const response = await fetch(image.url);
        const buf = await response.arrayBuffer();
        bytes = Array.from(new Uint8Array(buf));
      } else {
        bytes = await getOutputImage(image.filename, image.subfolder);
      }

      const path = await showSaveDialog(image.filename, ["png", "jpg", "jpeg", "webp"]);
      if (path) {
        await saveImageFile(bytes, path);
      } else {
        triggerBrowserDownload(new Uint8Array(bytes), image.filename, "image/png");
      }
      this.showToast(locale.t("gallery.toast.image_saved"), "success");
    } catch (e) {
      console.error("Failed to save image:", e);
    } finally {
      this.saving = false;
    }
  }

  /** Save a blob URL image to a user-chosen location (or browser download). */
  async saveBlobAs(blobUrl: string, defaultName: string = "image.png") {
    if (this.saving) return;
    this.saving = true;
    try {
      const response = await fetch(blobUrl);
      const blob = await response.blob();

      const path = await showSaveDialog(defaultName, ["png", "jpg", "jpeg", "webp"]);
      if (path) {
        const arrayBuf = await blob.arrayBuffer();
        const bytes = Array.from(new Uint8Array(arrayBuf));
        await saveImageFile(bytes, path);
      } else {
        const arrayBuf = await blob.arrayBuffer();
        triggerBrowserDownload(new Uint8Array(arrayBuf), defaultName, blob.type || "image/png");
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
      let bytes: number[];
      if (image.gallery_filename) {
        bytes = await loadGalleryImage(image.gallery_filename);
      } else if (image.url) {
        const response = await fetch(image.url);
        const buf = await response.arrayBuffer();
        bytes = Array.from(new Uint8Array(buf));
      } else {
        bytes = await getOutputImage(image.filename, image.subfolder);
      }
      const filename = image.filename || `image_${Date.now()}.png`;
      if (image.metadata && filename.toLowerCase().endsWith(".png")) {
        bytes = await embedPngMetadataBytes(bytes, image.metadata, generation.metadataMode);
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
        if (!fetchUrl) fetchUrl = image.url;
        if (fetchUrl) {
          try {
            const resp = await fetch(fetchUrl);
            if (!resp.ok) {
              this.showToast(locale.t("gallery.toast.copy_failed") || "Failed to copy image", "error");
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
      // Tauri mode: prefer native clipboard via file path
      if (image.gallery_filename) {
        const path = await getGalleryImagePath(image.gallery_filename);
        await copyImageToClipboard(path);
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
    throw new Error("Clipboard API not available — copy not supported in this browser context");
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

      if (metadata) {
        bytes = await embedPngMetadataBytes(bytes, metadata);
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
    } catch (e) {
      console.error("Failed to delete image:", e);
    }
  }

  private inferModeFromFilename(
    image: OutputImage,
  ): "txt2img" | "img2img" | "inpainting" {
    const n = `${image.filename} ${image.gallery_filename ?? ""}`.toLowerCase();
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
    const fmt = (b: number) => {
      if (b >= 1_073_741_824) return `${(b / 1_073_741_824).toFixed(1)} GB`;
      if (b >= 1_048_576) return `${(b / 1_048_576).toFixed(0)} MB`;
      return `${(b / 1024).toFixed(0)} KB`;
    };
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
}

export const gallery = new GalleryStore();
