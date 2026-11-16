/**
 * Where the tag name sits inside a prompt fragment that may carry weight syntax.
 *
 * Autocomplete and the spellcheck menu replace "the tag" with a suggestion.
 * The fragment they find runs from one comma to the next, so it can include a
 * weight's opener or closer: `-2::lowres` at the start of a NovelAI span,
 * `sketch::` at its end, `{sketch}`, `(sketch:1.2)`, or `b:1.2)` as the last
 * tag of an A1111 group. Replacing the whole fragment deletes that syntax, and
 * losing a closing `::` silently spreads the weight over every tag after it.
 *
 * This finds the bare tag between that syntax, so only it is replaced.
 */
export interface TagCore {
  /** Start of the tag name, after any weight opener or brackets. */
  start: number;
  /** End of the tag name, before any closer that belongs to the syntax around it. */
  end: number;
}

const OPENERS: Record<string, string> = { "(": ")", "[": "]", "{": "}" };
const CLOSERS: Record<string, string> = { ")": "(", "]": "[", "}": "{" };

function isEscaped(text: string, i: number): boolean {
  let slashes = 0;
  for (let j = i - 1; j >= 0 && text[j] === "\\"; j--) slashes++;
  return slashes % 2 === 1;
}

/** NovelAI numeric weight opener at `from`: `1.2::`, `-2::`, `.5::`. */
const NAI_OPENER = /^-?(?:\d+\.?\d*|\.\d+)::/;

/**
 * Brackets left open, and whether a NovelAI `::` span is open, at `upTo`.
 *
 * A `::` pair opens and closes a span, so an odd count before the tag means
 * the tag sits inside one. Escaped brackets are literal characters.
 */
function openState(text: string, upTo: number): { depth: Record<string, number>; spanOpen: boolean } {
  const depth: Record<string, number> = { "(": 0, "[": 0, "{": 0 };
  let colons = 0;
  for (let i = 0; i < upTo; i++) {
    const ch = text[i];
    if (ch === ":" && text[i + 1] === ":" && i + 1 < upTo) {
      colons++;
      i++;
      continue;
    }
    if (isEscaped(text, i)) continue;
    if (OPENERS[ch]) depth[ch]++;
    else if (CLOSERS[ch] && depth[CLOSERS[ch]] > 0) depth[CLOSERS[ch]]--;
  }
  return { depth, spanOpen: colons % 2 === 1 };
}

/**
 * The tag name inside `text[start, end)`, without the weight syntax around it.
 *
 * Leading: NovelAI `N::` openers and unescaped `(`, `[`, `{`.
 * Trailing: a `::` that closes an open NovelAI span, and `)`, `]`, `}` that
 * close a bracket opened before the tag (with an A1111 `:1.2` before a `)`).
 * A closer whose opener is part of the tag itself, such as the `)` in
 * `hatsune miku (vocaloid)` or an emoticon like `:)`, stays with the tag.
 */
export function findTagCore(text: string, start: number, end: number): TagCore {
  let coreStart = start;
  for (;;) {
    const rest = text.slice(coreStart, end);
    const nai = rest.match(NAI_OPENER);
    if (nai) {
      coreStart += nai[0].length;
    } else if (OPENERS[rest[0]] && !isEscaped(text, coreStart)) {
      coreStart += 1;
    } else {
      break;
    }
    while (coreStart < end && /\s/.test(text[coreStart])) coreStart++;
  }

  const { depth, spanOpen } = openState(text, coreStart);
  let closeSpan = spanOpen;
  let coreEnd = end;
  for (;;) {
    if (coreEnd <= coreStart) break;
    const last = text[coreEnd - 1];
    if (closeSpan && last === ":" && text[coreEnd - 2] === ":" && coreEnd - 2 >= coreStart) {
      closeSpan = false;
      coreEnd -= 2;
    } else if (CLOSERS[last] && !isEscaped(text, coreEnd - 1) && depth[CLOSERS[last]] > 0) {
      depth[CLOSERS[last]]--;
      coreEnd -= 1;
      if (last === ")") {
        const weight = text.slice(coreStart, coreEnd).match(/:\s*-?(?:\d+\.?\d*|\.\d+)\s*$/);
        if (weight) coreEnd -= weight[0].length;
      }
    } else {
      break;
    }
    while (coreEnd > coreStart && /\s/.test(text[coreEnd - 1])) coreEnd--;
  }

  if (coreEnd <= coreStart) return { start, end };
  return { start: coreStart, end: coreEnd };
}
