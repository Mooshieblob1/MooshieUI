// Prompt concatenation + send-time sanitization for the additive prompt boxes.
//
// The prompt textareas hold RAW user text and are never mutated. These pure
// helpers run only when params are built (toParams in generation.svelte.ts):
//
//   joinPromptBoxes  — chains the main box with any extra boxes into one string,
//                      in list (chronological) order, like a ComfyUI string
//                      concatenate node. Empty boxes drop out cleanly.
//
//   sanitizePromptForSend — strips the CLIP-era formatting habits that hurt the
//                      LLM text encoders in split models (Anima/Qwen etc.) and
//                      are dead weight everywhere else: the `BREAK` keyword,
//                      `<break>`, and stray newlines used for visual layout.
//                      ComfyUI's stock CLIPTextEncode does not implement A1111
//                      chunk-splitting, so `BREAK` only ever tokenizes as a
//                      literal word — removing it is safe for every model.

/** Strip whitespace and stray leading/trailing commas from one prompt fragment. */
function trimFragment(text: string): string {
  return text.trim().replace(/^,+\s*|\s*,+$/g, "").trim();
}

/**
 * Concatenate the main prompt with any extra boxes in order, dropping fragments
 * that are empty once trimmed, and joining survivors with ", ".
 */
export function joinPromptBoxes(contents: string[]): string {
  return contents.map(trimFragment).filter((frag) => frag.length > 0).join(", ");
}

export interface SanitizeOptions {
  /**
   * Keep line breaks instead of folding them into ", ". NovelAI reads the
   * prompt line by line: a `Text:` line opens the lettering block, and a blank
   * line inside it separates one rendered string from the next. Folding that
   * layout into commas turns `Text:\nMumei` into `Text:, Mumei`, and NovelAI
   * then renders the comma into the image.
   */
  keepNewlines?: boolean;
}

/**
 * Remove BREAK / <break> tokens and normalize newlines to ", " so the outgoing
 * prompt is a single clean comma-separated string regardless of how the user
 * laid it out in the textarea.
 *
 * With `keepNewlines` every line is cleaned on its own and the line structure
 * survives, with runs of blank lines collapsed to a single blank line.
 */
export function sanitizePromptForSend(prompt: string, options: SanitizeOptions = {}): string {
  if (!prompt) return "";
  const stripped = prompt
    // A1111 chunk keyword — case-sensitive whole word so "BREAKFAST" survives.
    .replace(/\bBREAK\b/g, " ")
    // Angle-bracket form some tools emit; case-insensitive.
    .replace(/<break>/gi, " ");
  if (options.keepNewlines) {
    return stripped
      .replace(/\r\n?/g, "\n")
      .split("\n")
      .map((line) => trimFragment(line).replace(/,\s*(?:,\s*)+/g, ", "))
      .join("\n")
      .replace(/\n{3,}/g, "\n\n")
      .trim();
  }
  return (
    stripped
      // Newlines used for visual layout become tag separators.
      .split("\n")
      .map(trimFragment)
      .filter((line) => line.length > 0)
      .join(", ")
      // Collapse any comma runs left behind into a single ", ".
      .replace(/,\s*(?:,\s*)+/g, ", ")
      .trim()
  );
}

// ---------------------------------------------------------------------------
// NovelAI tag merging
//
// `toParams` merges system fragments (style presets, active Artist Styles,
// Prompt Chunk prepend/append) into the user's prompt. The ComfyUI merge
// re-tokenizes the whole prompt on commas and rejoins with ", ", which is fine
// for a flat tag list but destroys a NovelAI prompt: every line that ends in a
// comma (the normal tag-line layout) loses its newline, so a prompt laid out as
//
//   1girl, rain,
//   Text:
//   Mumei
//
// leaves as `1girl, rain, Text:, Mumei`. The helpers below never re-tokenize
// the prompt. They only look at it to dedupe the incoming tags, splice those
// tags in before the `Text:` block, and hand the rest back byte for byte.
// ---------------------------------------------------------------------------

/** The first `Text:` label that starts a line, with any blank lines before it. */
const NAI_TEXT_BLOCK_RE = /(^|\n)(\s*Text\s*:)/i;

/**
 * Split a NovelAI prompt into the tag/prose head and the `Text:` lettering
 * block. The tail keeps the newline (and any blank lines) that preceded the
 * label so it can be reattached unchanged. `text` is "" when there is no block.
 */
export function splitNovelAiTextBlock(prompt: string): { head: string; text: string } {
  const match = NAI_TEXT_BLOCK_RE.exec(prompt);
  if (!match || match.index === undefined) return { head: prompt, text: "" };
  return { head: prompt.slice(0, match.index), text: prompt.slice(match.index) };
}

/** Tags split on commas or newlines, trimmed, empties dropped. */
function splitLooseTags(text: string): string[] {
  return text
    .split(/[\n,]/)
    .map((part) => part.trim())
    .filter((part) => part.length > 0);
}

/**
 * Merge a comma-separated tag fragment into a NovelAI prompt without touching
 * the prompt's layout.
 *
 * - Tags already present anywhere in the head (case-insensitive) are dropped;
 *   the prompt is returned untouched when nothing new is left.
 * - `"before"` puts the new tags in front of the head, `"after"` puts them at
 *   the end of the head, on the same line unless that line is prose ending in
 *   sentence punctuation, where they start a new line instead.
 * - The `Text:` block always stays last, on its own line, exactly as written.
 */
export function mergeNovelAiTags(prompt: string, tags: string, where: "before" | "after"): string {
  if (!tags) return prompt;
  const { head, text } = splitNovelAiTextBlock(prompt);
  const seen = new Set(splitLooseTags(head).map((tag) => tag.toLowerCase()));
  const fresh: string[] = [];
  for (const tag of splitLooseTags(tags)) {
    const key = tag.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    fresh.push(tag);
  }
  if (fresh.length === 0) return prompt;

  const joined = fresh.join(", ");
  const trimmedHead = head.trim();
  let merged: string;
  if (!trimmedHead) {
    merged = joined;
  } else if (where === "before") {
    merged = `${joined}, ${trimmedHead}`;
  } else {
    const body = trimmedHead.replace(/[,\s]+$/, "");
    merged = /[.!?]$/.test(body) ? `${body}\n${joined}` : `${body}, ${joined}`;
  }
  if (!text) return merged;
  // The tail carries its own leading newline unless the prompt began with the
  // label itself, in which case the block needs one to stay on its own line.
  return text.startsWith("\n") ? `${merged}${text}` : `${merged}\n${text}`;
}
