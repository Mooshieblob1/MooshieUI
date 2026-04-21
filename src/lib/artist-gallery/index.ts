/**
 * Portable Artist Gallery module
 * ===============================
 * Zero Tauri / backend dependencies. Consumes only `fetch` + a manifest URL.
 * Drop this folder into any Svelte 5 codebase (desktop shell or static site)
 * and render `<ArtistGalleryPage manifestUrl={...} />`.
 */
export type { ArtistEntry, ArtistShard, ArtistManifest, ArtistSearchHit, ArtistGalleryClient } from "./types.js";
export { createArtistGalleryClient } from "./client.js";
export { ArtistGalleryStore, createArtistGalleryStore } from "./store.svelte.js";
export { default as ArtistGalleryPage } from "./components/ArtistGalleryPage.svelte";
export { default as ArtistCard } from "./components/ArtistCard.svelte";
export { default as ArtistLightbox } from "./components/ArtistLightbox.svelte";
export { default as ArtistHoverPreview } from "./components/ArtistHoverPreview.svelte";
