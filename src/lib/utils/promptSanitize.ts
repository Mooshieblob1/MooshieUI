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
