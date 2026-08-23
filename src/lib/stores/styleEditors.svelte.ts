/**
 * Open-state for the artist-style and prompt-chunk editors.
 *
 * Both editors are full-screen modals, but they used to be mounted inside the
 * Styles tab of the bottom panel, so they could only be opened from there and
 * their `position: fixed` root was trapped by the mobile panel's transformed
 * wrapper. Holding the open state here lets App.svelte mount each editor once
 * at the app root: any part of the UI can open one, and the overlay always
 * covers the whole window instead of the panel it was launched from.
 */
class StyleEditorsStore {
  /** Artist style being edited, or null when the style editor is closed. */
  styleId = $state<string | null>(null);
  /** Prompt chunk being edited, or null when the chunk editor is closed. */
  presetId = $state<string | null>(null);

  openStyle(id: string): void {
    this.styleId = id;
  }

  closeStyle(): void {
    this.styleId = null;
  }

  openPreset(id: string): void {
    this.presetId = id;
  }

  closePreset(): void {
    this.presetId = null;
  }
}

export const styleEditors = new StyleEditorsStore();
