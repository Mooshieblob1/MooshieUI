import type { MusicParams, MusicSampling } from "../types/music.js";

export const defaultMusicSampling = (): MusicSampling => ({
  temperature: 1, top_p: 0.95, top_k: 100, repetition_penalty: 1.2,
  cfg_scale: -1, max_abc_tokens: 8192,
});

export function readMusicSampling(value: unknown): MusicSampling {
  const result = defaultMusicSampling();
  if (!value || typeof value !== "object") return result;
  const record = value as Record<string, unknown>;
  const bounds: Record<keyof MusicSampling, [number, number]> = {
    temperature: [0, 5], top_p: [0.01, 1], top_k: [1, 32768], repetition_penalty: [0.01, 10],
    cfg_scale: [-1, 20], max_abc_tokens: [1, 20000],
  };
  for (const key of Object.keys(bounds) as (keyof MusicSampling)[]) {
    const n = record[key];
    if (typeof n === "number" && Number.isFinite(n) && n >= bounds[key][0] && n <= bounds[key][1]
      && (!(key === "top_k" || key === "max_abc_tokens") || Number.isInteger(n))
      && (key !== "cfg_scale" || n === -1 || n >= 0)) result[key] = n;
  }
  return result;
}

export function cloneMusicParams(params: MusicParams): MusicParams {
  return JSON.parse(JSON.stringify(params));
}

export function musicSeed(): string {
  const parts = new Uint32Array(2);
  crypto.getRandomValues(parts);
  return ((BigInt(parts[0] & 0x7fffffff) << 32n) | BigInt(parts[1])).toString();
}

export function musicId(): string {
  return crypto.randomUUID?.() ?? [...crypto.getRandomValues(new Uint8Array(16))].map(n => n.toString(16).padStart(2, "0")).join("");
}
