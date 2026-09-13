import { saveMusicAudio } from "./api.js";
import { isTauri } from "./ipc.js";

export function audioTime(seconds: number): string {
  const total = Number.isFinite(seconds) ? Math.max(0, Math.floor(seconds)) : 0;
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

export function songTitle(lyrics: string): string {
  return lyrics.split(/\r?\n/).map(line => line.replace(/\[[^\]]*\]/g, "").trim())
    .find(Boolean)?.slice(0, 100) ?? "";
}

export function blobBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).split(",")[1]);
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}

/** A compact envelope of the actual audio, including both stereo channels. */
export function audioPeaks(channels: Float32Array[], count = 96): number[] {
  const length = channels[0]?.length ?? 0;
  if (!length || count <= 0) return [];
  const peaks = Array.from({ length: count }, (_, index) => {
    const start = Math.floor(index * length / count);
    const end = Math.max(start + 1, Math.floor((index + 1) * length / count));
    const stride = Math.max(1, Math.floor((end - start) / 2048));
    let peak = 0;
    for (const channel of channels) {
      for (let i = start; i < end; i += stride) peak = Math.max(peak, Math.abs(channel[i] ?? 0));
    }
    return peak;
  });
  const maximum = Math.max(...peaks, 0.001);
  return peaks.map(peak => peak / maximum);
}

export async function exportFlac(audioBase64: string, blob: Blob, filename: string): Promise<"saved" | "downloaded" | "cancelled"> {
  if (isTauri) {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({ defaultPath: filename, filters: [{ name: "FLAC", extensions: ["flac"] }] });
    if (!path) return "cancelled";
    await saveMusicAudio(audioBase64, path);
    return "saved";
  }
  // Own this URL separately from playback: changing songs must not revoke a
  // download in flight. Keep it alive long enough for the browser to consume it.
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  try { anchor.click(); }
  finally {
    anchor.remove();
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
  }
  return "downloaded";
}
