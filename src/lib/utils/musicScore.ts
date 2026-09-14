/**
 * Bounded YuE2/SheetSage2 ABC interpreter. Musical semantics adapted from
 * yue2-music/scripts/abc_tools.py,
 * Apache-2.0. Upstream revision: 88da114a67df892af0329472073b96a5ef700b93.
 * See third-party/yue2-music-LICENSE.txt. This is not a general ABC parser.
 * Durations use exact binary fractions (power-of-two units up to 1/1024).
 */
export const SCORE_VOICES = ["Vocal", "Ins"] as const;
export type ScoreVoiceName = typeof SCORE_VOICES[number];
export type ScoreVoiceSelection = "both" | ScoreVoiceName;
export interface ScoreNote { onset: number; pitch: number; duration: number }
export interface ScoreBar { onset: number; duration: number; meter: [number, number] }
export interface ScoreChord { onset: number; symbol: string }
export interface ScoreVoice {
  notes: ScoreNote[]; bars: ScoreBar[]; chords: ScoreChord[];
  keys: { onset: number; key: string }[];
}
export interface MusicScore {
  text: string; bpm: number; unit: number; key: string; quarters: number; seconds: number;
  voices: Record<ScoreVoiceName, ScoreVoice>;
  sections: { label: string; onset: number }[];
  musicLines: Record<number, ScoreVoiceName>;
}
export interface ScoreComparison { match: boolean; differences: string[] }

const durations = new Set([1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48]);
const natural: Record<string, number> = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };
const major = ["Cb", "Gb", "Db", "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#"];
const minor = ["Abm", "Ebm", "Bbm", "Fm", "Cm", "Gm", "Dm", "Am", "Em", "Bm", "F#m", "C#m", "G#m", "D#m", "A#m"];
const keys = Object.fromEntries([...major.map((key, i) => [key, i - 7]), ...minor.map((key, i) => [key, i - 7])]) as Record<string, number>;
export const SCORE_KEYS = [...major, ...minor];
const chordPattern = /^[A-G](?:bb|##|b|#)?(?:m\(maj7\)|maj7|m7b5|dim7|7sus4|sus4|sus2|dim|aug|m7|m6|m|7|6)?(?:\/[A-G](?:bb|##|b|#)?)?$/;
const tokenPattern = /^(?:"(?<chord>[^"\n]*)"|\[K:(?<key>[^\]\n]+)\]|(?<acc>\^\^|__|\^|_|=)?(?<note>[A-Ga-gz])(?<oct>[,']*)(?<duration>[0-9]*)(?<tie>-?))/;

function requireScore(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}
function keyAccidentals(key: string) {
  requireScore(Object.hasOwn(keys, key), `Unsupported key ${key}; use a standard major or minor key.`);
  const count = keys[key];
  const result: Record<string, number> = Object.fromEntries(Object.keys(natural).map(n => [n, 0]));
  for (const n of (count > 0 ? "FCGDAEB" : "BEADGCF").slice(0, Math.abs(count))) result[n] = count > 0 ? 1 : -1;
  return result;
}
function meterValue(text: string): [number, number] {
  const match = /^([1-9][0-9]*)\/([1-9][0-9]*)$/.exec(text);
  requireScore(match, `Unsupported meter ${text}; use a fraction.`);
  const n = Number(match[1]), d = Number(match[2]);
  requireScore(n <= 32 && d <= 1024 && !(d & (d - 1)), `Unsupported meter ${text}.`);
  return [n, d];
}
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

export function parseMusicScore(text: string): MusicScore {
  requireScore(new TextEncoder().encode(text).length <= 131072, "Score exceeds 128 KiB.");
  // Harmless whitespace/header spelling is normalized; musical tokens are not guessed.
  const lines = text.replace(/\r\n?/g, "\n").trim().split("\n");
  requireScore(lines.length >= 12, "Expected a native score with Vocal and Ins voices.");
  requireScore(/^X:\s*1$/.test(lines[0]) && /^T:/.test(lines[1]), "Expected X:1 and T: headers.");
  requireScore(lines[2].startsWith("M:"), "Missing M: header.");
  const meter = meterValue(lines[2].slice(2).trim());
  const unitMatch = /^L:\s*1\/([1-9][0-9]*)$/.exec(lines[3]);
  const denominator = Number(unitMatch?.[1]);
  requireScore(unitMatch && denominator <= 1024 && !(denominator & (denominator - 1)), "Expected L:1/<power of two>, up to 1/1024.");
  const tempoMatch = /^Q:\s*1\/4=([1-9][0-9]*)$/.exec(lines[4]);
  const bpm = Number(tempoMatch?.[1]);
  requireScore(tempoMatch && bpm <= 1000, "Expected quarter-note tempo Q:1/4=<BPM>, up to 1000.");
  requireScore(/^V:\s*Vocal(?:\s|$)/.test(lines[5]) && /^V:\s*Ins(?:\s|$)/.test(lines[6]), "Define the native Vocal and Ins voices before K:.");
  requireScore(lines[7].startsWith("K:"), "Missing K: header.");
  const key = lines[7].slice(2).trim();
  keyAccidentals(key);
  type State = ScoreVoice & { time: number; meter: [number, number]; key: string; pending: [number, number] | null };
  const makeVoice = (): State => ({ notes: [], bars: [], chords: [], keys: [{ onset: 0, key }], time: 0, meter: [...meter], key, pending: null });
  const voices: Record<ScoreVoiceName, State> = { Vocal: makeVoice(), Ins: makeVoice() };
  const musicLines: Record<number, ScoreVoiceName> = {};
  const sections: MusicScore["sections"] = [];
  let cursor = 8, group = 0;
  const parseBar = (body: string, voice: State, context: string) => {
    const length = 4 * voice.meter[0] / voice.meter[1];
    let offset = 0;
    const local: Record<string, number> = {};
    if (body === "Z") {
      requireScore(!voice.pending, `${context}: tie enters a resting measure.`);
      offset = length;
    } else {
      let pos = 0;
      while (pos < body.length) {
        if (/\s/.test(body[pos])) { pos++; continue; }
        const match = tokenPattern.exec(body.slice(pos));
        requireScore(match, `${context}: unsupported notation ${body.slice(pos, pos + 20)}.`);
        pos += match[0].length;
        requireScore(offset < length, `${context}: event after measure end.`);
        const t = match.groups!;
        if (t.chord !== undefined) {
          requireScore(chordPattern.test(t.chord), `${context}: unsupported chord ${t.chord}.`);
          voice.chords.push({ onset: voice.time + offset, symbol: t.chord });
          continue;
        }
        if (t.key !== undefined) {
          keyAccidentals(t.key); voice.key = t.key;
          voice.keys.push({ onset: voice.time + offset, key: t.key });
          for (const k of Object.keys(local)) delete local[k];
          continue;
        }
        const units = Number(t.duration || "1");
        requireScore(durations.has(units), `${context}: unsupported duration ${units}; use tied supported lengths.`);
        const duration = units * 4 / denominator;
        requireScore(offset + duration <= length, `${context}: note or rest exceeds the measure.`);
        requireScore(!(t.oct.includes(",") && t.oct.includes("'")), `${context}: mixed octave marks.`);
        if (t.note === "z") {
          requireScore(!t.acc && !t.oct && !t.tie && !voice.pending, `${context}: invalid rest or tie into rest.`);
        } else {
          const letter = t.note.toUpperCase();
          const written = 60 + natural[letter] + (t.note === t.note.toLowerCase() ? 12 : 0)
            + 12 * ([...t.oct].filter(c => c === "'").length - [...t.oct].filter(c => c === ",").length);
          let alteration = local[letter] ?? keyAccidentals(voice.key)[letter];
          if (t.acc) { alteration = ({ "=": 0, "_": -1, "__": -2, "^": 1, "^^": 2 } as Record<string, number>)[t.acc]; local[letter] = alteration; }
          let pitch = written + alteration;
          if (voice.pending) {
            if (!t.acc && written === voice.pending[1]) pitch = voice.pending[0];
            requireScore(pitch === voice.pending[0], `${context}: tie changes sounding pitch.`);
            voice.notes[voice.notes.length - 1].duration += duration;
          } else {
            requireScore(pitch >= 0 && pitch <= 127, `${context}: pitch outside MIDI range.`);
            voice.notes.push({ onset: voice.time + offset, pitch, duration });
          }
          voice.pending = t.tie ? [pitch, written] : null;
        }
        offset += duration;
      }
    }
    requireScore(offset === length, `${context}: ${offset} quarter notes, expected ${length}.`);
    voice.bars.push({ onset: voice.time, duration: length, meter: [...voice.meter] });
    voice.time += length;
  };
  while (cursor < lines.length) {
    while (cursor < lines.length && (!lines[cursor].trim() || lines[cursor].startsWith("%"))) {
      if (lines[cursor].startsWith("%")) sections.push({ label: lines[cursor].slice(1).trim(), onset: voices.Vocal.time });
      cursor++;
    }
    requireScore(cursor < lines.length, "Section marker has no music.");
    group++;
    const counts: number[] = [];
    for (const name of SCORE_VOICES) {
      const voice = voices[name], context = `Group ${group}, ${name}`;
      requireScore(lines[cursor]?.trim().replace(/^V:\s*/, "V: ") === `V: ${name}`, `${context}: expected V: ${name}.`);
      cursor++;
      const fields = new Set<string>();
      while (/^[MK]:/.test(lines[cursor] ?? "")) {
        const field = lines[cursor][0], value = lines[cursor].slice(2).trim();
        requireScore(!fields.has(field), `${context}: duplicate ${field}: field.`);
        fields.add(field);
        if (field === "M") voice.meter = meterValue(value);
        else { keyAccidentals(value); voice.key = value; voice.keys.push({ onset: voice.time, key: value }); }
        cursor++;
      }
      const line = lines[cursor]?.trim();
      requireScore(line?.endsWith("|"), `${context}: end the music line with a plain barline.`);
      musicLines[cursor++] = name;
      const bars = line.slice(0, -1).split("|").flatMap(bar => {
        bar = bar.trim();
        requireScore(bar, `${context}: empty or repeated barline is unsupported.`);
        const rest = /^Z([2-4])?$/.exec(bar);
        return rest ? Array<string>(Number(rest[1] ?? 1)).fill("Z") : [bar];
      });
      requireScore(bars.length >= 1 && bars.length <= 4, `${context}: use 1–4 measures per voice block.`);
      counts.push(bars.length);
      for (const bar of bars) parseBar(bar, voice, `${context}, bar ${voice.bars.length + 1}`);
    }
    requireScore(counts[0] === counts[1], `Group ${group}: voices have different measure counts.`);
  }
  for (const name of SCORE_VOICES) requireScore(!voices[name].pending, `${name}: unresolved tie at end.`);
  requireScore(!voices.Ins.chords.length, "Native chord symbols belong in Vocal, not Ins.");
  requireScore(same(voices.Vocal.bars, voices.Ins.bars), "Voice meter/time grids differ.");
  requireScore(same(voices.Vocal.keys, voices.Ins.keys), "Voice key changes differ.");
  const clean = (v: State): ScoreVoice => ({ notes: v.notes, bars: v.bars, chords: v.chords, keys: v.keys });
  return { text: lines.join("\n") + "\n", bpm, unit: 1 / denominator, key,
    quarters: voices.Vocal.time, seconds: voices.Vocal.time * 60 / bpm,
    voices: { Vocal: clean(voices.Vocal), Ins: clean(voices.Ins) }, musicLines, sections };
}

export function inspectMusicScore(text: string): { score: MusicScore | null; error: string } {
  try { return { score: parseMusicScore(text), error: "" }; }
  catch (error) { return { score: null, error: error instanceof Error ? error.message : String(error) }; }
}

export function compareMusicScores(before: MusicScore, after: MusicScore, voices: ScoreVoiceSelection = "both", allowTempo = false): ScoreComparison {
  const differences: string[] = [];
  if (!allowTempo && before.bpm !== after.bpm) differences.push("Quarter-note tempo changed.");
  for (const name of voices === "both" ? SCORE_VOICES : [voices]) {
    const a = before.voices[name], b = after.voices[name];
    if (!same(a.bars, b.bars)) differences.push(`${name}: measure timing or meter changed.`);
    if (!same(a.notes, b.notes)) {
      let first = 0;
      while (first < Math.min(a.notes.length, b.notes.length) && same(a.notes[first], b.notes[first])) first++;
      differences.push(`${name}: pitch, onset or duration changed at sounding note ${first + 1}.`);
    }
  }
  return { match: differences.length === 0, differences };
}

/** Retain native time grids and selected melodies; optionally retain harmony. */
export function prepareCoverScore(text: string, voice: ScoreVoiceSelection = "both", preserveHarmony = false): string {
  const before = parseMusicScore(text);
  const lines = before.text.trimEnd().split("\n");
  const tokens = new RegExp(tokenPattern.source.slice(1), "g");
  for (const [index, name] of Object.entries(before.musicLines)) {
    lines[Number(index)] = lines[Number(index)].replace(tokens, (...args: unknown[]) => {
      const groups = args[args.length - 1] as Record<string, string>;
      if (groups.chord !== undefined) return preserveHarmony ? String(args[0]) : "";
      if (voice !== "both" && name !== voice && groups.note !== undefined) return `z${groups.duration}`;
      return String(args[0]);
    });
  }
  const output = lines.join("\n") + "\n";
  const after = parseMusicScore(output), check = compareMusicScores(before, after, voice);
  requireScore(check.match, check.differences.join(" "));
  return output;
}

export function setScoreTempo(text: string, bpm: number): string {
  requireScore(Number.isInteger(bpm) && bpm >= 20 && bpm <= 400, "Choose a tempo from 20 to 400 BPM.");
  const before = parseMusicScore(text), output = before.text.replace(/^Q:.*$/m, `Q:1/4=${bpm}`);
  requireScore(compareMusicScores(before, parseMusicScore(output), "both", true).match, "Tempo change altered notes.");
  return output;
}

/** Standard MIDI type 1, exact sounding-note timing. Melody parts, not audio stems. */
export function scoreMidi(score: MusicScore, selected: ScoreVoiceSelection = "both", chords = false): Uint8Array {
  const ppq = 256;
  requireScore(score.quarters * ppq <= 0x0fffffff, "Score exceeds MIDI's timing range.");
  const ascii = (s: string) => [...s].map(c => c.charCodeAt(0));
  const u32 = (n: number) => [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255];
  const vlq = (n: number) => { const bytes = [n & 127]; while ((n = Math.floor(n / 128)) > 0) bytes.unshift((n & 127) | 128); return bytes; };
  const track = (data: number[]) => [...ascii("MTrk"), ...u32(data.length), ...data];
  const tempo = Math.round(60_000_000 / score.bpm);
  requireScore(tempo <= 0xffffff, "MIDI requires a tempo of at least 4 BPM.");
  const meta = [0, 0xff, 0x51, 3, (tempo >>> 16) & 255, (tempo >>> 8) & 255, tempo & 255];
  let metaTick = 0;
  for (const [index, bar] of score.voices.Vocal.bars.entries()) {
    const previous = score.voices.Vocal.bars[index - 1];
    if (previous && same(previous.meter, bar.meter)) continue;
    const tick = bar.onset * ppq;
    meta.push(...vlq(tick - metaTick), 0xff, 0x58, 4, bar.meter[0], Math.log2(bar.meter[1]), 24, 8);
    metaTick = tick;
  }
  meta.push(...vlq(score.quarters * ppq - metaTick), 0xff, 0x2f, 0);
  const parts: { name: string; notes: ScoreNote[] }[] = (selected === "both" ? [...SCORE_VOICES] : [selected]).map(name => ({ name, notes: score.voices[name].notes }));
  if (chords) parts.push({ name: "Chords", notes: scoreChordNotes(score) });
  const tracks = [track(meta)];
  parts.forEach(({ name, notes }, channel) => {
    const events = notes.flatMap(n => [
      { tick: n.onset * ppq, on: true, pitch: n.pitch },
      { tick: (n.onset + n.duration) * ppq, on: false, pitch: n.pitch },
    ]).sort((a, b) => a.tick - b.tick || Number(a.on) - Number(b.on));
    const data = [0, 0xff, 3, name.length, ...ascii(name), 0, 0xc0 | channel, 0];
    let tick = 0;
    for (const event of events) {
      data.push(...vlq(event.tick - tick), (event.on ? 0x90 : 0x80) | channel, event.pitch, event.on ? 80 : 0);
      tick = event.tick;
    }
    data.push(...vlq(score.quarters * ppq - tick), 0xff, 0x2f, 0);
    tracks.push(track(data));
  });
  return Uint8Array.from([...ascii("MThd"), 0, 0, 0, 6, 0, 1, 0, tracks.length, 1, 0, ...tracks.flat()]);
}

/** Basic close-position chord voicings for reference only, not generated accompaniment. */
export function scoreChordNotes(score: MusicScore): ScoreNote[] {
  const qualities: Record<string, number[]> = { "": [0,4,7], m:[0,3,7], dim:[0,3,6], aug:[0,4,8], "7":[0,4,7,10], maj7:[0,4,7,11], m7:[0,3,7,10], dim7:[0,3,6,9], m7b5:[0,3,6,10], sus4:[0,5,7], sus2:[0,2,7], "6":[0,4,7,9], m6:[0,3,7,9], "7sus4":[0,5,7,10], "m(maj7)":[0,3,7,11] };
  const pitchClass = (letter: string, accidental = "") => ({ C:0,D:2,E:4,F:5,G:7,A:9,B:11 }[letter]! + [...accidental].reduce((sum,c) => sum + (c === "#" ? 1 : -1), 0) + 12) % 12;
  return score.voices.Vocal.chords.flatMap((chord, index, all) => {
    const match = /^([A-G])(bb|##|b|#)?(.*?)(?:\/([A-G])(bb|##|b|#)?)?$/.exec(chord.symbol);
    if (!match || !qualities[match[3]]) return [];
    const root = pitchClass(match[1], match[2]);
    const pitches = qualities[match[3]].map(interval => 48 + root + interval);
    pitches.unshift(36 + (match[4] ? pitchClass(match[4], match[5]) : root));
    return pitches.map(pitch => ({ pitch, onset: chord.onset, duration: (all[index + 1]?.onset ?? score.quarters) - chord.onset })).filter(note => note.duration > 0);
  });
}
