import type { CheckpointCivitaiInfo, LoraCivitaiInfo } from "./api.js";
import { MODEL_FAMILIES } from "./modelFamily.js";
import type { ModelFamily } from "./modelFamily.js";

export type ModelGallerySort = "name" | "folder" | "family" | "tree";

export const FAMILY_ORDER: ModelFamily[] = [
  ...MODEL_FAMILIES,
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
    // "tree" reuses the plain name ordering here; the folder hierarchy is built
    // separately by buildFilenameTree() from this already-sorted list.
    return displayName(a).localeCompare(displayName(b));
  });
}

/** A folder node in a filename-derived tree. Root has an empty name/path. */
export interface FilenameTreeNode {
  name: string;
  path: string;
  folders: FilenameTreeNode[];
  files: string[];
}

/**
 * Groups flat model filenames (e.g. "anime/style.safetensors") into a folder
 * tree for the bottom-panel tree view. Files keep their incoming order within a
 * folder (so an enabled-first caller stays enabled-first); folders sort by name.
 */
export function buildFilenameTree(filenames: string[]): FilenameTreeNode {
  const root: FilenameTreeNode = { name: "", path: "", folders: [], files: [] };
  for (const filename of filenames) {
    const segments = filename.replace(/\\/g, "/").split("/").filter(Boolean);
    const folderSegments = segments.slice(0, -1);
    let node = root;
    let currentPath = "";
    for (const segment of folderSegments) {
      currentPath = currentPath ? `${currentPath}/${segment}` : segment;
      let child = node.folders.find((c) => c.name === segment);
      if (!child) {
        child = { name: segment, path: currentPath, folders: [], files: [] };
        node.folders.push(child);
      }
      node = child;
    }
    node.files.push(filename);
  }
  sortTreeFolders(root);
  return root;
}

function sortTreeFolders(node: FilenameTreeNode): void {
  node.folders.sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
  for (const child of node.folders) sortTreeFolders(child);
}

/** Total file count for a node, including all descendants. */
export function countTreeFiles(node: FilenameTreeNode): number {
  return node.files.length + node.folders.reduce((sum, child) => sum + countTreeFiles(child), 0);
}
