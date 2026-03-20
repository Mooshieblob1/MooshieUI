import type { OutputImage } from "../types/index.js";
import {
  listGalleryImageEntries,
  loadGalleryImage,
  saveToGallery,
  deleteGalleryImage,
  renameGalleryImage,
  saveImageFile,
  getOutputImage,
  copyImageToClipboard,
  getGalleryImagePath,
} from "../utils/api.js";
import { save } from "@tauri-apps/plugin-dialog";

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
  loading = $state(false);
  toast = $state<{ message: string; type: "success" | "error" | "info" } | null>(null);
  boardAssignments = $state<Record<string, string>>({});
  customBoards = $state<string[]>([]);
  private _toastTimer: ReturnType<typeof setTimeout> | null = null;

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

  addImages(newImages: OutputImage[]) {
    this.images = [...newImages, ...this.images];
    this.sessionImages = [...newImages, ...this.sessionImages];
  }

  openLightbox(image: OutputImage) {
    this.selectedImage = image;
    this.lightboxUrl = null;
    this.lightboxOpen = true;
  }

  /** Open lightbox with a raw image URL (e.g. preview blob). */
  openLightboxUrl(url: string) {
    this.selectedImage = null;
    this.lightboxUrl = url;
    this.lightboxOpen = true;
  }

  closeLightbox() {
    this.lightboxOpen = false;
    this.selectedImage = null;
    this.lightboxUrl = null;
  }

  showToast(message: string, type: "success" | "error" | "info" = "info") {
    this.toast = { message, type };
    if (this._toastTimer) clearTimeout(this._toastTimer);
    this._toastTimer = setTimeout(() => {
      this.toast = null;
      this._toastTimer = null;
    }, 2000);
  }

  /** Save generated images to the persistent gallery on disk. */
  async persistImages(images: OutputImage[], metadata?: Record<string, string>) {
    for (const img of images) {
      try {
        const galleryFilename = await saveToGallery(
          img.filename,
          img.subfolder,
          img.prompt_id,
          img.generation_mode,
          metadata
        );
        img.gallery_filename = galleryFilename;
      } catch (e) {
        console.error("Failed to save image to gallery:", e);
      }
    }
  }

  /** Load previously saved gallery images from disk on startup. */
  async loadFromDisk() {
    this.loading = true;
    try {
      const entries = await listGalleryImageEntries();
      const loaded: OutputImage[] = [];
      for (const entry of entries) {
        const filename = entry.filename;
        try {
          const bytes = await loadGalleryImage(filename);
          const ext = filename.split(".").pop()?.toLowerCase() ?? "png";
          const mimeType =
            ext === "jpg" || ext === "jpeg"
              ? "image/jpeg"
              : ext === "webp"
                ? "image/webp"
                : "image/png";
          const blob = new Blob([new Uint8Array(bytes)], { type: mimeType });
          const url = URL.createObjectURL(blob);

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
            url,
            gallery_filename: filename,
            file_size_bytes: entry.size_bytes,
            generated_at_ms: entry.modified_ms,
          });
        } catch (e) {
          console.error(`Failed to load gallery image ${filename}:`, e);
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
  }

  /** Save an image to a user-chosen location via native file dialog. */
  async saveImageAs(image: OutputImage) {
    try {
      const path = await save({
        defaultPath: image.filename,
        filters: [
          { name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] },
        ],
      });
      if (!path) return;

      let bytes: number[];
      if (image.gallery_filename) {
        bytes = await loadGalleryImage(image.gallery_filename);
      } else {
        bytes = await getOutputImage(image.filename, image.subfolder);
      }
      await saveImageFile(bytes, path);
      this.showToast("Image saved", "success");
    } catch (e) {
      console.error("Failed to save image:", e);
    }
  }

  /** Save a blob URL image to a user-chosen location. */
  async saveBlobAs(blobUrl: string, defaultName: string = "image.png") {
    try {
      const path = await save({
        defaultPath: defaultName,
        filters: [
          { name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] },
        ],
      });
      if (!path) return;

      const response = await fetch(blobUrl);
      const blob = await response.blob();
      const arrayBuf = await blob.arrayBuffer();
      const bytes = Array.from(new Uint8Array(arrayBuf));
      await saveImageFile(bytes, path);
      this.showToast("Image saved", "success");
    } catch (e) {
      console.error("Failed to save image:", e);
    }
  }

  /** Copy a gallery image file to clipboard (as file reference). */
  async copyToClipboard(image: OutputImage) {
    try {
      if (image.gallery_filename) {
        const path = await getGalleryImagePath(image.gallery_filename);
        await copyImageToClipboard(path);
      } else if (image.url) {
        await this.copyBlobToClipboard(image.url);
        return;
      } else {
        this.showToast("Image not saved to gallery yet", "info");
        return;
      }
      this.showToast("Copied to clipboard", "success");
    } catch (e) {
      console.error("Failed to copy to clipboard:", e);
      this.showToast("Failed to copy", "error");
    }
  }

  /** Copy a blob URL image to clipboard via native Tauri clipboard. */
  async copyBlobToClipboard(blobUrl: string) {
    try {
      const response = await fetch(blobUrl);
      const blob = await response.blob();
      const arrayBuf = await blob.arrayBuffer();
      const bytes = Array.from(new Uint8Array(arrayBuf));

      // Write to a temp file, then use native clipboard
      const tmpPath = `/tmp/mooshieui_clipboard_${Date.now()}.png`;
      await saveImageFile(bytes, tmpPath);
      await copyImageToClipboard(tmpPath);
      this.showToast("Copied to clipboard", "success");
    } catch (e) {
      console.error("Failed to copy blob to clipboard:", e);
      this.showToast("Failed to copy", "error");
    }
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
        for (const image of this.images) {
          if (image.url) URL.revokeObjectURL(image.url);
        }
        this.images = [];
        this.sessionImages = [];
        await this.loadFromDisk();
      }

      this.showToast(
        migrated > 0
          ? `Re-scanned metadata: migrated ${migrated} image${migrated === 1 ? "" : "s"}`
          : "Re-scan complete: no legacy metadata to migrate",
        migrated > 0 ? "success" : "info"
      );
    } catch (e) {
      console.error("Failed to re-scan gallery metadata:", e);
      this.showToast("Re-scan failed", "error");
    }
  }
}

export const gallery = new GalleryStore();
