import { scoreChordNotes, SCORE_VOICES, type MusicScore, type ScoreVoiceSelection } from "./musicScore.js";

let activePreview: (() => void) | null = null;
export function stopScorePreview() { activePreview?.(); }

/** Bounded look-ahead synthesis: no model downloads and no scheduling an entire song at once. */
export async function playScore(score: MusicScore, selected: ScoreVoiceSelection, chords: boolean, progress: (seconds: number) => void, ended: () => void): Promise<() => void> {
  stopScorePreview();
  const context = new AudioContext();
  try { await context.resume(); } catch (error) { void context.close(); throw error; }
  const notes = (selected === "both" ? SCORE_VOICES : [selected]).flatMap((name, part) => score.voices[name].notes.map(n => ({ ...n, part })))
    .concat(chords ? scoreChordNotes(score).map(n => ({ ...n, part: 2 })) : []).sort((a,b) => a.onset - b.onset);
  const start = context.currentTime + 0.06, beat = 60 / score.bpm;
  let index = 0, stopped = false;
  const stop = () => { if (stopped) return; stopped = true; clearInterval(timer); void context.close(); if (activePreview === stop) activePreview = null; ended(); };
  const pump = () => {
    const elapsed = context.currentTime - start;
    progress(Math.max(0, elapsed));
    while (index < notes.length && notes[index].onset * beat < elapsed + 0.8) {
      const note = notes[index++];
      const at = start + note.onset * beat, end = at + note.duration * beat;
      if (end <= context.currentTime) continue;
      const osc = context.createOscillator(), gain = context.createGain();
      const now = Math.max(at, context.currentTime), duration = end - now;
      osc.type = note.part === 1 ? "triangle" : "sine";
      osc.frequency.value = 440 * 2 ** ((note.pitch - 69) / 12);
      gain.gain.setValueAtTime(0, now);
      gain.gain.linearRampToValueAtTime(note.part === 2 ? 0.025 : 0.09, now + Math.min(0.015, duration / 3));
      gain.gain.setValueAtTime(note.part === 2 ? 0.025 : 0.09, Math.max(now + duration / 3, end - 0.04));
      gain.gain.linearRampToValueAtTime(0, end);
      osc.connect(gain); gain.connect(context.destination);
      osc.onended = () => { osc.disconnect(); gain.disconnect(); };
      osc.start(now); osc.stop(end);
    }
    if (elapsed >= score.seconds) stop();
  };
  const timer = setInterval(pump, 100); activePreview = stop; pump();
  return stop;
}

const escapeXml = (s: string) => s.replace(/[<>&"']/g, c => ({"<":"&lt;",">":"&gt;","&":"&amp;",'"':"&quot;","'":"&apos;"}[c]!));
/** Paginated four-bar piano-roll notation. All labels are escaped before export. */
export function scoreSvg(score: MusicScore, title: string, page = 0): string {
  const pageStart = Math.max(0, Math.floor(page)) * 32;
  const bars = score.voices.Vocal.bars.slice(pageStart, pageStart + 32), rows = Math.ceil(bars.length / 4);
  const all = SCORE_VOICES.flatMap(name => score.voices[name].notes);
  const low = Math.min(48, ...all.map(n => n.pitch)), high = Math.max(72, ...all.map(n => n.pitch));
  const height = 200, width = 1000;
  const parts = [`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${rows * height + 50}" role="img"><title>${escapeXml(title)}</title><rect width="100%" height="100%" fill="white"/><g font-family="sans-serif" font-size="12"><text x="20" y="25" fill="#111">${escapeXml(title)} · ${score.bpm} BPM · ${escapeXml(score.key)} · Vocal / Ins</text>`];
  for (let row = 0; row < rows; row++) {
    const chunk = bars.slice(row * 4, row * 4 + 4), from = chunk[0].onset, to = chunk.at(-1)!.onset + chunk.at(-1)!.duration;
    const x = (quarter: number) => 50 + (quarter - from) / (to - from) * 920;
    const y = (pitch: number) => row * height + 95 + (high - pitch) / (high - low + 1) * 125;
    for (let pitch = low; pitch <= high; pitch++) if (pitch % 12 === 0) parts.push(`<path d="M50 ${y(pitch)}H970" stroke="#ddd"/><text x="8" y="${y(pitch)+4}">C${pitch/12-1}</text>`);
    for (const [i, bar] of chunk.entries()) parts.push(`<path d="M${x(bar.onset)} ${row*height+70}v155" stroke="#aaa"/><text x="${x(bar.onset)+3}" y="${row*height+62}" fill="#555">${pageStart+row*4+i+1} (${bar.meter.join("/")})</text>`);
    for (const chord of score.voices.Vocal.chords.filter(c => c.onset >= from && c.onset < to)) parts.push(`<text x="${x(chord.onset)+3}" y="${row*height+82}" fill="#444">${escapeXml(chord.symbol)}</text>`);
    for (const name of SCORE_VOICES) for (const n of score.voices[name].notes.filter(n => n.onset < to && n.onset+n.duration > from)) parts.push(`<rect x="${x(Math.max(from,n.onset))}" y="${y(n.pitch)-3}" width="${Math.max(1,x(Math.min(to,n.onset+n.duration))-x(Math.max(from,n.onset))-1)}" height="6" fill="${name === "Vocal" ? "#6366f1" : "#059669"}"/>`);
  }
  return parts.join("") + "</g></svg>";
}
