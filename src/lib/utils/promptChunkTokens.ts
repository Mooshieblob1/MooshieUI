/**
 * Inline prompt-chunk tokens: the two spellings a user can drop into a prompt
 * to splice a saved chunk in at that exact spot.
 *
 * A leaf util on purpose. The regex and the slug rule are needed by the store
 * (`promptPresets.svelte.ts`), by the highlighter and by the inert-range
 * scanner, and none of those may depend on each other.
 */

/**
 * Normalise a chunk's display name to its slug form: lowercase, with every
 * run of non-alphanumerics collapsed to a single underscore.
 */
export function presetSlug(name: string): string {
  return (
    name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "_")
      .replace(/^_+|_+$/g, "") || "preset"
  );
}

/**
 * Both accepted inline forms in one pass:
 *
 * - `@preset:<slug>` - the original, slug-cased spelling.
 * - `@[Chunk Name]`  - the display-name spelling, which is what the editor
 *   offers to copy because it survives a rename being read back by a human.
 *
 * Group 1 holds the slug when the first form matched, group 2 the raw name
 * when the second did. Exactly one of the two is ever set, so `presetTokenSlug`
 * is the only thing callers need to read.
 */
export const PROMPT_PRESET_TOKEN_REGEX = /@(?:preset:([a-z0-9_]+)|\[([^\][\n]+)\])/gi;

/**
 * Cheap substring pre-filter. The highlighter and the inert-range scanner run
 * on every keystroke, so they check this before allocating a regex match.
 */
export function mayContainPresetToken(raw: string | null | undefined): boolean {
  return !!raw && (raw.includes("@preset:") || raw.includes("@["));
}

/**
 * The slug a matched token resolves to, whichever form was written. Both
 * spellings land on the same slug, so lookups stay single-keyed.
 */
export function presetTokenSlug(match: RegExpMatchArray): string {
  const slug = match[1];
  if (slug !== undefined) return slug.toLowerCase();
  return presetSlug(match[2] ?? "");
}

/**
 * The token to show and copy for a chunk. A name carrying a closing bracket or
 * a newline cannot be written in the bracket form without ambiguity, so those
 * fall back to the slug spelling rather than emitting something unparseable.
 */
export function inlineChunkToken(name: string): string {
  const trimmed = name.trim();
  if (!trimmed || /[\][\n]/.test(trimmed)) return `@preset:${presetSlug(name)}`;
  return `@[${trimmed}]`;
}
