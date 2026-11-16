// Related tag suggestions: the small pure parts the prompt box uses.
// The lookup itself (Danbooru + the gallery) runs in the backend, see
// src-tauri/src/related_tags.rs, which normalises tags the same way.

/** A prompt tag in plain form: no weights or emphasis, lowercase, spaces. */
export function normalizePromptTag(raw: string): string {
  let tag = raw.trim().replace(/\\\(/g, "\u0001").replace(/\\\)/g, "\u0002");
  const nai = tag.match(/^\s*-?\d+(?:\.\d+)?::/);
  if (nai) tag = tag.slice(nai[0].length);
  tag = tag.replace(/::/g, "");
  const weight = tag.match(/:\s*-?\d+(?:\.\d+)?\s*[)\]}]*\s*$/);
  if (weight) tag = tag.slice(0, weight.index);
  return tag
    .replace(/^[\s()[\]{}]+|[\s()[\]{}]+$/g, "")
    .replace(/_/g, " ")
    .replace(/\u0001/g, "(")
    .replace(/\u0002/g, ")")
    .split(/\s+/)
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
}

/** Every tag already in a prompt, normalised, for dimming suggestions. */
export function promptTagSet(text: string): Set<string> {
  const out = new Set<string>();
  for (const raw of text.split(/[,\n|]/)) {
    const tag = normalizePromptTag(raw);
    if (tag) out.add(tag);
  }
  return out;
}

/**
 * Insert `formatted` as a new tag right after the tag that ends at or after
 * `selEnd`: before the next comma or line break. Returns the new text and the
 * caret just after the inserted tag.
 */
export function insertTagAfter(value: string, selEnd: number, formatted: string): { value: string; caret: number } {
  let end = selEnd;
  while (end < value.length && value[end] !== "," && value[end] !== "\n") end++;
  // Keep a weight's closing bracket or `::` with the tag it belongs to.
  const before = value.slice(0, end).replace(/\s+$/, "");
  const after = value.slice(end);
  const insert = before.length === 0 ? formatted : `, ${formatted}`;
  return { value: before + insert + after, caret: before.length + insert.length };
}
