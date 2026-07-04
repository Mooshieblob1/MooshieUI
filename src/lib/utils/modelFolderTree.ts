import type { ManagedModelFile, ManagedModelFolder, ModelInstallDir } from "./api.js";

export interface FolderTreeNode {
  /** Segment name; the label of the install dir for a root node. */
  name: string;
  /** Path relative to the install dir root ("" for the root node). */
  path: string;
  /** Install dir root path this node lives under. */
  directory: string;
  directoryLabel: string;
  children: FolderTreeNode[];
  files: ManagedModelFile[];
}

/**
 * Builds one tree per install directory from the flat file/folder lists
 * returned by listModelFiles()/listModelFolders(). Folders are included even
 * when empty so newly-created (not-yet-populated) folders still appear.
 */
export function buildFolderTrees(
  installDirs: ModelInstallDir[],
  files: ManagedModelFile[],
  folders: ManagedModelFolder[],
): FolderTreeNode[] {
  const roots = new Map<string, FolderTreeNode>();
  for (const dir of installDirs) {
    roots.set(dir.path, {
      name: dir.label,
      path: "",
      directory: dir.path,
      directoryLabel: dir.label,
      children: [],
      files: [],
    });
  }

  function ensureNode(root: FolderTreeNode, segments: string[]): FolderTreeNode {
    let node = root;
    let currentPath = "";
    for (const segment of segments) {
      currentPath = currentPath ? `${currentPath}/${segment}` : segment;
      let child = node.children.find((c) => c.name === segment);
      if (!child) {
        child = {
          name: segment,
          path: currentPath,
          directory: root.directory,
          directoryLabel: root.directoryLabel,
          children: [],
          files: [],
        };
        node.children.push(child);
      }
      node = child;
    }
    return node;
  }

  for (const folder of folders) {
    const root = roots.get(folder.directory);
    if (!root) continue;
    const segments = folder.path.split("/").filter(Boolean);
    if (segments.length > 0) ensureNode(root, segments);
  }

  for (const file of files) {
    const root = roots.get(file.directory);
    if (!root) continue;
    const segments = file.filename.split("/").filter(Boolean);
    const basename = segments.pop();
    if (basename === undefined) continue;
    const node = ensureNode(root, segments);
    node.files.push(file);
  }

  const trees = [...roots.values()];
  for (const root of trees) sortFolderTree(root);
  return trees;
}

function sortFolderTree(node: FolderTreeNode): void {
  node.children.sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
  node.files.sort((a, b) => a.filename.localeCompare(b.filename, undefined, { sensitivity: "base" }));
  for (const child of node.children) sortFolderTree(child);
}

/** Flattens a tree into every folder node (including the root), depth-first. */
export function flattenFolderNodes(node: FolderTreeNode): FolderTreeNode[] {
  return [node, ...node.children.flatMap(flattenFolderNodes)];
}

/** Total file count for a node, including all descendants. */
export function countFilesRecursive(node: FolderTreeNode): number {
  return node.files.length + node.children.reduce((sum, child) => sum + countFilesRecursive(child), 0);
}
