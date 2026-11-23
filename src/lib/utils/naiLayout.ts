/**
 * Layout and occlusion hygiene for NovelAI V5 rewrites.
 *
 * V5 handles "a character sitting behind a table" poorly, and three failures
 * come back again and again: the character sits on the table, the table goes
 * missing, or one of them turns see-through. A short layout in the base prompt
 * (`table in foreground, behind table, sitting in chair` plus one plain
 * sentence) holds up best. What a rewrite tends to add on top makes it worse:
 *
 * - Object-relative tags in a character box. A box is that character's own
 *   region, so `behind table` there draws a table as part of the character,
 *   which is how they end up on top of it.
 * - Assertions of what should not happen (`opaque`, `not transparent`, `not on
 *   table`). Naming a failure tokenises it and invites it in.
 * - Undesired content that negates an object the scene contains (`table`, `on
 *   table`). A negated object disappears, or fades where it overlaps.
 *
 * `tidyNaiLayout` fixes the tag-level cases without a retry. Prose is never
 * edited here: `layoutProseProblems` reports it so the validator can ask for a
 * rewrite instead, since deleting a sentence can take wanted detail with it.
 *
 * Frame placement (`on the left`, `in the center`) is not touched. It places
 * the box itself, and outfit isolation depends on it.
 *
 * Leaf util. It must not import any store.
 */

import { appendTags, fieldTags, isProseLine, removeTag, tagKind } from "./naiOutfits.js";

/** Body parts a `behind` tag can name without placing anyone behind an object. */
const BODY_PARTS = /^(?:own |her |his |their )?(?:back|head|ear|ears|neck|hair)$/;

/** Who a `behind` tag can name without it being a scene object. */
const PEOPLE = /^(?:her|him|them|another|each other|the other|other|viewer|partner|girl|boy|man|woman)$/;

/** Words that end a placement in the frame rather than naming an object. */
const FRAME_WORDS = new Set(["left", "right", "center", "centre", "middle", "side", "viewer", "frame"]);

/** Tags that only say what is hidden, with no object to pin them to. */
const OCCLUSION_TAGS = new Set([
  "partially hidden",
  "partially obscured",
  "partially occluded",
  "occluded",
  "obscured",
  "lower body hidden",
  "legs hidden",
  "lower body obscured",
  "legs obscured",
]);

/** A tag that asserts what should not happen. Never useful as a positive tag. */
const ASSERTION =
  /^(?:solid|opaque|fully opaque|completely opaque|non-transparent|non transparent|not (?:transparent|translucent|see-through|see through|ghostly|faded|floating|(?:sitting |standing |kneeling |lying )?on (?:top of )?(?:the )?\S.*))$/;

/** `on table`, `sitting on the table`: a placement on an object, for UC checks. */
const ON_OBJECT = /^(?:sitting |standing |kneeling |lying |leaning )?(?:on|onto) (?:top of )?(?:the |a )?(.+)$/;

/** Prepositions that tie a pose to an object: `hands on table`, `leaning against wall`. */
const TIED_TO_OBJECT = /\b(?:on|at|against|over|across|under|behind)\s+(?:the\s+|a\s+)?(\S+)$/;

/** The object a relative layout tag points at, or null when it is not one. */
export function layoutObject(norm: string): string | null {
  const behind = norm.match(/^(?:behind|in front of|hidden behind|obscured by)\s+(?:the\s+|a\s+)?(.+)$/);
  if (behind && !BODY_PARTS.test(behind[1]) && !PEOPLE.test(behind[1])) return lastWord(behind[1]);
  const fore = norm.match(/^(.+?)\s+in (?:the )?foreground$/);
  if (fore) return lastWord(fore[1]);
  return null;
}

function lastWord(text: string): string {
  const words = text.replace(/\s*\([^)]*\)\s*$/, "").trim().split(" ");
  return words[words.length - 1] ?? "";
}

/** True for a box tag that belongs in BASE: a position relative to a scene object, or occlusion. */
function isRelativeTag(norm: string, objects: Set<string>): boolean {
  if (OCCLUSION_TAGS.has(norm)) return true;
  if (layoutObject(norm) !== null) return true;
  const tied = norm.match(TIED_TO_OBJECT);
  return !!tied && objects.has(tied[1]) && !FRAME_WORDS.has(tied[1]);
}

/** The first tag line of a field, so moved tags join the tag line and not the prose. */
function appendToTagLine(base: string, tags: string[]): string {
  if (tags.length === 0) return base;
  const lines = (base ?? "").split("\n");
  const at = lines.findIndex((l) => l.trim() && !isProseLine(l) && !/^\s*Text\s*:/i.test(l));
  const textAt = lines.findIndex((l) => /^\s*Text\s*:/i.test(l));
  if (at === -1 || (textAt !== -1 && at > textAt)) return [tags.join(", "), "", ...lines].join("\n").trim();
  lines[at] = appendTags(lines[at], tags);
  return lines.join("\n");
}

export interface LayoutFields {
  base: string;
  uc: string;
  boxes: string[];
  /** Each box's own UC, or null when the rewrite wrote none for it. */
  ucs: (string | null)[];
}

/**
 * Tag-level layout cleanup, safe to run on every rewrite:
 * - object-relative and occlusion tags leave the boxes for the BASE tag line;
 * - a bare object noun leaves a box when the layout is about that object;
 * - assertion tags (`opaque`, `not transparent`, `not on table`) leave every
 *   positive field;
 * - UC and box UC drop tags that negate an object BASE asks for.
 *
 * Returns the fields unchanged when there is nothing to fix.
 */
export function tidyNaiLayout(fields: LayoutFields): LayoutFields {
  let base = fields.base;
  const baseTags = () => fieldTags(base).map((t) => t.norm);

  for (const tag of new Set(baseTags())) if (ASSERTION.test(tag)) base = removeTag(base, tag);

  // Objects the scene holds, by their head noun: `wooden table` is a table.
  const objectsOf = (tags: string[]) => {
    const out = new Set<string>();
    for (const t of tags) {
      const obj = layoutObject(t);
      if (obj) out.add(obj);
      else if (tagKind(t) === "other") out.add(lastWord(t));
    }
    return out;
  };
  const baseObjects = objectsOf(baseTags());

  const moved: string[] = [];
  const boxes = fields.boxes.map((box) => {
    let out = box;
    const tags = fieldTags(box).map((t) => t.norm);
    for (const tag of new Set(tags)) {
      if (ASSERTION.test(tag)) {
        out = removeTag(out, tag);
      } else if (isRelativeTag(tag, baseObjects)) {
        out = removeTag(out, tag);
        if (!moved.includes(tag)) moved.push(tag);
      }
    }
    return out;
  });

  const existing = new Set(baseTags());
  base = appendToTagLine(base, moved.filter((t) => !existing.has(t)));

  // What the layout is about: the objects named by a relative tag anywhere.
  const layoutObjects = new Set(
    [...baseTags(), ...moved].map(layoutObject).filter((o): o is string => o !== null),
  );
  const cleanBoxes = boxes.map((box) => {
    let out = box;
    for (const tag of new Set(fieldTags(box).map((t) => t.norm))) {
      if (layoutObjects.has(tag)) out = removeTag(out, tag);
    }
    return out;
  });

  // Undesired content never negates what the scene is built around. The
  // global UC loses any tag BASE also asks for; every UC loses the layout's
  // objects and placements on an object BASE holds (`on table`). Other tags in
  // a box's UC stay: avoiding a pose BASE mentions can be right for one box.
  const wantedTags = new Set(baseTags());
  const onWanted = (tag: string) => {
    const on = tag.match(ON_OBJECT);
    if (!on) return false;
    const obj = lastWord(on[1]);
    return !FRAME_WORDS.has(obj) && (layoutObjects.has(obj) || baseObjects.has(obj));
  };
  const negatesWanted = (tag: string, boxUc: boolean) =>
    (!boxUc && wantedTags.has(tag)) || layoutObjects.has(tag) || onWanted(tag);
  const cleanUc = (uc: string, boxUc: boolean) => {
    let out = uc;
    for (const tag of new Set(fieldTags(uc).map((t) => t.norm))) {
      if (negatesWanted(tag, boxUc)) out = removeTag(out, tag);
    }
    return out;
  };

  return {
    base,
    uc: cleanUc(fields.uc, false),
    boxes: cleanBoxes,
    ucs: fields.ucs.map((uc) => (uc === null ? null : cleanUc(uc, true))),
  };
}

/**
 * Prose in BASE that spells out what the layout is not. Each match is quoted
 * back in the validator problem. Reported, not deleted: the same sentence can
 * carry detail the rewrite should keep.
 */
const PROSE_NEGATIONS: RegExp[] = [
  /\b(?:not|never|isn't|aren't|nor|rather than|instead of|without (?:being|looking))\b[^.!?\n]{0,30}?\b(?:transparent|translucent|see[- ]through|ghostly|faded|floating)\b/i,
  /\b(?:not|never|isn't|aren't|rather than|instead of)\b[^.!?\n]{0,24}?\bon (?:top of )?(?:the|a|an)\s+(?!left\b|right\b|side\b|other\b)\w+/i,
  /\b(?:fully |completely |entirely )?(?:opaque|non-transparent)\b/i,
];

/** Negating phrases in the BASE prose, as written, deduplicated. */
export function layoutProseProblems(base: string): string[] {
  const prose = (base ?? "")
    .split(/(?:^|\n)\s*Text\s*:/i)[0]
    .split("\n")
    .filter(isProseLine)
    .join(" ");
  const found: string[] = [];
  for (const re of PROSE_NEGATIONS) {
    const m = prose.match(re);
    if (m && !found.includes(m[0])) found.push(m[0]);
  }
  return found;
}
