/**
 * Tags the tagger found in gallery images that have no saved prompt, kept in
 * IndexedDB so an image is only ever tagged once. Saved character detection
 * reads them in place of the missing prompt.
 */

import type { InterrogationResult } from "../types/index.js";

const DB_NAME = "mooshieui.gallery.tags";
const STORE = "tags";

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    if (typeof indexedDB === "undefined") {
      reject(new Error("IndexedDB is not available"));
      return;
    }
    const request = indexedDB.open(DB_NAME, 1);
    request.onupgradeneeded = () => {
      if (!request.result.objectStoreNames.contains(STORE)) request.result.createObjectStore(STORE);
    };
    request.onsuccess = () => {
      request.result.onversionchange = () => request.result.close();
      resolve(request.result);
    };
    request.onerror = () => reject(request.error);
    request.onblocked = () => reject(new Error("The gallery tag cache is open in an older app window"));
  });
}

/** Every cached tag string, by image key. Empty when the cache cannot be read. */
export async function loadGalleryTags(): Promise<Record<string, string>> {
  let db: IDBDatabase;
  try {
    db = await openDb();
  } catch {
    return {};
  }
  try {
    return await new Promise((resolve) => {
      const tags: Record<string, string> = {};
      const request = db.transaction(STORE, "readonly").objectStore(STORE).openCursor();
      request.onsuccess = () => {
        const cursor = request.result;
        if (!cursor) return resolve(tags);
        if (typeof cursor.key === "string" && typeof cursor.value === "string") tags[cursor.key] = cursor.value;
        cursor.continue();
      };
      request.onerror = () => resolve(tags);
    });
  } finally {
    db.close();
  }
}

/** Store tag strings. A failed write only means the images are tagged again next time. */
export async function saveGalleryTags(entries: Record<string, string>): Promise<void> {
  let db: IDBDatabase;
  try {
    db = await openDb();
  } catch {
    return;
  }
  try {
    await new Promise<void>((resolve) => {
      const tx = db.transaction(STORE, "readwrite");
      const store = tx.objectStore(STORE);
      for (const [key, value] of Object.entries(entries)) store.put(value, key);
      tx.oncomplete = () => resolve();
      tx.onabort = () => resolve();
      tx.onerror = () => resolve();
    });
  } finally {
    db.close();
  }
}

/**
 * A tagger result as one prompt-like tag string: who is in the picture first
 * (character tags name them outright), then what they look like.
 */
export function tagsFromInterrogation(result: Pick<InterrogationResult, "character_tags" | "general_tags">): string {
  const seen = new Set<string>();
  for (const tag of [...(result.character_tags ?? []), ...(result.general_tags ?? [])]) {
    const name = tag.name?.trim();
    if (name) seen.add(name);
  }
  return [...seen].join(", ");
}
