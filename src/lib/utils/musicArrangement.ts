export const arrangementKinds = ["intro", "verse", "chorus", "bridge", "theme", "development", "outro"] as const;
export type ArrangementKind = typeof arrangementKinds[number];
export interface ArrangementSection { kind: ArrangementKind; seconds: number; direction: string }
export function validArrangement(sections: ArrangementSection[], duration: number): boolean {
  return Number.isFinite(duration) && duration >= 1 && duration <= 360 && sections.length > 0 && sections.length <= 12
    && sections.every(s => arrangementKinds.includes(s.kind) && Number.isInteger(s.seconds) && s.seconds >= 1 && s.seconds <= 360 && typeof s.direction === "string" && s.direction.length <= 160 && !/[\r\n]/.test(s.direction))
    && sections.reduce((sum, s) => sum + s.seconds, 0) <= duration;
}
export function fitArrangement(sections: ArrangementSection[], duration: number): ArrangementSection[] {
  const budget = Math.floor(duration);
  if (!Number.isFinite(budget) || budget < sections.length || budget > 360 || !sections.length || sections.length > 12) throw new Error("invalid_arrangement");
  const weights = sections.map(s => Number.isFinite(s.seconds) && s.seconds > 0 ? s.seconds : 1);
  const total = weights.reduce((a, b) => a + b, 0);
  const extra = budget - sections.length;
  const parts = weights.map(w => w / total * extra);
  const counts = parts.map(n => 1 + Math.floor(n));
  const order = parts.map((n, i) => ({ i, fraction: n % 1 })).sort((a, b) => b.fraction - a.fraction);
  const remainder = budget - counts.reduce((a, b) => a + b, 0);
  for (let i = 0; i < remainder; i++) counts[order[i].i]++;
  return sections.map((s, i) => ({ ...s, seconds: counts[i] }));
}
export function suggestArrangement(duration: number, lyrics: string): ArrangementSection[] {
  let kinds: ArrangementKind[];
  if (lyrics.trim()) {
    const sections = [...lyrics.matchAll(/^\s*\[([^\]]+)\]\s*$/gm)].map(m => {
      const label = m[1].toLowerCase();
      return arrangementKinds.find(kind => label === kind || label.startsWith(kind + " "));
    }).filter((kind): kind is ArrangementKind => !!kind).slice(0, 12);
    kinds = sections.length ? sections : ["intro", "verse", "chorus", "outro"];
  } else kinds = ["intro", "theme", "development", "theme", "outro"];
  if (duration < kinds.length) kinds = [lyrics.trim() ? "verse" : "theme"];
  return fitArrangement(kinds.map(kind => ({ kind, seconds: kind === "intro" || kind === "outro" ? 1 : 4, direction: "" })), duration);
}
export function arrangementTimeline(sections: ArrangementSection[]) {
  let time = 0;
  return sections.map(section => { const start = time; time += section.seconds; return { ...section, start, end: time }; });
}
export function arrangementStyle(style: string, sections: ArrangementSection[], duration: number, instrumental: boolean): string {
  if (!validArrangement(sections, duration)) throw new Error("invalid_arrangement");
  const rows = arrangementTimeline(sections).map(s => s.start + "–" + s.end + "s " + s.kind + (s.direction.trim() ? " (" + s.direction.trim() + ")" : ""));
  const hint = "Arrangement plan: " + rows.join("; ") + ". Aim for these approximate section timings within " + duration
    + " seconds, with smooth transitions and a resolved ending."
    + (instrumental ? " Entirely instrumental; no singing, speech, humming or choir." : "Keep the supplied lyrics and their order; do not invent or repeat extra lines to fill the plan.");
  const result = [style.trim(), hint].filter(Boolean).join(" ");
  if (result.length > 16000) throw new Error("invalid_arrangement");
  return result;
}
