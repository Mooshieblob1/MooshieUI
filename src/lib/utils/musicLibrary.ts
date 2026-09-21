import type { MusicBatch, MusicPlan, MusicPlaylist, MusicResult } from "../types/music.js";
import type { SavedMusicStyle } from "./musicStyleProfiles.js";
import type { MusicComparisonNotes } from "./musicComparison.js";

// Audio is stored separately so browsing a library never loads every recording.
// Each signed-in user gets a separate database on this browser/device.
function openLibrary(owner: string): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(`mooshieui.music.library.${owner}`, 3);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains("songs")) db.createObjectStore("songs", { keyPath: "prompt_id" });
      if (!db.objectStoreNames.contains("audio")) db.createObjectStore("audio");
      if (!db.objectStoreNames.contains("playlists")) db.createObjectStore("playlists", { keyPath: "id" });
      if (!db.objectStoreNames.contains("plans")) db.createObjectStore("plans", { keyPath: "prompt_id" });
      if (!db.objectStoreNames.contains("batches")) db.createObjectStore("batches", { keyPath: "id" });
      if (!db.objectStoreNames.contains("styles")) db.createObjectStore("styles", { keyPath: "id" });
      if (!db.objectStoreNames.contains("comparisons")) db.createObjectStore("comparisons", { keyPath: "id" });
    };
    request.onsuccess = () => { request.result.onversionchange = () => request.result.close(); resolve(request.result); };
    request.onerror = () => reject(request.error);
    request.onblocked = () => reject(new Error("Music library is open in an older app window"));
  });
}

async function transaction<T>(owner: string, stores: string[], mode: IDBTransactionMode, run: (tx: IDBTransaction) => () => T): Promise<T> {
  const db = await openLibrary(owner);
  try {
    return await new Promise<T>((resolve, reject) => {
      const tx = db.transaction(stores, mode);
      const result = run(tx);
      tx.oncomplete = () => resolve(result());
      tx.onabort = () => reject(tx.error ?? new Error("Music library transaction aborted"));
      tx.onerror = () => reject(tx.error);
    });
  } finally { db.close(); }
}

// Strip Svelte proxies before structured cloning into IndexedDB.
const plain = <T>(value: T): T => JSON.parse(JSON.stringify(value));

export const musicLibrary = {
  async load(owner: string) {
    return transaction(owner, ["songs", "playlists", "plans", "batches", "styles", "comparisons"], "readonly", tx => {
      const songs = tx.objectStore("songs").getAll();
      const playlists = tx.objectStore("playlists").getAll();
      const plans = tx.objectStore("plans").getAll();
      const batches = tx.objectStore("batches").getAll();
      const styles = tx.objectStore("styles").getAll();
      const comparisons = tx.objectStore("comparisons").getAll();
      return () => ({ songs: songs.result as MusicResult[], playlists: playlists.result as MusicPlaylist[], plans: plans.result as MusicPlan[], batches: batches.result as MusicBatch[], styles: styles.result as SavedMusicStyle[], comparisons: comparisons.result as MusicComparisonNotes[] });
    });
  },
  async audio(owner: string, id: string): Promise<Blob | undefined> {
    return transaction(owner, ["audio"], "readonly", tx => {
      const request = tx.objectStore("audio").get(id);
      return () => request.result;
    });
  },
  async saveStyle(owner: string, style: SavedMusicStyle) {
    return transaction(owner, ["styles"], "readwrite", tx => { tx.objectStore("styles").put(plain(style)); return () => undefined; });
  },
  async deleteStyle(owner: string, id: string) {
    return transaction(owner, ["styles"], "readwrite", tx => { tx.objectStore("styles").delete(id); return () => undefined; });
  },
  async saveComparison(owner: string, comparison: MusicComparisonNotes) {
    return transaction(owner, ["comparisons"], "readwrite", tx => { tx.objectStore("comparisons").put(plain(comparison)); return () => undefined; });
  },
  async saveSong(owner: string, song: MusicResult, blob?: Blob) {
    return transaction(owner, blob ? ["songs", "audio"] : ["songs"], "readwrite", tx => {
      tx.objectStore("songs").put(plain(song));
      if (blob) tx.objectStore("audio").put(blob, song.prompt_id);
      return () => undefined;
    });
  },
  async savePlan(owner: string, plan: MusicPlan) {
    return transaction(owner, ["plans"], "readwrite", tx => {
      tx.objectStore("plans").put(plain(plan));
      return () => undefined;
    });
  },
  async saveBatch(owner: string, batch: MusicBatch) {
    return transaction(owner, ["batches"], "readwrite", tx => {
      tx.objectStore("batches").put(plain(batch));
      return () => undefined;
    });
  },
  async savePlaylist(owner: string, playlist: MusicPlaylist) {
    return transaction(owner, ["playlists"], "readwrite", tx => {
      tx.objectStore("playlists").put(plain(playlist));
      return () => undefined;
    });
  },
  async deletePlaylist(owner: string, id: string) {
    return transaction(owner, ["playlists"], "readwrite", tx => {
      tx.objectStore("playlists").delete(id);
      return () => undefined;
    });
  },
};
