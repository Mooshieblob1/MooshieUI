import type { SplatResult } from "../types/model3d.js";

// One database per signed-in user on this browser/device, like the music
// library. PLY files are stored apart from the records so listing the library
// never loads every model. The "ply" store also holds the .glb of mesh
// (Pixal3D) results; the record's `kind` says which.
function openLibrary(owner: string): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(`mooshieui.model3d.library.${owner}`, 1);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains("results")) db.createObjectStore("results", { keyPath: "id" });
      if (!db.objectStoreNames.contains("ply")) db.createObjectStore("ply");
    };
    request.onsuccess = () => { request.result.onversionchange = () => request.result.close(); resolve(request.result); };
    request.onerror = () => reject(request.error);
    request.onblocked = () => reject(new Error("The 3D library is open in an older app window"));
  });
}

async function transaction<T>(owner: string, stores: string[], mode: IDBTransactionMode, run: (tx: IDBTransaction) => () => T): Promise<T> {
  const db = await openLibrary(owner);
  try {
    return await new Promise<T>((resolve, reject) => {
      const tx = db.transaction(stores, mode);
      const result = run(tx);
      tx.oncomplete = () => resolve(result());
      tx.onabort = () => reject(tx.error ?? new Error("3D library transaction aborted"));
      tx.onerror = () => reject(tx.error);
    });
  } finally { db.close(); }
}

// Strip Svelte proxies before structured cloning into IndexedDB.
const plain = <T>(value: T): T => JSON.parse(JSON.stringify(value));

export const splatLibrary = {
  async load(owner: string): Promise<SplatResult[]> {
    return transaction(owner, ["results"], "readonly", (tx) => {
      const all = tx.objectStore("results").getAll();
      return () => (all.result as SplatResult[]).sort((a, b) => b.createdAt - a.createdAt);
    });
  },
  async ply(owner: string, id: string): Promise<Blob | undefined> {
    return transaction(owner, ["ply"], "readonly", (tx) => {
      const request = tx.objectStore("ply").get(id);
      return () => request.result as Blob | undefined;
    });
  },
  async save(owner: string, result: SplatResult, ply: Blob) {
    return transaction(owner, ["results", "ply"], "readwrite", (tx) => {
      tx.objectStore("results").put(plain(result));
      tx.objectStore("ply").put(ply, result.id);
      return () => undefined;
    });
  },
  async rename(owner: string, result: SplatResult) {
    return transaction(owner, ["results"], "readwrite", (tx) => {
      tx.objectStore("results").put(plain(result));
      return () => undefined;
    });
  },
  async remove(owner: string, id: string) {
    return transaction(owner, ["results", "ply"], "readwrite", (tx) => {
      tx.objectStore("results").delete(id);
      tx.objectStore("ply").delete(id);
      return () => undefined;
    });
  },
};
