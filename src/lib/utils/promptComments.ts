// Prompt line comments.
//
// `//` starts a comment that runs to the end of the line, so a tag or a whole
// line can be switched off without deleting it. The textareas keep the comments;
// they are stripped only when params are built (toParams) and for token counts.
//
// `//` only counts at the start of a line or after whitespace or a comma, so a
// URL or any other `a//b` inside a tag is left alone.

const COMMENT_RE = /(^|[\s,])\/\/[^\n]*/g;

/** [start, end) ranges of every comment, start at the `//`. */
export function findCommentRanges(text: string): Array<[number, number]> {
  const ranges: Array<[number, number]> = [];
  if (!text.includes("//")) return ranges;
  COMMENT_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = COMMENT_RE.exec(text)) !== null) {
    const start = match.index + match[1].length;
    ranges.push([start, match.index + match[0].length]);
  }
  return ranges;
}

export function hasPromptComments(text: string): boolean {
  return findCommentRanges(text).length > 0;
}

/**
 * Remove comments. A line that held only a comment disappears with its line
 * break; a comment after tags is cut along with the whitespace before it.
 */
export function stripPromptComments(text: string): string {
  if (!text || !text.includes("//")) return text;
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const kept: string[] = [];
  for (const line of lines) {
    const ranges = findCommentRanges(line);
    if (ranges.length === 0) {
      kept.push(line);
      continue;
    }
    const cut = line.slice(0, ranges[0][0]).replace(/\s+$/, "");
    if (cut.trim() === "") continue;
    kept.push(cut);
  }
  return kept.join("\n");
}

export interface CommentToggleResult {
  value: string;
  selectionStart: number;
  selectionEnd: number;
}

/**
 * Ctrl+/: comment out the lines the selection touches, or uncomment them when
 * every non-blank one is already commented.
 */
export function toggleLineComments(value: string, selStart: number, selEnd: number): CommentToggleResult {
  const lineStart = value.lastIndexOf("\n", selStart - 1) + 1;
  // A selection that ends right at the start of a line does not include it.
  const effectiveEnd = selEnd > selStart && value[selEnd - 1] === "\n" ? selEnd - 1 : selEnd;
  const nextBreak = value.indexOf("\n", effectiveEnd);
  const lineEnd = nextBreak === -1 ? value.length : nextBreak;

  const block = value.slice(lineStart, lineEnd);
  const lines = block.split("\n");
  const nonBlank = lines.filter((line) => line.trim() !== "");
  const allCommented = nonBlank.length > 0 && nonBlank.every((line) => /^\s*\/\//.test(line));

  let firstDelta = 0;
  const changed = lines.map((line, i) => {
    if (line.trim() === "") return line;
    let next: string;
    if (allCommented) {
      next = line.replace(/^(\s*)\/\/ ?/, "$1");
    } else {
      const indent = line.match(/^\s*/)?.[0] ?? "";
      next = `${indent}// ${line.slice(indent.length)}`;
    }
    const delta = next.length - line.length;
    if (i === 0) firstDelta = delta;
    return next;
  });

  const newBlock = changed.join("\n");
  const newValue = value.slice(0, lineStart) + newBlock + value.slice(lineEnd);
  if (selStart === selEnd) {
    // A bare caret keeps its place in the text it was in.
    const caret = Math.max(lineStart, selStart + firstDelta);
    return { value: newValue, selectionStart: caret, selectionEnd: caret };
  }
  // A selection grows to cover the whole lines it toggled.
  return { value: newValue, selectionStart: lineStart, selectionEnd: lineStart + newBlock.length };
}
