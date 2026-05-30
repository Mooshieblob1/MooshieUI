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

function familyFromBaseModelLabel(label: string | null | undefined): ModelFamily | null {
  if (!label) return null;
  const bm = label.toLowerCase();
  if (civitaiBaseModelIndicatesAnima(label)) return "anima";
  if (bm.includes("illustrious") || bm.includes("noob")) return "illustrious";
  if (bm.includes("pony")) return "pony";
  if (bm.includes("sdxl")) return "sdxl";
  if (bm.includes("sd 1.5") || bm.includes("sd1.5") || bm === "sd 1.5") return "sd15";
  if (bm.includes("flux")) return "flux";
  if (bm.includes("sd 3") || bm.includes("sd3")) return "sd3";
  return null;
}

export function inferCheckpointFamily(
  filename: string,
  info?: Pick<CheckpointCivitaiInfo, "base_model" | "modelspec_architecture"> | null,
): ModelFamily {
  const fromCivitai = familyFromBaseModelLabel(info?.base_model);
  if (fromCivitai) return fromCivitai;
  if (filenameIndicatesIllustrious(filename)) return "illustrious";
  if (filenameIndicatesAnima(filename)) return "anima";
  const arch = (info?.modelspec_architecture ?? "").toLowerCase();
  if (arch.includes("anima") || arch.includes("wan")) return "anima";
  const n = filename.toLowerCase();
  if (n.includes("pony")) return "pony";
  if (n.includes("flux")) return "flux";
  if (n.includes("sdxl")) return "sdxl";
  if (n.includes("sd15") || n.includes("sd1.5")) return "sd15";
  if (n.includes("nanosaur")) return "nanosaur";
  return "unknown";
}

export function inferLoraFamily(
  filename: string,
  info?: Pick<LoraCivitaiInfo, "civitai_base_model" | "modelspec_architecture"> | null,
): ModelFamily {
  const fromCivitai = familyFromBaseModelLabel(info?.civitai_base_model);
  if (fromCivitai) return fromCivitai;
  if (filenameIndicatesIllustrious(filename)) return "illustrious";
  if (filenameIndicatesAnima(filename)) return "anima";
  const arch = (info?.modelspec_architecture ?? "").toLowerCase();
  if (arch.includes("anima") || arch.includes("wan")) return "anima";
  const n = filename.toLowerCase();
  if (n.includes("pony")) return "pony";
  if (n.includes("flux")) return "flux";
  if (n.includes("sdxl")) return "sdxl";
  if (n.includes("sd15") || n.includes("sd1.5")) return "sd15";
  return "unknown";
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
