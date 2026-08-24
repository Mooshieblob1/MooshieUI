/**
 * The NovelAI import dialog's pending state.
 *
 * Dropping or pasting a NovelAI image does not apply anything on its own any
 * more: the metadata is parked here and the dialog asks what to do with it,
 * the way novelai.net does. Every other backend keeps applying straight away.
 *
 * A leaf store on purpose. It holds the staged image and the checkbox state and
 * nothing else; the code that acts on a choice lives in `metadataImport` and in
 * the dialog, so this never has to import a feature store.
 */

import { isTauri } from "../utils/ipc.js";

/**
 * Where the staged image came from.
 *
 * Native Tauri drops hand over a path and nothing else, so the bytes behind one
 * are read only if the user actually asks for the preview or for an action that
 * needs the pixels. A drop that ends in "import metadata" never touches disk.
 */
export type NovelAiImportSource =
  | { kind: "file"; file: File }
  | { kind: "bytes"; bytes: number[]; filename: string }
  | { kind: "path"; path: string };

/** Which parts of the metadata the Import Metadata button will write. */
export interface NovelAiImportSelection {
  prompt: boolean;
  undesired: boolean;
  characters: boolean;
  /** Add the image's characters to the panel's own instead of replacing them. */
  appendCharacters: boolean;
  /** Empty the character panel, for an image that carries no characters. */
  clearCharacters: boolean;
  settings: boolean;
  seed: boolean;
  /** Strip the app's own quality tags and unsupported inline syntax. */
  clean: boolean;
}

/** The defaults novelai.net opens with: everything but the seed and Append. */
export function defaultNovelAiImportSelection(): NovelAiImportSelection {
  return {
    prompt: true,
    undesired: true,
    characters: true,
    appendCharacters: false,
    clearCharacters: false,
    settings: true,
    seed: false,
    clean: true,
  };
}

function filenameFor(source: NovelAiImportSource): string {
  switch (source.kind) {
    case "file":
      return source.file.name || "image.png";
    case "bytes":
      return source.filename;
    case "path":
      return source.path.split(/[\\/]/).pop() || "image.png";
  }
}

class NovelAiImportStore {
  /** The parsed metadata awaiting a decision, or null when nothing is staged. */
  meta = $state<Record<string, string> | null>(null);
  selection = $state<NovelAiImportSelection>(defaultNovelAiImportSelection());
  /** Object URL for the staged image, resolved lazily once the dialog opens. */
  previewUrl = $state<string | null>(null);
  /** Set while an action button is uploading or encoding, to lock the dialog. */
  busy = $state(false);

  private source: NovelAiImportSource | null = null;
  private bytes: number[] | null = null;

  get isOpen(): boolean {
    return this.meta !== null;
  }

  get filename(): string {
    return this.source ? filenameFor(this.source) : "image.png";
  }

  /** Stage an image for the dialog, replacing anything already staged. */
  open(meta: Record<string, string>, source: NovelAiImportSource): void {
    this.close();
    this.meta = meta;
    this.source = source;
    this.selection = defaultNovelAiImportSelection();
    void this.loadPreview();
  }

  close(): void {
    if (this.previewUrl) {
      URL.revokeObjectURL(this.previewUrl);
      this.previewUrl = null;
    }
    this.meta = null;
    this.source = null;
    this.bytes = null;
    this.busy = false;
  }

  update(patch: Partial<NovelAiImportSelection>): void {
    this.selection = { ...this.selection, ...patch };
  }

  /**
   * The staged image's bytes, read on demand and kept for the rest of the
   * dialog's life. Returns null when a path could not be read.
   */
  async imageBytes(): Promise<number[] | null> {
    if (this.bytes) return this.bytes;
    const source = this.source;
    if (!source) return null;
    try {
      if (source.kind === "bytes") {
        this.bytes = source.bytes;
      } else if (source.kind === "file") {
        this.bytes = Array.from(
          new Uint8Array(await source.file.arrayBuffer()),
        );
      } else if (isTauri) {
        const { readFile } = await import("@tauri-apps/plugin-fs");
        this.bytes = Array.from(await readFile(source.path));
      }
    } catch (err) {
      console.error("Failed to read the staged NovelAI image:", err);
      return null;
    }
    return this.bytes;
  }

  /** The staged image as a `File`, for the paths that upload or re-encode it. */
  async imageFile(): Promise<File | null> {
    if (this.source?.kind === "file") return this.source.file;
    const bytes = await this.imageBytes();
    if (!bytes) return null;
    return new File([new Uint8Array(bytes)], this.filename, {
      type: "image/png",
    });
  }

  private async loadPreview(): Promise<void> {
    const staged = this.meta;
    const bytes = await this.imageBytes();
    // The dialog may have been closed or replaced while the read was in flight.
    if (!bytes || this.meta !== staged) return;
    const url = URL.createObjectURL(
      new Blob([new Uint8Array(bytes)], { type: "image/png" }),
    );
    if (this.meta !== staged) {
      URL.revokeObjectURL(url);
      return;
    }
    this.previewUrl = url;
  }
}

export const novelaiImport = new NovelAiImportStore();
