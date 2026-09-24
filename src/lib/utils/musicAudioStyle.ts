import { analyzeMusicAudioStyle, getMusicAudioStyle } from "./api.js";
import { blobBase64 } from "./musicAudio.js";

export interface AudioStyleCapabilities {
  available: boolean; backend_id?: string; model?: string; destination?: string; custom?: boolean; error?: string;
}
export interface AudioStyleTarget { instrumental: boolean; max_duration: number; language: string; source_start?: number; source_end?: number }
export interface AudioStyleProfile {
  description: string; style: string; estimates: string[]; audio_sha256: string; duration_seconds: number; backend_id: string;
  source_start_seconds?: number; source_end_seconds?: number; excerpt?: boolean; analysis_version?: number;
}
export interface AudioStyleStatus { status: "running" | "completed" | "error" | "cancelled"; profile?: AudioStyleProfile; error?: string; loudness_lufs?: number | null; duration_seconds?: number }
export function audioRangeValid(start: number, end: number, duration = 86400): boolean {
  return Number.isFinite(start) && Number.isFinite(end) && start >= 0 && end > start && end - start <= 360 && end <= Math.min(86400, duration) + 0.05;
}
export function audioStyleTargetKey(target: AudioStyleTarget): string {
  return JSON.stringify([target.instrumental, target.max_duration, target.language]);
}
export function validateAudioStyle(profile: AudioStyleProfile, backendId: string): AudioStyleProfile {
  if (!profile || profile.backend_id !== backendId || !/^[a-f0-9]{64}$/.test(profile.audio_sha256)
    || !Number.isFinite(profile.duration_seconds) || profile.duration_seconds <= 0 || profile.duration_seconds > 360.5
    || typeof profile.description !== "string" || !profile.description.trim() || [...profile.description].length > 2400
    || typeof profile.style !== "string" || !profile.style.trim() || [...profile.style].length > 1200 || /[\r\n]/.test(profile.style)
    || !Array.isArray(profile.estimates) || !profile.estimates.length || profile.estimates.length > 8
    || profile.estimates.some(s => typeof s !== "string" || !s.trim() || [...s].length > 500)
    || (profile.excerpt && !audioRangeValid(profile.source_start_seconds!, profile.source_end_seconds!))) throw new Error("invalid_audio_style");
  return profile;
}
/** A lost component, changed account or replaced source cancels the server's leased job. */
export async function describeAudio(blob: Blob, target: AudioStyleTarget, backendId: string, current: () => boolean, started: (id: string) => void): Promise<AudioStyleProfile | null> {
  if (!blob.size || blob.size > 64 * 1024 * 1024) throw new Error("music_audio_size");
  if ((target.source_start !== undefined || target.source_end !== undefined) && !audioRangeValid(target.source_start!, target.source_end!)) throw new Error("music_audio_range");
  const encoded = await blobBase64(blob);
  if (!current()) return null;
  const id = await analyzeMusicAudioStyle(encoded, target, backendId);
  started(id);
  let consumed = false;
  try {
    while (current()) {
      const status = await getMusicAudioStyle(id);
      if (!current()) return null;
      if (status.status === "completed") {
        consumed = true;
        return validateAudioStyle(status.profile!, backendId);
      }
      if (status.status === "error") { consumed = true; throw new Error(status.error || "invalid_audio_style"); }
      if (status.status === "cancelled") { consumed = true; return null; }
      await new Promise(resolve => setTimeout(resolve, 750));
    }
    return null;
  } finally {
    if (!consumed) await getMusicAudioStyle(id, true).catch(() => {});
  }
}
