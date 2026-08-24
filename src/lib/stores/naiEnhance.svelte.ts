import { generation } from "./generation.svelte.js";
import { createNovelAiCharacter } from "./generation.svelte.js";
import { NOVELAI_MAX_CHARACTERS } from "../utils/novelaiModels.js";
import type { NovelAiCharacter } from "../types/index.js";
import type { NaiVariant } from "../utils/naiPrompt.js";
import type { NaiLanguage } from "../utils/naiLanguage.js";

/**
 * Review state for the NovelAI V5 rewrite.
 *
 * The other two enhance paths overwrite the prompt box and offer an undo. This
 * one cannot: a V5 rewrite touches the base prompt, the undesired content and
 * every character box at once, so a blind overwrite would silently discard hand
 * written character work the user spent real Anlas tuning. The result is staged
 * here instead, shown as a diff with a checkbox per field, and only the ticked
 * rows are written.
 *
 * A feature store, so depending on `generation` is allowed. Nothing may depend
 * on this one in the other direction. In particular it holds no reference to the
 * prompt assistant: the modal owns that call, so this stays a plain state
 * machine over the two stages.
 */

/**
 * Which half of the modal is showing.
 *
 * `input` is where the user types what they want written; `review` is the diff
 * gate over what came back. One modal rather than two because the second is the
 * answer to the first, and cancelling out of either means the same thing:
 * nothing was written.
 */
export type NaiEnhanceStage = "input" | "review";

/** One reviewable field: what is there now, what the rewrite proposes. */
export interface NaiEnhanceRow {
  before: string;
  after: string;
  selected: boolean;
}

export interface NaiEnhanceCharacterRow extends NaiEnhanceRow {
  /**
   * Index into `novelaiSettings.characters`, or null when the rewrite invented
   * a character the user does not have a slot for yet.
   */
  targetIndex: number | null;
}

export interface NaiEnhancePending {
  variant: NaiVariant;
  language: NaiLanguage;
  /** Soft token budget for this variant, for the modal's bar. */
  budget: number;
  /** A `NOTE:` line from the rewrite, usually "this did not fit in Curated". */
  note: string;
  /** Validator problems that survived the retry. Advisory only. */
  problems: string[];
  base: NaiEnhanceRow;
  uc: NaiEnhanceRow;
  characters: NaiEnhanceCharacterRow[];
}

interface UndoSnapshot {
  positivePrompt: string;
  negativePrompt: string;
  characters: NovelAiCharacter[];
}

/** Matches the text-enhance undo window in PromptInputs. */
const UNDO_WINDOW_MS = 10000;

class NaiEnhanceStore {
  stage = $state<NaiEnhanceStage | null>(null);
  /** What the user typed: a prompt to rewrite, or instructions for a new one. */
  input = $state("");
  /** True while the rewrite is in flight, so the modal stays open and busy. */
  busy = $state(false);
  pending = $state<NaiEnhancePending | null>(null);
  showUndo = $state(false);

  private snapshot: UndoSnapshot | null = null;
  private undoTimer: ReturnType<typeof setTimeout> | null = null;

  get isOpen(): boolean {
    return this.stage !== null;
  }

  /** True once at least one row is ticked, which is what enables Apply. */
  get hasSelection(): boolean {
    const p = this.pending;
    if (!p) return false;
    return p.base.selected || p.uc.selected || p.characters.some((c) => c.selected);
  }

  /**
   * Every field that would be sent if the user applied right now.
   *
   * Concatenated the way NovelAI counts it, so the modal's bar reflects the
   * selection rather than the whole rewrite. Unticked rows keep their current
   * text, because that is what will still be there afterwards.
   */
  get selectedText(): string {
    const p = this.pending;
    if (!p) return "";
    const parts = [
      p.base.selected ? p.base.after : p.base.before,
      p.uc.selected ? p.uc.after : p.uc.before,
      ...p.characters.map((c) => (c.selected ? c.after : c.before)),
    ];
    return parts.filter((t) => t.trim() !== "").join(", ");
  }

  /**
   * Open on an empty box.
   *
   * Deliberately empty rather than seeded with the current prompt: the box takes
   * instructions ("make it a rainy night scene") as readily as a prompt, and a
   * prefilled box reads as "edit this", which is the narrower of the two uses.
   * The copy button underneath covers the other case in one click.
   */
  openInput(): void {
    this.stage = "input";
    this.input = "";
    this.busy = false;
    this.pending = null;
  }

  /** Paste the prompt box into the input, for the "tidy up what I have" case. */
  copyExistingPrompt(): void {
    this.input = generation.positivePrompt ?? "";
  }

  showReview(pending: NaiEnhancePending): void {
    this.pending = pending;
    this.busy = false;
    this.stage = "review";
  }

  dismiss(): void {
    this.stage = null;
    this.pending = null;
    this.busy = false;
    this.input = "";
  }

  toggleBase(): void {
    if (!this.pending) return;
    this.pending = {
      ...this.pending,
      base: { ...this.pending.base, selected: !this.pending.base.selected },
    };
  }

  toggleUc(): void {
    if (!this.pending) return;
    this.pending = {
      ...this.pending,
      uc: { ...this.pending.uc, selected: !this.pending.uc.selected },
    };
  }

  toggleCharacter(i: number): void {
    if (!this.pending) return;
    this.pending = {
      ...this.pending,
      characters: this.pending.characters.map((c, idx) =>
        idx === i ? { ...c, selected: !c.selected } : c,
      ),
    };
  }

  setAll(selected: boolean): void {
    if (!this.pending) return;
    this.pending = {
      ...this.pending,
      base: { ...this.pending.base, selected },
      uc: { ...this.pending.uc, selected },
      characters: this.pending.characters.map((c) => ({ ...c, selected })),
    };
  }

  /**
   * Write the ticked rows and close.
   *
   * Character slots are built in one pass rather than through
   * `addNovelAiCharacter`, so that a rewrite proposing three new characters
   * cannot leave two of them written and the third dropped at the cap.
   */
  apply(): void {
    const p = this.pending;
    if (!p) return;

    this.snapshot = {
      positivePrompt: generation.positivePrompt,
      negativePrompt: generation.negativePrompt,
      characters: generation.novelaiSettings.characters.map((c) => ({
        ...c,
        center: { ...c.center },
      })),
    };

    if (p.base.selected) generation.positivePrompt = p.base.after;
    if (p.uc.selected) generation.negativePrompt = p.uc.after;

    const chars = generation.novelaiSettings.characters.map((c) => ({ ...c }));
    for (const row of p.characters) {
      if (!row.selected) continue;
      if (row.targetIndex !== null && row.targetIndex < chars.length) {
        chars[row.targetIndex] = { ...chars[row.targetIndex], prompt: row.after };
      } else if (chars.length < NOVELAI_MAX_CHARACTERS) {
        chars.push({ ...createNovelAiCharacter(), prompt: row.after });
      }
    }
    generation.updateNovelAiSettings({ characters: chars });
    generation.saveSettings();

    this.stage = null;
    this.pending = null;
    this.input = "";
    this.showUndo = true;
    if (this.undoTimer) clearTimeout(this.undoTimer);
    this.undoTimer = setTimeout(() => (this.showUndo = false), UNDO_WINDOW_MS);
  }

  /** Restore everything the last apply touched, including untouched slots. */
  undo(): void {
    const snap = this.snapshot;
    if (snap) {
      generation.positivePrompt = snap.positivePrompt;
      generation.negativePrompt = snap.negativePrompt;
      generation.updateNovelAiSettings({ characters: snap.characters });
      generation.saveSettings();
      this.snapshot = null;
    }
    this.showUndo = false;
    if (this.undoTimer) clearTimeout(this.undoTimer);
    this.undoTimer = null;
  }
}

export const naiEnhance = new NaiEnhanceStore();
