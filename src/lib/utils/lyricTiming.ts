export interface LyricLine {
  text: string;
  section: boolean;
  start: number | null;
  end: number | null;
}

export interface LyricAlignment {
  lines: LyricLine[];
  matched: number;
  total: number;
}

export function lyricLines(lyrics: string): LyricLine[] {
  return lyrics.split(/\r?\n/).map(text => text.trim()).filter(Boolean)
    .map(text => ({ text, section: /^\[[^\]]+\]$/.test(text), start: null, end: null }));
}

/** User-entered line starts, with each line ending at the next mark or song end. */
export function manualLyricAlignment(lines: LyricLine[], duration: number): LyricAlignment {
  if (!Number.isFinite(duration) || duration <= 0) throw new Error("invalid_timing");
  let previous = -1;
  const result = lines.map(line => {
    const start = line.section ? null : line.start;
    if (start !== null && (!Number.isFinite(start) || start < 0 || start >= duration || start <= previous)) {
      throw new Error("invalid_timing");
    }
    if (start !== null) previous = start;
    return { ...line, start, end: null as number | null };
  });
  let end = duration;
  for (let i = result.length - 1; i >= 0; i--) {
    if (result[i].start !== null) { result[i].end = end; end = result[i].start!; }
  }
  return { lines: result, matched: result.filter(line => line.start !== null).length, total: result.filter(line => !line.section).length };
}

export function activeLyricLine(lines: LyricLine[], seconds: number): number {
  return Number.isFinite(seconds) ? lines.findIndex(line => line.start !== null && line.end !== null && seconds >= line.start && seconds < line.end) : -1;
}
