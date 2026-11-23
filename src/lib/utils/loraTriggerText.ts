/**
 * Putting a LoRA's trigger text into the positive prompt and taking it out
 * again, as one unit.
 *
 * A trigger can be one word (`@artist`), a phrase, or several tags with commas
 * in between (`yae miko, pink hair`). The prompt is read as a list of pieces
 * split on commas and line breaks; a trigger matches a run of consecutive
 * pieces that equal its own pieces, ignoring case and the spaces around them.
 * So `yae miko,pink hair` is found as `yae miko, pink hair`, but a weighted
 * `(yae miko:1.2)` or a tag that merely contains the trigger is not, and is
 * never touched.
 */

/** The trigger's pieces: split on commas and line breaks, trimmed, empties dropped. */
export function triggerParts(trigger: string): string[] {
  return trigger
    .split(/[,\n]/)
    .map((part) => part.trim())
    .filter(Boolean);
}

/** The trigger as it is written into a prompt: its pieces joined by ", ". */
export function normalizeTrigger(trigger: string): string {
  return triggerParts(trigger).join(", ");
}

interface Piece {
  /** First non-space character. */
  start: number;
  /** One past the last non-space character. */
  end: number;
  key: string;
}

function pieces(text: string): Piece[] {
  const out: Piece[] = [];
  let from = 0;
  for (let i = 0; i <= text.length; i++) {
    if (i < text.length && text[i] !== "," && text[i] !== "\n") continue;
    let start = from;
    let end = i;
    while (start < end && /\s/.test(text[start])) start++;
    while (end > start && /\s/.test(text[end - 1])) end--;
    out.push({ start, end, key: text.slice(start, end).toLowerCase() });
    from = i + 1;
  }
  return out;
}

/** Where the trigger sits in `text` as whole pieces, or null. */
export function findTrigger(text: string, trigger: string): { start: number; end: number } | null {
  const parts = triggerParts(trigger).map((p) => p.toLowerCase());
  if (parts.length === 0) return null;
  const list = pieces(text);
  for (let i = 0; i + parts.length <= list.length; i++) {
    let ok = true;
    for (let j = 0; j < parts.length; j++) {
      if (list[i + j].key !== parts[j]) {
        ok = false;
        break;
      }
    }
    if (ok) return { start: list[i].start, end: list[i + parts.length - 1].end };
  }
  return null;
}

export function hasTrigger(text: string, trigger: string): boolean {
  return findTrigger(text, trigger) !== null;
}

/** Append the trigger as its own piece, unless it is already in the prompt. */
export function insertTrigger(text: string, trigger: string): string {
  const unit = normalizeTrigger(trigger);
  if (!unit || hasTrigger(text, unit)) return text;
  const base = text.replace(/\s+$/, "");
  if (!base) return unit;
  return base.endsWith(",") ? `${base} ${unit}` : `${base}, ${unit}`;
}

/**
 * Take the first occurrence of the trigger out, with one comma next to it so
 * the rest of the prompt reads as before. A trigger alone on its line takes
 * the line break with it. Returns the text unchanged when it is not there.
 */
export function removeTrigger(text: string, trigger: string): string {
  const hit = findTrigger(text, trigger);
  if (!hit) return text;
  let { start, end } = hit;
  // Following comma and the spaces after it.
  let after = end;
  while (after < text.length && (text[after] === " " || text[after] === "\t")) after++;
  if (text[after] === ",") {
    after++;
    while (after < text.length && (text[after] === " " || text[after] === "\t")) after++;
    return text.slice(0, start) + text.slice(after);
  }
  // Otherwise a preceding comma.
  let before = start;
  while (before > 0 && (text[before - 1] === " " || text[before - 1] === "\t")) before--;
  if (before > 0 && text[before - 1] === ",") {
    return text.slice(0, before - 1) + text.slice(after);
  }
  // Alone on its line (or in the whole prompt).
  if (text[after] === "\n" && (before === 0 || text[before - 1] === "\n")) after++;
  else if (after === text.length && before > 0 && text[before - 1] === "\n") before--;
  return text.slice(0, before) + text.slice(after);
}
