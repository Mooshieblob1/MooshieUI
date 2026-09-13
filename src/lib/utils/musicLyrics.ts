/** Insert a section at a line boundary without replacing any existing lyrics. */
export function insertLyricSection(lyrics: string, section: "verse" | "chorus", cursor: number) {
  let position = Number.isFinite(cursor) ? Math.min(lyrics.length, Math.max(0, Math.trunc(cursor))) : lyrics.length;
  const newline = lyrics.includes("\r\n") ? "\r\n" : "\n";
  if (position > 0 && lyrics[position - 1] !== "\n" && position < lyrics.length) {
    const nextLine = lyrics.indexOf("\n", position);
    position = nextLine < 0 ? lyrics.length : nextLine;
    if (lyrics[position - 1] === "\r") position--;
  }
  const before = lyrics.slice(0, position);
  const after = lyrics.slice(position);
  const gap = !before || before.endsWith(newline + newline) ? "" : before.endsWith(newline) ? newline : newline + newline;
  const inserted = `${gap}[${section}]${newline}`;
  const separator = after && !after.startsWith(newline) ? newline : "";
  return { text: before + inserted + separator + after, cursor: before.length + inserted.length };
}
