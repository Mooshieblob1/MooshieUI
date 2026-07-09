// Lightweight CLIP prompt-token estimator.
//
// SD/SDXL text encoders (CLIP) tokenize a prompt into chunks of 75 tokens
// (77 with the BOS/EOS markers), and each chunk is encoded independently, so
// crossing a 75-token boundary can subtly change the result. This gives the
// prompt boxes a live token gauge against that boundary.
//
// Exact CLIP tokenization needs the ~49k-entry BPE vocab/merges we do not ship,
// so this is a heuristic approximation. ComfyUI parses weighting, scheduling and
// LoRA syntax out *before* tokenizing, so we strip that control syntax first and
// count only the visible tag text — otherwise weighted prompts would read far
// higher than what actually reaches the encoder. The estimate is close for the
// short, common tag and natural-language words these prompts are made of.

export const CLIP_CHUNK_SIZE = 75;

// Pre-tokenization roughly mirrors CLIP's regex: contractions, letter runs,
// single digits (CLIP emits one token per digit), and punctuation runs.
const TOKEN_PATTERN = /'s|'t|'re|'ve|'m|'ll|'d|[a-z]+|[0-9]|[^\sa-z0-9]+/g;

/** Remove the control syntax ComfyUI resolves before tokenizing, keeping tag text. */
function stripPromptSyntax(text: string): string {
  return (
    text
      // Inline LoRA / embedding / hypernetwork tags (applied outside CLIP text).
      .replace(/<(?:lora|lyco|hypernet|embedding|emb):[^>]*>/gi, " ")
      // MooshieUI scheduling markers: <from:0.2>…</from>, <to:…>, <range:…>.
      .replace(/<\/?(?:from|to|range)(?::[\d.]+(?::[\d.]+)?)?>/gi, " ")
      // SwarmUI scheduling prefix: <fromto[0.5]:before, after>.
      .replace(/<fromto\[[\d.]+\]:/gi, " ")
      // NEW: InvokeAI trailing explicit weight after a group close: )0.8
      // Must precede the paren strip below, which would otherwise eat the ")".
      .replace(/\)\d+(?:\.\d+)?(?=[\s,(){}\[\]]|$)/g, " ")
      // NEW: InvokeAI +/- emphasis runs attached to a word or ) boundary.
      // Must precede the paren strip below; paren strip replaces ")" with a space,
      // which breaks the lookbehind that anchors on ")" (e.g. "(cat ears)++").
      .replace(/(?<=\w|\))(\++|-+)(?=\s|,|$|\)|\}|\])/g, " ")
      // Emphasis grouping characters — the inner tag text is what gets encoded.
      .replace(/[(){}\[\]]/g, " ")
      // Explicit weights like ":1.2" (including step-schedule ":0.5").
      .replace(/:\s*\d+(?:\.\d+)?/g, " ")
  );
}

/** Estimate how many CLIP tokens a prompt encodes to (0 for empty prompts). */
export function estimatePromptTokens(prompt: string): number {
  const matches = stripPromptSyntax(prompt).toLowerCase().match(TOKEN_PATTERN);
  if (!matches) return 0;
  let count = 0;
  for (const piece of matches) {
    // Long letter runs are more likely to split into multiple BPE pieces;
    // everything else counts as a single token.
    count += /^[a-z]+$/.test(piece) ? Math.max(1, Math.ceil(piece.length / 8)) : 1;
  }
  return count;
}

/**
 * Rounded-up chunk capacity for a token count: 75 while it fits one chunk, then
 * 150, 225, … — the familiar "current / limit" denominator SD tools show.
 */
export function promptTokenLimit(tokens: number): number {
  return Math.max(CLIP_CHUNK_SIZE, Math.ceil(tokens / CLIP_CHUNK_SIZE) * CLIP_CHUNK_SIZE);
}
