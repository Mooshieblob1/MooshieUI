import type { OutputImage } from "../types/index.js";
import {
  listGalleryImages,
  loadGalleryImage,
  saveToGallery,
  deleteGalleryImage,
  saveImageFile,
  getOutputImage,
  copyImageToClipboard,
  getGalleryImagePath,
} from "../utils/api.js";
import { save } from "@tauri-apps/plugin-dialog";

class GalleryStore {
  images = $state<OutputImage[]>([]);
  /** Images generated during this app session (not loaded from disk). */
  sessionImages = $state<OutputImage[]>([]);
  selectedImage = $state<OutputImage | null>(null);
  lightboxOpen = $state(false);
  loading = $state(false);
  toastMessage = $state<string | null>(null);
  private _toastTimer: ReturnType<typeof setTimeout> | null = null;

  addImages(newImages: OutputImage[]) {
    this.images = [...newImages, ...this.images];
    this.sessionImages = [...newImages, ...this.sessionImages];
  }

  openLightbox(image: OutputImage) {
    this.selectedImage = image;
    this.lightboxOpen = true;
  }

  closeLightbox() {
    this.lightboxOpen = false;
    this.selectedImage = null;
  }

  showToast(message: string) {
    this.toastMessage = message;
    if (this._toastTimer) clearTimeout(this._toastTimer);
    this._toastTimer = setTimeout(() => {
      this.toastMessage = null;
      this._toastTimer = null;
    }, 2000);
  }

  /** Save generated images to the persistent gallery on disk. */
  async persistImages(images: OutputImage[]) {
    for (const img of images) {
      try {
        const galleryFilename = await saveToGallery(
          img.filename,
          img.subfolder,
          img.prompt_id
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
      const filenames = await listGalleryImages();
      const loaded: OutputImage[] = [];
      for (const filename of filenames) {
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

          const underscoreIdx = filename.indexOf("_");
          const promptId =
            underscoreIdx > 0 ? filename.substring(0, underscoreIdx) : "";
          const origFilename =
            underscoreIdx > 0
              ? filename.substring(underscoreIdx + 1)
              : filename;

          loaded.push({
            filename: origFilename,
            subfolder: "",
            type: "output",
            prompt_id: promptId,
            url,
            gallery_filename: filename,
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
      this.showToast("Image saved");
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
      this.showToast("Image saved");
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
      } else {
        this.showToast("Image not saved to gallery yet");
        return;
      }
      this.showToast("Copied to clipboard");
    } catch (e) {
      console.error("Failed to copy to clipboard:", e);
      this.showToast("Failed to copy");
    }
  }

  /** Copy a gallery image file to clipboard by filename. */
  async copyBlobToClipboard(blobUrl: string) {
    // For preview images that aren't in the gallery yet, we can't copy as file
    // This is a fallback that shouldn't normally be reached
    this.showToast("Save to gallery first to copy");
  }

  /** Delete an image from the gallery. */
  async deleteImage(image: OutputImage) {
    try {
      if (image.gallery_filename) {
        await deleteGalleryImage(image.gallery_filename);
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
}

export const gallery = new GalleryStore();
