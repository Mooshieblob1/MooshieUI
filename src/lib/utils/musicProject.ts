import { saveMusicFile } from "./api.js";
import { blobBase64 } from "./musicAudio.js";
import { isTauri } from "./ipc.js";
import type { MusicResult } from "../types/music.js";

export async function downloadMusicFile(blob: Blob, filename: string): Promise<"saved" | "downloaded" | "cancelled"> {
  if (blob.size > 256 * 1024 * 1024) throw new Error("music_export_limit");
  if (isTauri) {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const extension = filename.split(".").at(-1)!;
    const path = await save({ defaultPath: filename, filters: [{ name: extension.toUpperCase(), extensions: [extension] }] });
    if (!path) return "cancelled";
    await saveMusicFile(await blobBase64(blob), path);
    return "saved";
  }
  const url = URL.createObjectURL(blob), anchor = document.createElement("a");
  anchor.href = url; anchor.download = filename; document.body.appendChild(anchor);
  try { anchor.click(); } finally { anchor.remove(); setTimeout(() => URL.revokeObjectURL(url), 60_000); }
  return "downloaded";
}

const crcTable = Uint32Array.from({ length: 256 }, (_, value) => {
  for (let bit = 0; bit < 8; bit++) value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
  return value >>> 0;
});
async function crc32(blob: Blob) {
  let crc = 0xffffffff;
  let processed = 0;
  const reader = blob.stream().getReader();
  try { for (;;) {
    const { value, done } = await reader.read(); if (done) break;
    for (const byte of value) crc = crcTable[(crc ^ byte) & 255] ^ (crc >>> 8);
    processed += value.length;
    if (processed >= 4 * 1024 * 1024) { processed = 0; await new Promise(resolve => setTimeout(resolve, 0)); }
  } }
  finally { reader.releaseLock(); }
  return (crc ^ 0xffffffff) >>> 0;
}

/** Store-only ZIP: preserves FLAC bytes, no recompression or execution instructions. */
export async function musicZip(files: { name: string; blob: Blob }[]): Promise<Blob> {
  if (!files.length || files.length > 100 || files.reduce((sum,f) => sum + f.blob.size, 0) > 256 * 1024 * 1024 - 65536) throw new Error("music_export_limit");
  const parts: BlobPart[] = [], central: Uint8Array<ArrayBuffer>[] = [], names = new Set<string>();
  let offset = 0;
  for (const file of files) {
    if (!/^[a-zA-Z0-9._/-]+$/.test(file.name) || file.name.startsWith("/") || file.name.split("/").some(p => p === ".." || !p) || names.has(file.name)) throw new Error("Invalid project filename");
    names.add(file.name);
    const name = new TextEncoder().encode(file.name), crc = await crc32(file.blob);
    const header = new Uint8Array(30 + name.length), h = new DataView(header.buffer);
    h.setUint32(0, 0x04034b50, true); h.setUint16(4, 20, true); h.setUint16(6, 0x800, true);
    h.setUint16(12, 33, true); h.setUint32(14, crc, true); h.setUint32(18, file.blob.size, true); h.setUint32(22, file.blob.size, true); h.setUint16(26, name.length, true); header.set(name, 30);
    const directory = new Uint8Array(46 + name.length), d = new DataView(directory.buffer);
    d.setUint32(0, 0x02014b50, true); d.setUint16(4, 20, true); d.setUint16(6, 20, true); d.setUint16(8, 0x800, true); d.setUint16(14, 33, true);
    d.setUint32(16, crc, true); d.setUint32(20, file.blob.size, true); d.setUint32(24, file.blob.size, true); d.setUint16(28, name.length, true); d.setUint32(42, offset, true); directory.set(name, 46);
    parts.push(header, file.blob); central.push(directory); offset += header.length + file.blob.size;
  }
  const size = central.reduce((n,p) => n + p.length, 0), end = new Uint8Array(22), e = new DataView(end.buffer);
  e.setUint32(0, 0x06054b50, true); e.setUint16(8, files.length, true); e.setUint16(10, files.length, true); e.setUint32(12, size, true); e.setUint32(16, offset, true);
  return new Blob([...parts, ...central, end], { type: "application/zip" });
}

export async function musicProject(songs: MusicResult[], audio: (song: MusicResult) => Promise<Blob>): Promise<Blob> {
  if (!songs.length || songs.length > 8) throw new Error("music_export_limit");
  const files: { name: string; blob: Blob }[] = [];
  const manifest = { format: "mooshieui-music-project", version: 1, created_at: new Date().toISOString(),
    contents: "Original FLAC, request parameters, score and generation receipts. Does not include model weights, tokens, latents or a guarantee of identical regeneration.", versions: songs };
  files.push({ name: "project.json", blob: new Blob([JSON.stringify(manifest, null, 2)], { type: "application/json" }) });
  let total = files[0].blob.size;
  for (const [i, song] of songs.entries()) {
    const prefix = `version-${i+1}`;
    const blob = await audio(song); total += blob.size;
    if (total > 256 * 1024 * 1024 - 65536) throw new Error("music_export_limit");
    files.push({ name: `${prefix}/original.flac`, blob },
      { name: `${prefix}/score.abc`, blob: new Blob([song.abc]) },
      { name: `${prefix}/lyrics.txt`, blob: new Blob([song.params.lyrics]) },
      { name: `${prefix}/style.txt`, blob: new Blob([song.params.style]) },
      { name: `${prefix}/request.json`, blob: new Blob([JSON.stringify({ ...song.params, seed: song.seed }, null, 2)]) });
  }
  return musicZip(files);
}
