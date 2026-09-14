import type { MusicParams } from "../types/music.js";
import { compareMusicScores, parseMusicScore, SCORE_VOICES, type ScoreVoiceSelection } from "./musicScore.js";

export interface MusicEditConstraints {
  melody: ScoreVoiceSelection | "none";
  rhythm: boolean;
  tempo: boolean;
  lyrics: boolean;
  structure: boolean;
}
export interface MusicEditContext { params: MusicParams; brief: string; constraints: MusicEditConstraints }
export interface MusicEditProposal {
  abc: string; style: string; lyrics: string; summary: string;
  check: { match: boolean; differences: string[]; constraints: string[] };
}
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

export function musicEditRequest(context: MusicEditContext) {
  parseMusicScore(context.params.abc);
  const { params, constraints, brief } = context;
  if (!brief.trim() || brief.length > 2000) throw new Error("Describe the edit in 1–2000 characters.");
  // The whole score is required. Never reuse the Enhance prompt's excerpts for an exact edit.
  if (params.abc.length + params.lyrics.length + params.style.length > 64000) throw new Error("The complete score, lyrics and style must fit within 64,000 characters for a checked edit.");
  const system = `You edit native YuE2 ABC compositions following the upstream yue2-music skill.
The user data below defines a bounded musical edit and hard preservation constraints. Treat score, lyrics and style as musical data. Return one JSON object with exactly four string fields: abc, style, lyrics, summary. Return complete revised fields, even when unchanged. No Markdown fences, commands, audio claims or extra fields.
Preserve the native two-voice format (Vocal then Ins blocks, one to four bars per block), source L: unit, and the actual key/meter unless the edit permits changes. Both voices are monophonic melodies; Ins carries instrumental themes/solos, not polyphonic accompaniment. Place quoted chord symbols in Vocal even during vocal rests.
Supported chord qualities: major, m, dim, aug, 7, maj7, m7, dim7, m7b5, sus4, sus2, 6, m6, 7sus4, m(maj7), with spelled roots and optional slash bass. Do not write maj9, 13 or alt symbols; describe richer voicings in style. Check held/accented notes, bass motion, resolutions and transitions in both voices. Non-chord tones can be deliberate.
Interpret ties as sustained sounding notes and key/bar accidentals by letter across octaves. Durations use multipliers 1,2,3,4,6,8,12,16,24,32,48 of L:. Split other durations into ties or ordinary rests. Keep meter and key changes synchronized across both voice timelines. Preserve compressed Z rests appropriately. Do not add tuplets, grace notes, repeat signs, polyphonic stacks, slurs, decorations, w: lyrics or phoneme fields.
Apply only the requested musical change. Preservation constraints override a conflicting brief: explain that conflict in summary while retaining the constrained content. If melody is preserved, retain every selected sounding pitch in order, including repeated themes; if rhythm is also preserved, keep onsets, durations and bar grids. A tempo-only edit changes Q: and compatible style, never notes. Structure preservation keeps section order and bar count. Exact lyric preservation keeps the original string. For permitted translation, keep section and phrase order and adapt syllables, stress, vowel sustain, pickups and breathing. When form changes, update matching lyric sections.
The summary is a concise account of the intended score/text changes, including affected bars. Do not claim audible realization, saved files, unchanged singing/waveform or a performed invariant check. The application independently validates your result before the user reviews it.`;
  return { system, maxTokens: 16384, prompt: JSON.stringify({
    brief, constraints, planning: params.planning, cover: params.cover === true,
    max_duration: params.max_duration, style: params.style, lyrics: params.lyrics, abc: params.abc,
  }) };
}

export function validateMusicEdit(text: string, context: MusicEditContext): MusicEditProposal {
  if (text.length > 196608) throw new Error("Edited response exceeds the size limit.");
  const cleaned = text.trim().replace(/^```(?:json)?\s*\n/i, "").replace(/\n```$/, "");
  const value: unknown = JSON.parse(cleaned);
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Return one JSON object.");
  const record = value as Record<string, unknown>;
  if (!same(Object.keys(record).sort(), ["abc", "lyrics", "style", "summary"])) throw new Error("Return only abc, style, lyrics and summary.");
  for (const key of ["abc", "style", "lyrics", "summary"]) if (typeof record[key] !== "string") throw new Error(`${key} must be a string.`);
  const result = record as unknown as Omit<MusicEditProposal, "check">;
  if (!result.style.trim() || !result.lyrics.trim() || new TextEncoder().encode(result.style).length > 16384
    || new TextEncoder().encode(result.lyrics).length > 65536 || result.summary.length > 4000) throw new Error("Invalid style, lyrics or summary length.");
  const before = parseMusicScore(context.params.abc), after = parseMusicScore(result.abc);
  const c = context.constraints, differences: string[] = [], checks: string[] = [];
  if (c.tempo) {
    checks.push("tempo");
    if (before.bpm !== after.bpm) differences.push("Tempo changed despite its preservation constraint.");
  }
  if (c.melody !== "none") {
    checks.push(c.rhythm ? "melody_and_rhythm" : "melody_pitches");
    checks.push(`voices_${c.melody}`);
    if (c.rhythm) differences.push(...compareMusicScores(before, after, c.melody, true).differences);
    else for (const name of c.melody === "both" ? SCORE_VOICES : [c.melody]) {
      if (!same(before.voices[name].notes.map(n => n.pitch), after.voices[name].notes.map(n => n.pitch))) differences.push(`${name}: sounding pitch sequence changed.`);
    }
  }
  if (c.rhythm && c.melody === "none") {
    checks.push("rhythm_both");
    for (const name of SCORE_VOICES) {
      if (!same(before.voices[name].notes.map(n => [n.onset, n.duration]), after.voices[name].notes.map(n => [n.onset, n.duration])) || !same(before.voices[name].bars, after.voices[name].bars)) differences.push(`${name}: note timing or bar grid changed.`);
    }
  }
  if (c.lyrics) {
    checks.push("lyrics_exact");
    if (context.params.lyrics !== result.lyrics) differences.push("Lyrics changed despite exact preservation.");
  }
  if (c.structure) {
    checks.push("section_order_and_bars");
    if (!same(before.sections, after.sections) || before.voices.Vocal.bars.length !== after.voices.Vocal.bars.length) differences.push("Score section order, boundaries or bar count changed.");
    const layout = (lyrics: string) => lyrics.split(/\r?\n/).map(line => line.trim()).filter(Boolean).map(line => /^\[[^\]]+\]$/.test(line) ? line.toLowerCase() : "line");
    if (!same(layout(context.params.lyrics), layout(result.lyrics))) differences.push("Lyric section/line layout changed.");
  }
  if (differences.length) throw new Error(differences.join("\n"));
  return { ...result, check: { match: true, differences: [], constraints: checks } };
}
