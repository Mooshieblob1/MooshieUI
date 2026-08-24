/** True when `index` is escaped by an odd number of consecutive backslashes. */
export function isBackslashEscaped(raw: string, index: number): boolean {
  let slashCount = 0;
  for (let i = index - 1; i >= 0 && raw[i] === "\\"; i--) {
    slashCount += 1;
  }
  return slashCount % 2 === 1;
}

/**
 * Expression tags like `:<`, `>:<`, and `d:<` use `:` to mark a literal `<`.
 * Those angles must not open schedule/XML syntax parsing.
 */
export function isColonEscapedAngle(raw: string, index: number): boolean {
  return (
    raw[index] === "<" &&
    index > 0 &&
    raw[index - 1] === ":" &&
    !isBackslashEscaped(raw, index - 1)
  );
}

/** True when `<` at `index` opens MooshieUI schedule / markup syntax. */
export function isSyntaxAngleOpen(raw: string, index: number): boolean {
  return raw[index] === "<" && !isBackslashEscaped(raw, index) && !isColonEscapedAngle(raw, index);
}

/** True when token text contains `<`/`>` that are not colon-escaped literals. */
export function hasUnescapedSyntaxAngles(token: string): boolean {
  for (let i = 0; i < token.length; i++) {
    if (isBackslashEscaped(token, i)) continue;
    const ch = token[i];
    if (ch === "<" && !isColonEscapedAngle(token, i)) return true;
    if (ch === ">" && !(i > 0 && token[i - 1] === ":")) return true;
  }
  return false;
}

/** Regex lookbehind: `<` not immediately preceded by `:`. */
export const SYNTAX_ANGLE_LOOKBEHIND = "(?<![:])";

/** Token delimiters the emphasis translator splits on before looking for a trailing run. */
const EMPHASIS_TOKEN_RE = /[^\s,(){}]+/g;

/**
 * Backslash-escape a trailing `+`/`-` run in each token of `text`.
 *
 * The send-time translator (`translateInvokeAiWeightSyntax` in
 * `generation.svelte.ts`) reads a trailing `+`/`-` run as InvokeAI emphasis
 * weighting, so a tag whose *name* ends in one — `blood+`, `k+`, `9-nine-`,
 * `grs-` — is otherwise rewritten to `(blood:1.10)` / `(9-nine:0.90)` and the
 * literal character is lost. The translator honours a backslash escape and
 * strips it again on the way out, so anything writing a tag name into a prompt
 * has to add it, exactly like it already escapes `(` and `)`.
 *
 * Mirrors the translator's own guard: a run is only escaped when the base
 * before it contains an ASCII letter, which is exactly the condition under
 * which the translator would rewrite it. Emoticon tags like `+_+` and bare
 * `++` are therefore left alone, since those already pass through untouched.
 *
 * Call this *after* escaping parens, not before, so it tokenizes the same text
 * the translator will: in `moon knight \(disney+\)` the `+` is already shielded
 * by the trailing `\)` and escaping it again would leak a stray backslash.
 */
export function escapeEmphasisMarks(text: string): string {
  return text.replace(EMPHASIS_TOKEN_RE, (token) => {
    const m = token.match(/^(.*?)(\++|-+)$/);
    if (!m) return token;
    const [, base, marks] = m;
    // Already escaped — don't double up.
    if (base.endsWith("\\")) return token;
    if (!/[a-zA-Z]/.test(base)) return token;
    return `${base}\\${marks}`;
  });
}

/** Strip the escaping backslash from `\+` / `\-`, for comparisons and lookups. */
export function unescapeEmphasisMarks(text: string): string {
  return text.replace(/\\([+-])/g, "$1");
}
