/** Prompt IDs whose ComfyUI output should not be saved to the gallery (regional inpaint chain intermediates). */
const suppressGalleryPromptIds = new Set<string>();

export function suppressRegionalChainGallerySave(promptId: string): void {
  const id = promptId.trim();
  if (id) suppressGalleryPromptIds.add(id);
}

export function shouldSuppressRegionalChainGallerySave(promptId: string): boolean {
  return suppressGalleryPromptIds.has(promptId.trim());
}

export function clearRegionalChainGallerySuppress(promptId: string): void {
  suppressGalleryPromptIds.delete(promptId.trim());
}

export function clearAllRegionalChainGallerySuppress(): void {
  suppressGalleryPromptIds.clear();
}
