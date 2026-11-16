/**
 * On NovelAI models with character prompts (V4 and later), a portrait puts
 * the character in a character box, the way Use does, and keeps the scene
 * (count, framing, style) in the main prompt. Counts such as 1girl belong in
 * the main prompt only (see `naiPrompt.ts`), so the box starts with girl, boy
 * or other instead.
 *
 * Leaf util with no store imports, so it runs under plain Node for checking.
 */
import { splitTopLevel } from "./portraitFilter.js";

/** A count tag such as 1girl, 2boys or 1other. */
const COUNT_TAG = /^(\d+)\s*(girl|boy|other)s?$/;

/** Lowercase, with underscores read as spaces. */
function plain(tag: string): string {
  return tag.trim().toLowerCase().replace(/_/g, " ").replace(/\s+/g, " ");
}

/**
 * Split a card's tag prompt into the count tags the main prompt takes and the
 * look the character box takes. `solo` goes too, since the portrait adds its
 * own. The box starts with girl, boy or other when the card had a count tag
 * and does not say so already.
 */
export function splitCountTags(prompt: string): { counts: string; look: string } {
  const counts: string[] = [];
  const look: string[] = [];
  let noun: string | null = null;
  for (const tag of splitTopLevel(prompt)) {
    const text = plain(tag);
    const count = COUNT_TAG.exec(text);
    if (count) {
      counts.push(tag);
      noun ??= count[2];
    } else if (text !== "solo") {
      look.push(tag);
    }
  }
  if (noun && !look.some((t) => plain(t) === noun)) look.unshift(noun);
  return { counts: counts.join(", "), look: look.join(", ") };
}
