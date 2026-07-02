import { triggerSync } from "../utils/syncTrigger.js";

const NOTES_KEY = "mooshieui.notes.v1";

class NotesStore {
  text = $state("");
  private saveTimer: ReturnType<typeof setTimeout> | null = null;

  constructor() {
    this.loadSettings();
  }

  loadSettings() {
    try {
      const raw = localStorage.getItem(NOTES_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw);
      if (typeof parsed?.text === "string") {
        this.text = parsed.text;
      }
    } catch (e) {
      console.error("Failed to load notes:", e);
    }
  }

  /** Update the note text and persist after a short debounce (typing is high-frequency). */
  setText(value: string) {
    this.text = value;
    if (this.saveTimer !== null) clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => {
      this.saveTimer = null;
      this.saveSettings();
    }, 500);
  }

  /** Flush a pending debounced save immediately (e.g. on blur). */
  flush() {
    if (this.saveTimer !== null) {
      clearTimeout(this.saveTimer);
      this.saveTimer = null;
      this.saveSettings();
    }
  }

  saveSettings() {
    try {
      localStorage.setItem(NOTES_KEY, JSON.stringify({ text: this.text }));
      triggerSync();
    } catch (e) {
      console.error("Failed to save notes:", e);
    }
  }

  collectPrefs(): unknown {
    return { text: this.text };
  }

  applyServerPrefs(data: any): void {
    try {
      if (typeof data?.text === "string") {
        this.text = data.text;
        localStorage.setItem(NOTES_KEY, JSON.stringify({ text: this.text }));
      }
    } catch (e) {
      console.error("Failed to apply server prefs (notes):", e);
    }
  }
}

export const notes = new NotesStore();
