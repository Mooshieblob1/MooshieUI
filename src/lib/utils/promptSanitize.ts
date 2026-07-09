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

/**
 * Remove BREAK / <break> tokens and normalize newlines to ", " so the outgoing
 * prompt is a single clean comma-separated string regardless of how the user
 * laid it out in the textarea.
 */
export function sanitizePromptForSend(prompt: string): string {
  if (!prompt) return "";
  return (
    prompt
      // A1111 chunk keyword — case-sensitive whole word so "BREAKFAST" survives.
      .replace(/\bBREAK\b/g, " ")
      // Angle-bracket form some tools emit; case-insensitive.
      .replace(/<break>/gi, " ")
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
