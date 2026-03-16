import type { OutputImage } from "../types/index.js";

class GalleryStore {
  images = $state<OutputImage[]>([]);
  selectedImage = $state<OutputImage | null>(null);
  lightboxOpen = $state(false);

  addImages(newImages: OutputImage[]) {
    this.images = [...newImages, ...this.images];
  }

  openLightbox(image: OutputImage) {
    this.selectedImage = image;
    this.lightboxOpen = true;
  }

  closeLightbox() {
    this.lightboxOpen = false;
    this.selectedImage = null;
  }
}

export const gallery = new GalleryStore();
