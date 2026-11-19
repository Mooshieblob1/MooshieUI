/**
 * Duplicate detection for Extract in the Characters tab: which extracted
 * characters are already saved, so they are offered for review instead of
 * being added a second time or silently overwriting the saved card.
 *
 * Two signals. The LLM's `sameAs` catches nicknames and an unnamed character
 * who is clearly a saved one by appearance, and is only trusted when it names
 * a character that is actually saved. Name matching below backs it up when the
 * model leaves it blank, which small local models often do.
 */
import type { ExtractedCharacter } from "./characterExtract.js";
import { promptTags } from "./galleryCharacters.js";

export interface DuplicateCandidate {
  id: string;
  name: string;
  prompt: string;
  negative: string;
}

/** Lowercase words of a name, punctuation dropped. */
function nameWords(name: string): string[] {
  return name
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean);
}

/** Whether every word of the shorter name is a word of the longer one. */
function wordsContained(a: string[], b: string[]): boolean {
  const [short, long] = a.length <= b.length ? [a, b] : [b, a];
  if (short.length === 0) return false;
  // One short word ("a", "of") alone is too weak to call two names the same.
  if (short.length === 1 && short[0].length < 3) return false;
  return short.every((w) => long.includes(w));
}

/** Tags that count people rather than describe one. */
const COUNT_TAG = /^(\d+\+?\s?(girl|boy|other)s?|multiple (girls|boys|others)|solo( focus)?|no humans)$/;

/** How many describing tags two prompts share. */
function sharedTags(a: string, b: string): number {
  const other = new Set(promptTags(b));
  return new Set(promptTags(a).filter((t) => !COUNT_TAG.test(t) && other.has(t))).size;
}

/** The saved character this extracted one duplicates, or null. */
export function findDuplicate<T extends DuplicateCandidate>(
  item: ExtractedCharacter,
  saved: T[],
): T | null {
  const hint = item.sameAs.trim().toLowerCase();
  if (hint) {
    const byHint = saved.find((c) => c.name.trim().toLowerCase() === hint);
    // Small models set same_as on characters who only look alike, so the
    // hint needs backing: a shared name word, or two describing tags in
    // common (a nickname, or an unnamed character drawn the same way).
    if (
      byHint &&
      (wordsContained(nameWords(item.name), nameWords(byHint.name)) || sharedTags(item.prompt, byHint.prompt) >= 2)
    ) {
      return byHint;
    }
  }
  const words = nameWords(item.name);
  const joined = words.join(" ");
  const exact = saved.find((c) => nameWords(c.name).join(" ") === joined);
  if (exact) return exact;
  // "Julie" against "Julie Evergreen", or "Evergreen" alone. Only when exactly
  // one saved character fits; two Julies is not a match to guess at.
  const partial = saved.filter((c) => wordsContained(words, nameWords(c.name)));
  if (partial.length === 1) return partial[0];
  return null;
}

/** Whether the extracted text is what the saved card already says. */
export function sameContent(item: ExtractedCharacter, saved: DuplicateCandidate): boolean {
  const norm = (s: string) => s.replace(/\s+/g, " ").trim().toLowerCase();
  return norm(item.prompt) === norm(saved.prompt) && norm(item.negative) === norm(saved.negative);
}
