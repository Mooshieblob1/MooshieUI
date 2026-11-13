import type { OutputImage } from "../types/index.js";

/** What the editor was opened on: a gallery/session image, or a bare URL. */
export interface ImageEditorSource {
  image: OutputImage | null;
  /** Used when there is no `image`, or as a fallback for one with no file yet. */
  url: string | null;
  /** Base name for the saved copy. */
  name: string;
}

/**
 * Open/close state for the built-in image editor. A leaf store: the editor
 * component owns the document, so anything that can show an image only needs
 * `imageEditor.open(image)`.
 */
class ImageEditorStore {
  /** Non-null exactly while the editor is open. */
  source = $state<ImageEditorSource | null>(null);

  open(image: OutputImage, fallbackUrl: string | null = null) {
    this.source = { image, url: fallbackUrl, name: image.filename || "image.png" };
  }

  openUrl(url: string, name = "image.png") {
    this.source = { image: null, url, name };
  }

  close() {
    this.source = null;
  }
}

export const imageEditor = new ImageEditorStore();
