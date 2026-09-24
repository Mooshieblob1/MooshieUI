import { getMusicAudioStyle, measureMusicLoudness } from "./api.js";
import { blobBase64 } from "./musicAudio.js";
import type { MusicParams } from "../types/music.js";
export const comparisonCriteria = ["style", "cleanliness", "arrangement", "ending"] as const;
export type ComparisonCriterion = typeof comparisonCriteria[number];
export interface MusicComparisonNotes {
  id: string; song_ids: [string, string]; votes: Partial<Record<ComparisonCriterion, string>>; notes: string; updatedAt: number;
}
export const comparisonId = (a: string, b: string) => JSON.stringify([a, b].sort());
export function readComparisonNotes(value: MusicComparisonNotes): MusicComparisonNotes {
  if (!value || value.song_ids?.length !== 2 || value.song_ids.some(id => typeof id !== "string" || !id || id.length > 200)
    || value.song_ids[0] === value.song_ids[1] || value.id !== comparisonId(...value.song_ids)
    || typeof value.notes !== "string" || value.notes.length > 2000 || !Number.isFinite(value.updatedAt)) throw new Error("invalid_music_comparison");
  const votes: MusicComparisonNotes["votes"] = {};
  for (const key of comparisonCriteria) {
    const vote = value.votes?.[key];
    if (vote && vote !== "tie" && !value.song_ids.includes(vote)) throw new Error("invalid_music_comparison");
    if (vote) votes[key] = vote;
  }
  return { id: value.id, song_ids: [...value.song_ids], votes, notes: value.notes, updatedAt: value.updatedAt };
}
/** Match to the quieter recording; attenuation only, with no extra clipping. */
export function comparisonGains(levels: (number | null)[]): [number, number] | null {
  if (levels.length !== 2 || levels.some(n => n === null || !Number.isFinite(n) || n <= -69.9 || n > 5)) return null;
  const target = Math.min(...levels as number[]);
  return levels.map(n => Math.min(1, 10 ** ((target - n!) / 20))) as [number, number];
}
export function comparisonDifferences(a: MusicParams, b: MusicParams): string[] {
  return (["checkpoint", "style", "lyrics", "abc", "planning", "max_duration", "steps", "seed", "sampling"] as const)
    .filter(key => JSON.stringify(a[key]) !== JSON.stringify(b[key]));
}
export function comparisonSeek(time: number, durationA: number, durationB: number): number {
  const limit = Math.min(...[durationA, durationB].filter(n => Number.isFinite(n) && n > 0));
  return Math.max(0, Math.min(Number.isFinite(time) ? time : 0, Number.isFinite(limit) ? Math.max(0, limit - 0.05) : 0));
}
export async function measurePlaybackLoudness(blob: Blob, current: () => boolean, started: (id: string) => void): Promise<number | null> {
  if (!blob.size || blob.size > 64 * 1024 * 1024) throw new Error("music_audio_size");
  const encoded = await blobBase64(blob);
  if (!current()) return null;
  const id = await measureMusicLoudness(encoded);
  started(id);
  let consumed = false;
  try {
    while (current()) {
      const status = await getMusicAudioStyle(id);
      if (!current()) return null;
      if (status.status === "completed") {
        consumed = true;
        if (status.loudness_lufs === null) return null;
        if (typeof status.loudness_lufs !== "number" || !Number.isFinite(status.loudness_lufs) || status.loudness_lufs < -70 || status.loudness_lufs > 5) throw new Error("invalid_music_loudness");
        return status.loudness_lufs;
      }
      if (status.status === "error") { consumed = true; throw new Error(status.error || "invalid_music_loudness"); }
      if (status.status === "cancelled") { consumed = true; return null; }
      await new Promise(resolve => setTimeout(resolve, 750));
    }
    return null;
  } finally { if (!consumed) await getMusicAudioStyle(id, true).catch(() => {}); }
}
