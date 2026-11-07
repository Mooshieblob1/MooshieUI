/**
 * What a neck-up portrait leaves out of a character's prompt: everything
 * below the neck (body, legwear, shoes, poses, hands and what they hold) and
 * framings that fight a front-on close-up. Left in, those pull the model
 * into a wider shot or a pose the portrait cannot show.
 *
 * Leaf util with no imports, so it runs under plain Node for checking.
 */

/** Words that name something below the neck, matched as whole words. */
const BELOW_NECK_WORDS = new Set([
  // Body
  "arm", "arms", "armpit", "armpits", "elbow", "elbows", "forearm", "forearms", "wrist", "wrists",
  "hand", "hands", "palm", "palms", "finger", "fingers", "fingernails", "fist", "fists",
  "chest", "breast", "breasts", "cleavage", "underboob", "sideboob", "pectorals", "nipple", "nipples",
  "areola", "areolae", "torso", "stomach", "belly", "navel", "midriff", "abs", "waist", "hip", "hips",
  "butt", "ass", "crotch", "groin", "pubic", "pussy", "penis", "testicles", "genitals", "anus",
  "thigh", "thighs", "leg", "legs", "knee", "knees", "calf", "calves", "ankle", "ankles",
  "foot", "feet", "toe", "toes", "toenails", "sole", "soles", "barefoot", "tail", "tails",
  // Clothing below the neck
  "skirt", "skirts", "miniskirt", "microskirt", "pants", "trousers", "jeans", "shorts", "hakama",
  "panties", "underwear", "lingerie", "bra", "bikini", "swimsuit", "leotard", "bodysuit",
  "socks", "kneehighs", "thighhighs", "overkneehighs", "pantyhose", "stockings", "leggings",
  "legwear", "garter", "garters", "thighband", "anklet", "belt", "belts",
  "sock", "kneehigh", "thighhigh", "uwabaki",
  "shoe", "shoes", "boots", "sneakers", "loafers", "sandals", "slippers", "heels", "footwear", "geta", "zouri",
  "glove", "gloves", "mittens", "bracelet", "bracelets", "wristband", "wristbands", "sleeve", "sleeves",
  // Poses and actions
  "sitting", "standing", "kneeling", "squatting", "crouching", "lying", "reclining", "walking", "running",
  "jumping", "leaning", "seiza", "wariza", "contrapposto", "pose", "posing", "holding", "carrying",
  "pointing", "waving", "salute", "reaching",
  // Things carried
  "weapon", "weapons", "sword", "swords", "katana", "gun", "guns", "rifle", "pistol", "handgun",
  "spear", "polearm", "shield", "axe", "dagger", "knife", "staff", "wand", "scythe",
  "bag", "backpack", "handbag", "briefcase", "umbrella", "book", "phone", "cellphone", "smartphone",
]);

/** Several-word phrases: framings, views and gestures a front-on close-up cannot have. */
const BELOW_NECK_PHRASES = [
  "full body", "upper body", "lower body", "cowboy shot", "wide shot", "out of frame",
  "from side", "from behind", "from below", "from above", "dutch angle", "multiple views",
  "looking back", "looking away", "looking to the side", "on back", "on side", "all fours", "bent over",
  "peace sign", "thumbs up", "double v", "mary janes",
];

/** Colours, so "a black and white skirt" is read as one thing, not cut after "black". */
const COLOURS = new Set([
  "black", "white", "red", "blue", "green", "yellow", "purple", "pink", "orange", "brown", "grey", "gray",
  "silver", "gold", "golden", "navy", "beige", "cream", "dark", "light", "pale",
]);

/** Whole tags that only mean something below the neck on their own. */
const BELOW_NECK_TAGS = new Set(["v", "profile"]);

/** Lowercase words of a prompt piece, with underscores and hyphens read as spaces. */
function words(text: string): string[] {
  return text
    .toLowerCase()
    .replace(/[_-]/g, " ")
    .split(/[^\p{L}\p{N}']+/u)
    .filter(Boolean);
}

/** Does this piece of a prompt name something a neck-up, front-on portrait cannot show? */
export function mentionsBelowNeck(text: string): boolean {
  const list = words(text).filter((w) => !/^[\d.]+$/.test(w));
  if (list.length === 0) return false;
  if (list.some((w) => BELOW_NECK_WORDS.has(w))) return true;
  if (BELOW_NECK_TAGS.has(list.join(" "))) return true;
  const joined = ` ${list.join(" ")} `;
  return BELOW_NECK_PHRASES.some((p) => joined.includes(` ${p} `));
}

/**
 * Split at commas and newlines that are not inside brackets or a NovelAI
 * `1.2::...::` emphasis block, so a weighted group stays whole.
 */
export function splitTopLevel(text: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let inBlock = false;
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch === "\\") {
      i++;
      continue;
    }
    if (ch === "(" || ch === "[" || ch === "{") depth++;
    else if ((ch === ")" || ch === "]" || ch === "}") && depth > 0) depth--;
    else if (ch === ":" && text[i + 1] === ":") {
      // `1.2::` opens a block; the next `::` closes it.
      inBlock = inBlock ? false : /-?[\d.]+\s*$/.test(text.slice(start, i));
      i++;
    } else if ((ch === "," || ch === "\n") && depth === 0 && !inBlock) {
      out.push(text.slice(start, i));
      start = i + 1;
    }
  }
  out.push(text.slice(start));
  return out.map((s) => s.trim()).filter(Boolean);
}

/** The wrapper around a weighted group, or null when the item is a plain tag. */
function groupOf(item: string): { open: string; inner: string; close: string } | null {
  const nai = /^(-?[\d.]+\s*::)([\s\S]*?)(::)$/.exec(item);
  if (nai) return { open: nai[1], inner: nai[2], close: nai[3] };
  const paren = /^(\(+)([\s\S]*?)((?::\s*-?[\d.]+)?\)+)$/.exec(item);
  if (paren) return { open: paren[1], inner: paren[2], close: paren[3] };
  const other = /^([[{]+)([\s\S]*?)([\]}]+)$/.exec(item);
  if (other && !other[2].includes("|")) return { open: other[1], inner: other[2], close: other[3] };
  return null;
}

/** Artist tags and inline chunks are the user's picks, never filtered. */
function isUserToken(item: string): boolean {
  return /^@/.test(item) || /^artist:/i.test(item);
}

export interface PortraitFilterResult {
  /** The prompt with what a portrait cannot show taken out. */
  kept: string;
  /** What was taken out, in prompt order, so the user can put any of it back. */
  dropped: string[];
}

/**
 * Filter a comma-separated tag prompt. A weighted group that lists several
 * tags is filtered inside and kept with what is left.
 */
export function neckUpTags(prompt: string): PortraitFilterResult {
  const dropped: string[] = [];
  const filterList = (text: string): string[] => {
    const kept: string[] = [];
    for (const item of splitTopLevel(text)) {
      if (isUserToken(item)) {
        kept.push(item);
        continue;
      }
      const group = groupOf(item);
      if (group && splitTopLevel(group.inner).length > 1) {
        const inner = filterList(group.inner);
        if (inner.length > 0) kept.push(`${group.open}${inner.join(", ")}${group.close}`);
        continue;
      }
      if (mentionsBelowNeck(item)) dropped.push(item);
      else kept.push(item);
    }
    return kept;
  };
  return { kept: filterList(prompt).join(", "), dropped };
}

/**
 * Filter a sentence prompt. Clauses that mention something below the neck
 * lose the part that does: "wearing a sailor uniform with a pleated skirt and
 * knee socks" keeps "wearing a sailor uniform".
 */
export function neckUpProse(prompt: string): PortraitFilterResult {
  const dropped: string[] = [];
  const clauses = prompt.split(/([,;]\s*|(?<=[.!?])\s+)/);
  const out: string[] = [];
  // Separators sit at odd indexes; each one belongs to the clause before it.
  for (let i = 0; i < clauses.length; i += 2) {
    const clause = clauses[i];
    const sep = clauses[i + 1] ?? "";
    if (!mentionsBelowNeck(clause)) {
      out.push(clause + sep);
      continue;
    }
    const kept = trimClause(clause, dropped);
    if (kept) out.push(kept + sep);
    else if (/[.!?]\s*$/.test(clause) && out.length > 0 && !/[.!?]\s*$/.test(out[out.length - 1])) {
      // A sentence that ends here keeps its full stop on what came before.
      out[out.length - 1] = out[out.length - 1].replace(/[,;]\s*$/, "") + ". ";
    }
  }
  const kept = out
    .join("")
    .replace(/\s+([,.;])/g, "$1")
    .replace(/[,;]\s*$/, "")
    .trim();
  return { kept, dropped };
}

/**
 * One clause with its below-the-neck parts cut at "with" and "and". When the
 * clause's own head is what goes ("She is holding a katana and smiling"), the
 * whole clause goes, since what is left would not read as a sentence.
 */
function trimClause(clause: string, dropped: string[]): string {
  const end = /[.!?]$/.test(clause.trim()) ? clause.trim().slice(-1) : "";
  const body = end ? clause.trim().slice(0, -1) : clause.trim();
  const pieces = body.split(/\s+(with|and|plus|while)\s+/i);
  const parts: { sep: string; text: string }[] = [{ sep: "", text: pieces[0] }];
  for (let i = 1; i < pieces.length; i += 2) {
    const sep = pieces[i];
    const text = pieces[i + 1] ?? "";
    const prev = parts[parts.length - 1];
    const lastWord = prev.text.trim().split(/\s+/).pop()?.toLowerCase() ?? "";
    const firstWord = text.trim().split(/\s+/)[0]?.toLowerCase() ?? "";
    if (/^and$/i.test(sep) && COLOURS.has(lastWord) && COLOURS.has(firstWord)) {
      prev.text = `${prev.text} ${sep} ${text}`;
    } else {
      parts.push({ sep, text });
    }
  }
  if (mentionsBelowNeck(parts[0].text)) {
    dropped.push(body.trim());
    return "";
  }
  const kept: { sep: string; text: string }[] = [];
  let carry: string | null = null;
  for (const part of parts) {
    if (mentionsBelowNeck(part.text)) {
      dropped.push(part.text.trim());
      // The first cut part's joining word goes to the next kept part.
      if (carry === null) carry = part.sep;
      continue;
    }
    kept.push({ sep: carry ?? part.sep, text: part.text });
    carry = null;
  }
  if (kept.length === 0) return "";
  const text = kept.map((p, i) => (i === 0 ? p.text : `${p.sep} ${p.text}`)).join(" ").trim();
  return text + end;
}
