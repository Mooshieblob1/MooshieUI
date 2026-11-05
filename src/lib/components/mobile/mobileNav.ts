/**
 * Pages of the phone layout and how the bottom bar splits them.
 *
 * The bar keeps four of them in reach and puts the rest behind "More", which
 * opens a sheet listing every page and tool the desktop sidebar has. Five slots
 * is the most that keeps each label readable at 360 px; the old bar squeezed
 * eight in and the labels ran into each other.
 */

import { locale } from "../../stores/locale.svelte.js";

/** Same page ids as the desktop sidebar, so App.svelte's navigation works on both. */
export type MobilePage =
  | "generate"
  | "music"
  | "model3d"
  | "scene"
  | "gallery"
  | "modelhub"
  | "artists"
  | "characters"
  | "settings";

/** A page, or the video workspace (the generate page in video mode). */
export type MobileNavId = MobilePage | "video";

export interface MobileNavOptions {
  showModelhub: boolean;
  showModel3d: boolean;
}

/** Every destination, in the order the "More" sheet lists them. */
export function allDestinations(opts: MobileNavOptions): MobileNavId[] {
  return [
    "generate",
    "video",
    "gallery",
    "music",
    "artists",
    "characters",
    "scene",
    ...(opts.showModel3d ? (["model3d"] as const) : []),
    ...(opts.showModelhub ? (["modelhub"] as const) : []),
    "settings",
  ];
}

/** The four slots before "More". */
export const PRIMARY_DESTINATIONS: MobileNavId[] = ["generate", "video", "gallery", "music"];

const LABEL_KEYS: Record<MobileNavId, string> = {
  generate: "nav.generate",
  video: "generation.mode.video",
  music: "nav.music",
  gallery: "nav.gallery",
  modelhub: "nav.modelhub",
  artists: "nav.artists",
  characters: "artist_gallery.tab_characters",
  scene: "nav.scene",
  model3d: "nav.model3d_experimental",
  settings: "nav.settings",
};

export function navLabel(id: MobileNavId): string {
  return locale.t(LABEL_KEYS[id]);
}
