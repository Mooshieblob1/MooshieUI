/** Model selection always returns an exact filename advertised by ComfyUI. */
export function resolveAvailableModel(
  filename: string,
  available: readonly string[],
  cachedFilename?: string,
): string | undefined {
  const normalize = (name: string) => name.replace(/\\/g, "/");
  const exact = available.find((name) => normalize(name) === normalize(filename));
  if (exact) return exact;
  if (cachedFilename) {
    const cached = available.find((name) => normalize(name) === normalize(cachedFilename));
    if (cached) return cached;
  }
  // A unique basename supports server-side subfolders. Ambiguous names must
  // remain separate choices instead of silently picking a different model.
  const basename = normalize(filename).split("/").pop();
  const matches = available.filter((name) => normalize(name).split("/").pop() === basename);
  return matches.length === 1 ? matches[0] : undefined;
}

export function localOnlyModels(local: readonly string[], available: readonly string[]): string[] {
  return local.filter((name) => !resolveAvailableModel(name, available));
}
