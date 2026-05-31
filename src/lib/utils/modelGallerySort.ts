import type { CheckpointCivitaiInfo, LoraCivitaiInfo } from "./api.js";
import {
  civitaiBaseModelIndicatesAnima,
  filenameIndicatesAnima,
  filenameIndicatesIllustrious,
  type ModelFamily,
} from "./modelFamily.js";

export type ModelGallerySort = "name" | "folder" | "family";

const FAMILY_ORDER: ModelFamily[] = [
  "anima",
  "illustrious",
  "pony",
  "sdxl",
  "sd15",
  "flux",
  "zib",
  "zit",
  "sd3",
  "nanosaur",
  "mugen",
  "unknown",
];

/** Parent folder path for a ComfyUI model filename (empty string = root of category). */
export function modelFolderPath(filename: string): string {
  const normalized = filename.replace(/\\/g, "/");
  const idx = normalized.lastIndexOf("/");
  return idx >= 0 ? normalized.slice(0, idx) : "";
}

export function inferCheckpointFamily(
  _filename: string,
  info?: Pick<CheckpointCivitaiInfo, "family"> | null,
): ModelFamily {
  return (info?.family as ModelFamily | undefined) ?? "unknown";
}

export function inferLoraFamily(
  _filename: string,
  info?: Pick<LoraCivitaiInfo, "family"> | null,
): ModelFamily {
  return (info?.family as ModelFamily | undefined) ?? "unknown";
}

function familyRank(family: ModelFamily): number {
  const idx = FAMILY_ORDER.indexOf(family);
  return idx >= 0 ? idx : FAMILY_ORDER.length;
}

/** Sort model filenames for gallery display. Active model stays first when `active` is set. */
export function sortModelFilenames(
  filenames: string[],
  sort: ModelGallerySort,
  active: string | null,
  displayName: (filename: string) => string,
  familyOf: (filename: string) => ModelFamily,
): string[] {
  return [...filenames].sort((a, b) => {
    if (active) {
      if (a === active && b !== active) return -1;
      if (b === active && a !== active) return 1;
    }
    if (sort === "folder") {
      const fa = modelFolderPath(a);
      const fb = modelFolderPath(b);
      if (fa !== fb) return fa.localeCompare(fb);
      return displayName(a).localeCompare(displayName(b));
    }
    if (sort === "family") {
      const ra = familyRank(familyOf(a));
      const rb = familyRank(familyOf(b));
      if (ra !== rb) return ra - rb;
      return displayName(a).localeCompare(displayName(b));
    }
    return displayName(a).localeCompare(displayName(b));
  });
}
