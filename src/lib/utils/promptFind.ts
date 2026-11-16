// Plain-text find and replace for the prompt boxes. No regex syntax: what the
// user types is what gets matched.

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** [start, end) of every non-overlapping match, left to right. */
export function findMatches(text: string, query: string, caseSensitive = false): Array<[number, number]> {
  if (!query) return [];
  const re = new RegExp(escapeRegExp(query), caseSensitive ? "g" : "gi");
  const ranges: Array<[number, number]> = [];
  let match: RegExpExecArray | null;
  while ((match = re.exec(text)) !== null) {
    ranges.push([match.index, match.index + match[0].length]);
  }
  return ranges;
}

export function replaceAllMatches(text: string, query: string, replacement: string, caseSensitive = false): string {
  if (!query) return text;
  const re = new RegExp(escapeRegExp(query), caseSensitive ? "g" : "gi");
  // A function replacement keeps `$&` and friends in the replacement literal.
  return text.replace(re, () => replacement);
}
