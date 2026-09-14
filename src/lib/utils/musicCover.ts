/** Original short melody for trying score-conditioned generation without transcription. */
export const COVER_EXAMPLE = `X:1
T:
M:4/4
L:1/8
Q:1/4=100
V: Vocal clef=treble name="Vocal Melody" snm="Vocal"
V: Ins clef=treble name="Ins Melody" snm="Inst."
K:C
% verse
V: Vocal
C2 E2 G2 E2 | D2 F2 A4 | G2 E2 D2 C2 | E4 D4 |
V: Ins
Z4|
% chorus
V: Vocal
C2 E2 G2 c2 | B2 A2 G4 | A2 G2 E2 D2 | C8 |
V: Ins
Z4|
`;

/** Match the source without clipping its fractional last second. */
export function coverSourceMaxDuration(seconds: number): number | null {
  return Number.isFinite(seconds) && seconds > 0 && seconds <= 360
    ? Math.max(1, Math.ceil(seconds)) : null;
}

/** Remove harmony symbols, preserving headers, inline fields, comments and positioned annotations. */
export function melodyOnlyAbc(abc: string): string {
  return abc.split(/\r?\n/).map(line => {
    if (/^\s*(?:[A-Za-z]:|%)/.test(line)) return line;
    return line.replace(/\[[A-Za-z]:[^\]]*\]|%.*|"(?:\\.|[^"\\])*"/g, token =>
      token.startsWith('"') && !/^[\^_<>@]/.test(token[1] ?? "") ? "" : token);
  }).join("\n");
}

export function coverScoreError(abc: string, preserveHarmony = false): "cover_score_required" | "cover_score_invalid" | "cover_chords" | null {
  if (!abc.trim()) return "cover_score_required";
  if (new TextEncoder().encode(abc).length > 131072 || !/^\s*X:\s*\d+/m.test(abc) || !/^\s*K:\s*\S+/m.test(abc)) return "cover_score_invalid";
  const body = abc.split(/\r?\n/).filter(line => !/^\s*(?:[A-Za-z]:|%)/.test(line)).join("\n")
    .replace(/\[[A-Za-z]:[^\]]*\]|"(?:\\.|[^"\\])*"|%[^\n]*/g, "");
  if (!/[A-Ga-g]/.test(body)) return "cover_score_invalid";
  if (!preserveHarmony && melodyOnlyAbc(abc).replace(/\r\n/g, "\n") !== abc.replace(/\r\n/g, "\n")) return "cover_chords";
  return null;
}
