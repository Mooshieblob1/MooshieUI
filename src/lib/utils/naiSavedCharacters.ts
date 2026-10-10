/**
 * The user's saved NovelAI characters, as a block of the V5 rewrite's system
 * prompt, so "an image of Julie" means the saved Julie Evergreen rather than
 * whoever the model would otherwise make up.
 *
 * Every saved character goes out, not only the ones whose name appears in the
 * text: matching a nickname or a misspelling is the model's job, and a string
 * match here would quietly decide it. The caps keep a large roster from eating
 * the prompt budget.
 *
 * Leaf util, like `naiPrompt.ts`: it takes plain data and imports no store.
 */

/**
 * Which side wins when a saved canon character's card disagrees with canon:
 * the saved details (the default) or the canon look. The user picks after
 * seeing the differences the first rewrite lists in its CANON field.
 */
export type NaiCanonPriority = "saved" | "canon";

export interface NaiSavedCharacter {
  name: string;
  prompt: string;
  negative: string;
  /** Name of the character this is a variant of (another look), if any. */
  variantOf?: string;
}

/** Most saved characters sent per request. */
export const NAI_SAVED_CHARACTER_LIMIT = 40;

/** Longest saved prompt or UC sent per character, in characters. */
const FIELD_LIMIT = 800;

function clip(text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > FIELD_LIMIT ? `${flat.slice(0, FIELD_LIMIT).trimEnd()} …` : flat;
}

/** The SAVED CHARACTERS block, or "" when there are none to send. */
export function naiSavedCharacterDirective(
  characters: NaiSavedCharacter[],
  canonPriority: NaiCanonPriority = "saved",
): string {
  const roster = characters
    .filter((c) => c.name.trim() && c.prompt.trim())
    .slice(0, NAI_SAVED_CHARACTER_LIMIT);
  if (roster.length === 0) return "";
  const entries = roster
    .map((c) => {
      const lines = [
        `- Name: ${c.name.trim()}${c.variantOf ? ` (a variant of ${c.variantOf.trim()})` : ""}`,
        `  Appearance: ${clip(c.prompt)}`,
      ];
      if (c.negative.trim()) lines.push(`  Avoid: ${clip(c.negative)}`);
      return lines.join("\n");
    })
    .join("\n");
  return `SAVED CHARACTERS
The user has saved these characters of their own. Each has a name, the prompt text that draws them, and things to avoid when drawing them.
${entries}

- When the user's text refers to a person by a saved character's full name, by part of it (a first name or a surname alone), or by a close approximation of it (a nickname, a short form, a misspelling), that person is the saved character. Use them; do not ask and do not invent someone new.
- The exception is text that plainly means someone else: a series, a surname or a description that does not fit the saved character ("Julie from Persona" is not a saved Julie Evergreen whose saved text has nothing to do with Persona). Context that fits the saved character, such as the series their name or saved text belongs to, still means the saved character ("Brock from Pokemon" is a saved Brock who is the Pokemon Brock). A bare name with no such context always means the saved character.
- If more than one saved character could fit, pick the closest match on the name. Never merge two saved characters into one.
- A variant is another look of the character it belongs to (another outfit or style), not a different person. A reference to the character uses the character's own entry, unless the text asks for what a variant's name describes, such as "Julie in her swimsuit" for a variant named "Julie Evergreen (swimsuit)". Either way the CHAR block names the character, not the variant label.
- A saved character the text refers to gets their own CHAR block. Its identity section uses the saved name, then the saved appearance. Rewrite the appearance into the CHAR block's sections and V5 form as you go, but keep every detail it gives: hair, eyes, body, features and outfit.
- Some saved characters are canon characters you already know, such as Brock from Pokemon: their name or saved text matches a known character. For those, use both. Write their danbooru character and series tags and draw on your own knowledge of their canon look, and add the saved details on top. ${
    canonPriority === "saved"
      ? "Where the saved text and canon differ, the saved text wins, because the user saved it on purpose."
      : "Where the saved text and canon differ, canon wins: the user asked for the canon look over their saved details. Saved details canon says nothing about still apply."
  }
- Whenever a saved canon character is in the image and their saved text disagrees with canon on something (hair, eyes, build, outfit, features), list every disagreement in a CANON field after the CHAR blocks and before any NOTE line: the label CANON: on its own line, then one line per difference in the form <name>: <what> (saved: <saved detail>; canon: <canon detail>). List them whichever side won. Leave the field out when nothing differs.
- A saved character you do not recognise is the user's original. Their appearance comes only from the saved text, never from a canon character who happens to share the name.
- The user's text wins over both the saved text and canon where it changes something. If they ask for her in a swimsuit, the swimsuit replaces the saved outfit; everything else saved still holds.
- Add a saved character's avoid list to UC, skipping anything already there and anything that would rule out another character in the image or something the user asked for.
- A saved character's appearance counts as something the user gave you, so it is not invention.
- Never bring in a saved character the text does not refer to. This list is reference, not a cast.`;
}
