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
