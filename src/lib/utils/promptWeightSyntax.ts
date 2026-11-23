/**
 * Send-time translation between the prompt weight syntaxes MooshieUI accepts.
 *
 * The prompt box holds whatever the user typed or imported: A1111 `(tag:1.2)`,
 * NovelAI `1.2::tag::` / `{tag}` / `[tag]`, InvokeAI `(tag)1.2` / `tag+`.
 * Nothing rewrites it there. It is translated only when a request is built,
 * and only as far as the backend needs:
 *
 * - ComfyUI reads `(tag:1.2)` and nothing else, so every other syntax is
 *   converted to it.
 * - NovelAI reads its own syntax, so that part goes out exactly as written.
 *   Converting it to ComfyUI syntax here and back in Rust was lossy: it split
 *   `1.2::a, b::` into one span per tag, rounded weights and lost the sign of
 *   `-1::hat::`. Only the InvokeAI forms are converted, to `(tag:1.2)`, which
 *   `prompt_syntax::to_novelai` then turns into NovelAI syntax along with any
 *   A1111 weights the user typed. Where a NovelAI request's prompt does reach
 *   ComfyUI (the local refine pass), Rust translates it there.
 *
 * Leaf util: it must not import any store.
 */

/** Which backend a prompt is being built for. */
export type PromptWeightBackend = "comfyui" | "novelai";

/**
 * NAI numeric weight: `N::text::`, where text has no colon. The optional minus
 * is only part of the number at the start of the prompt or after a separator,
 * so a hyphenated word that ends in a digit keeps its hyphen.
 */
const NAI_NUMERIC_WEIGHT_RE = /((?:(?<=^|[\s,(\[{:])-)?\d+\.?\d*)::([^:]+)::/g;

/**
 * Translate NAI-style weight syntax to ComfyUI (tag:weight) syntax.
 * - {text} -> (text:1.05)     each layer multiplies by 1.05
 * - [text] -> (text:0.95)     each layer divides by 1.05
 * - 1.1::text:: -> (text:1.10)   numeric weight prefix
 * - -1::text:: -> (text:-1.00)   negative weight: the sign belongs to the
 *   number when it starts the prompt or follows a separator, the same rule as
 *   `from_novelai` in `novelai/prompt_syntax.rs`
 * Processes innermost brackets first, so nesting works: {{tag}} -> ((tag:1.05):1.05)
 */
export function translateNaiWeightSyntax(prompt: string): string {
  prompt = prompt.replace(NAI_NUMERIC_WEIGHT_RE, (_m, weight, text) => {
    return `(${text.trim()}:${parseFloat(weight).toFixed(2)})`;
  });

  // Process innermost {text} -> (text:1.05) repeatedly
  let prev: string;
  do {
    prev = prompt;
    prompt = prompt.replace(/\{([^{}]+)\}/g, (_m, inner) => `(${inner}:1.05)`);
  } while (prompt !== prev);

  // Process innermost [text] -> (text:0.95) repeatedly
  // Skip escaped brackets \[ and \]
  do {
    prev = prompt;
    prompt = prompt.replace(/(?<!\\)\[([^\[\]]+)\]/g, (_m, inner) => `(${inner}:0.95)`);
  } while (prompt !== prev);

  return prompt;
}

/**
 * Translate InvokeAI/compel weight + emphasis syntax to ComfyUI (tag:weight).
 * - (group)0.8      -> (group:0.8)      explicit weight, number OUTSIDE the paren
 * - (group)+ / ++   -> (group:1.10) / (group:1.21)   group emphasis, 1.1^n
 * - (group)- / --   -> (group:0.90) / (group:0.81)   group de-emphasis, 0.9^n
 * - word+ / word--  -> (word:1.10) / (word:0.81)     bareword emphasis
 * Guard: bareword rewrite only fires when the base (token minus the trailing
 * +/- run) contains an ASCII letter, so emoticon tags like +_+ and bare ++ / 1+
 * are left untouched. Blend/swap operators are intentionally not handled.
 *
 * Escape: a backslash directly before a trailing +/- run keeps it a literal
 * character instead of emphasis — e.g. a crossover tag "nero (bride)\+astolfo"
 * or a character name "La\+ darkness" whose name is not emphasis syntax. The
 * escaping backslash is stripped from the output either way.
 */
export function translateInvokeAiWeightSyntax(prompt: string): string {
  const emphasisWeight = (marks: string): string => {
    const base = marks[0] === "+" ? 1.1 : 0.9;
    return Math.pow(base, marks.length).toFixed(2);
  };

  // 1. Group trailing explicit weight: (group)0.8 -> (group:0.8)
  prompt = prompt.replace(
    /\(([^()]+)\)(\d+\.?\d*)(?=[\s,(){}\[\]]|$)/g,
    (_m, inner, weight) => `(${inner}:${weight})`,
  );

  // 2. Group emphasis: (group)+++ / (group)--- -> (group:W). Innermost-first.
  // An escaped run is left untouched here (returned byte-for-byte) so the loop
  // still converges; the escaping backslash is stripped once, after the loop,
  // so the now-literal +/- doesn't get re-matched and wrongly converted on a
  // later pass.
  let prev: string;
  do {
    prev = prompt;
    prompt = prompt.replace(
      /\(([^()]+)\)(\\(?:\++|-+)|\++|-+)/g,
      (_m, inner, marks) => {
        if (marks[0] === "\\") return `(${inner})${marks}`;
        return `(${inner}:${emphasisWeight(marks)})`;
      },
    );
  } while (prompt !== prev);
  prompt = prompt.replace(/\)\\(\++|-+)/g, ")$1");

  // 3. Bareword emphasis: tokenize on delimiters, rewrite word+ / word- when the
  // base has a letter. Delimiters: whitespace , ( ) { }
  prompt = prompt.replace(/[^\s,(){}]+/g, (token) => {
    const m = token.match(/^(.*?)(\\?)(\++|-+)$/);
    if (!m) return token;
    const [, base, esc, marks] = m;
    if (esc) return base + marks;
    if (!/[a-zA-Z]/.test(base)) return token;
    return `(${base}:${emphasisWeight(marks)})`;
  });

  return prompt;
}

/**
 * Translate a prompt's weight syntax for the backend it is about to be sent to.
 * See the module comment for what each backend gets.
 */
export function translatePromptWeightSyntax(
  prompt: string,
  backend: PromptWeightBackend = "comfyui",
): string {
  const translated = translateInvokeAiWeightSyntax(prompt);
  return backend === "novelai" ? translated : translateNaiWeightSyntax(translated);
}
