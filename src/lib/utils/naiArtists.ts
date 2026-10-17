/**
 * "Keep artists" for the NovelAI V5 enhance.
 *
 * A V5 rewrite is blind by default: the model never sees the prompt box, so an
 * artist stack the user tuned by hand is gone the moment the rewrite is
 * applied. With the box ticked, the artist tags in the current prompt are
 * pulled out here, handed to the model as tags to keep, and put back into the
 * base tag line afterwards if the rewrite dropped any. The model is asked
 * first so the artists land where V5 wants them; the check after is what makes
 * "keep" a promise rather than a request.
 *
 * NovelAI mode writes artist tags bare, so telling one apart from any other
 * danbooru tag takes the artist index, plus the `artist:` prefix people carry
 * over from Danbooru's search box. Leaf util: no store imports.
 */

import type { ArtistTagIndex } from "../artist-gallery/detection.js";
import { artistIndexKey, isArtistTagToken } from "./artistTag.js";

/** A V5 weighted span, `1.2::a, b::` or `-1::hat::`. */
const WEIGHT_SPAN = /(-?(?:\d+\.?\d*|\.\d+))::([\s\S]*?)::/g;

/**
 * `{tag}` / `[tag]` emphasis peeled off, any depth, plus the `artist:` prefix.
 * Each side is stripped on its own, since a group like `{{a, b}}` splits into
 * tokens that carry only one side of the brackets.
 */
function bareTag(token: string): string {
  return token
    .trim()
    .replace(/^[{[\s]+/, "")
    .replace(/[}\]\s]+$/, "")
    .replace(/^artist:\s*/i, "")
    .trim();
}

function isArtist(token: string, index: ArtistTagIndex | null): boolean {
  const bare = bareTag(token);
  if (!bare) return false;
  if (/^[{[\s]*artist:/i.test(token)) return true;
  return isArtistTagToken(bare, "", index);
}

/** The comparable key of an artist token, weights and brackets ignored. */
function artistKey(token: string): string {
  return artistIndexKey(bareTag(token));
}

/** A token whose brackets open and close inside it, so it can stand alone. */
function balanced(token: string): boolean {
  const count = (re: RegExp) => (token.match(re) ?? []).length;
  return count(/\{/g) === count(/\}/g) && count(/\[/g) === count(/\]/g);
}

/**
 * The `Text:` block is lettering, not tags, so a word in it that happens to be
 * an artist name is not one. Everything from that line on is left out.
 */
function tagSection(prompt: string): string {
  const lines = prompt.split("\n");
  const textAt = lines.findIndex((line) => /^\s*text:/i.test(line));
  return (textAt === -1 ? lines : lines.slice(0, textAt)).join("\n");
}

/**
 * The artist tags in a prompt, as written, in prompt order.
 *
 * A weighted span comes back as a span holding only its artists, so
 * `0.6::wlop, rain::` keeps `0.6::wlop::`. A plain tag keeps its `{}` / `[]`
 * emphasis when the brackets close inside it, and is bare otherwise.
 */
export function extractNaiArtists(prompt: string, index: ArtistTagIndex | null): string[] {
  const source = tagSection(prompt);
  const found: { at: number; text: string }[] = [];
  // Spans are blanked to the same length, so offsets in what is left still
  // line up with the prompt and the result can be put back in order.
  const rest = source.replace(WEIGHT_SPAN, (whole: string, weight: string, body: string, at: number) => {
    const artists = body
      .split(/[,\n]/)
      .map((t) => t.trim())
      .filter((t) => isArtist(t, index));
    if (artists.length > 0) found.push({ at, text: `${weight}::${artists.join(", ")}::` });
    return "," + " ".repeat(whole.length - 1);
  });
  for (const match of rest.matchAll(/[^,\n]+/g)) {
    const token = match[0].trim();
    if (!isArtist(token, index)) continue;
    found.push({ at: match.index ?? 0, text: balanced(token) ? token : bareTag(token) });
  }
  const seen = new Set<string>();
  return found
    .sort((a, b) => a.at - b.at)
    .map((f) => f.text)
    .filter((text) => {
      if (seen.has(text)) return false;
      seen.add(text);
      return true;
    });
}

/** Every tag key in a prompt's tag section, spans opened up. */
function promptTagKeys(prompt: string): Set<string> {
  const flat = tagSection(prompt).replace(WEIGHT_SPAN, (_whole: string, _weight: string, body: string) => `,${body},`);
  return new Set(
    flat
      .split(/[,\n]/)
      .map((t) => t.trim())
      .filter(Boolean)
      .map(artistKey),
  );
}

/**
 * Put back the kept artists a rewrite dropped.
 *
 * An artist already in the base under any weight counts as kept: the model
 * moving it is fine, losing it is not. The missing ones go on the end of the
 * first line, the tag line, where V5's base order keeps artists. A span that
 * lost only some of its artists comes back holding just those.
 */
export function restoreNaiArtists(base: string, kept: string[]): string {
  if (kept.length === 0) return base;
  const present = promptTagKeys(base);
  const missing: string[] = [];
  for (const piece of kept) {
    const span = /^(-?(?:\d+\.?\d*|\.\d+))::([\s\S]*)::$/.exec(piece);
    if (span) {
      const lost = span[2]
        .split(",")
        .map((t) => t.trim())
        .filter((t) => t && !present.has(artistKey(t)));
      if (lost.length > 0) missing.push(`${span[1]}::${lost.join(", ")}::`);
    } else if (!present.has(artistKey(piece))) {
      missing.push(piece);
    }
  }
  if (missing.length === 0) return base;
  if (!base.trim()) return missing.join(", ");
  const lines = base.split("\n");
  const first = lines[0].trim().replace(/,\s*$/, "");
  // An empty first line or one that opens on the lettering block has no tag
  // line to extend, so the artists become one.
  if (!first || /^text:/i.test(first)) return [missing.join(", "), ...lines].join("\n");
  lines[0] = `${first}, ${missing.join(", ")}`;
  return lines.join("\n");
}

/** The system prompt block naming the artists to keep, or "" for none. */
export function naiKeptArtistsDirective(kept: string[] | undefined): string {
  if (!kept || kept.length === 0) return "";
  return `KEPT ARTISTS
- The user wants the artists from their current prompt kept: ${kept.join(", ")}
- Put them in the base tag line exactly as written here, weights and brackets included. Do not drop, rename or reweight them, and do not add other artists unless the user's message asks for them.`;
}
