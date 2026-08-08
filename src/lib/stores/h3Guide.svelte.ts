/**
 * Open/closed state for the H3 prompting guide.
 *
 * The trigger button lives in the prompt panel (H3PromptGuide.svelte) but the
 * guide itself renders over the preview column (GenerationPage.svelte) so the
 * side panels stay lit and typeable while it is open. Those two are in
 * different subtrees, so the flag has to live outside both.
 *
 * Leaf store: imports nothing, so nothing can cycle through it.
 */
class H3GuideStore {
  open = $state(false);

  show(): void {
    this.open = true;
  }

  hide(): void {
    this.open = false;
  }

  toggle(): void {
    this.open = !this.open;
  }
}

export const h3Guide = new H3GuideStore();
