import type { MusicPlaylist, MusicResult } from "../types/music.js";

// Audio is stored separately so browsing a library never loads every recording.
// Each signed-in user gets a separate database on this browser/device.
function openLibrary(owner: string): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(`mooshieui.music.library.${owner}`, 1);
    request.onupgradeneeded = () => {
      const db = request.result;
      db.createObjectStore("songs", { keyPath: "prompt_id" });
      db.createObjectStore("audio");
      db.createObjectStore("playlists", { keyPath: "id" });
    };
    request.onsuccess = () => resolve(request.result);
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
    return transaction(owner, ["songs", "playlists"], "readonly", tx => {
      const songs = tx.objectStore("songs").getAll();
      const playlists = tx.objectStore("playlists").getAll();
      return () => ({ songs: songs.result as MusicResult[], playlists: playlists.result as MusicPlaylist[] });
    });
  },
  async audio(owner: string, id: string): Promise<Blob | undefined> {
    return transaction(owner, ["audio"], "readonly", tx => {
      const request = tx.objectStore("audio").get(id);
      return () => request.result;
    });
  },
  async saveSong(owner: string, song: MusicResult, blob?: Blob) {
    return transaction(owner, blob ? ["songs", "audio"] : ["songs"], "readwrite", tx => {
      tx.objectStore("songs").put(plain(song));
      if (blob) tx.objectStore("audio").put(blob, song.prompt_id);
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
