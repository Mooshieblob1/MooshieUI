import { generation } from "./generation.svelte.js";
import { createNovelAiCharacter } from "./generation.svelte.js";
import { novelAiMaxCharacters } from "../utils/novelaiModels.js";
import type { NovelAiCharacter } from "../types/index.js";
import type { NovelAiSettings } from "./generation.svelte.js";
import type { NaiVariant } from "../utils/naiPrompt.js";
import type { NaiCanonPriority } from "../utils/naiSavedCharacters.js";
import type { NaiLanguage } from "../utils/naiLanguage.js";

/**
 * Review state for the Enhance modal: the NovelAI V5 rewrite, the general
 * Enhance every other image model uses, and the video (H3) rewrite.
 *
 * All three share the input stage (a free text box, reference images, "edit
 * current prompt") and differ only in what they review. The general and H3
 * ones change the prompt box alone, so they review one before/after pair; the
 * V5 one is the multi-field diff described below.
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

/**
 * `nai` is the V5 rewrite, `general` the Enhance for every other image model,
 * `h3` the video rewrite.
 */
export type EnhanceFlow = "nai" | "general" | "h3";

/** A general or H3 rewrite awaiting review: it only ever touches the prompt box. */
export interface GeneralEnhancePending {
  before: string;
  after: string;
  /** Shown above the diff: an H3 format miss, or the idle-mode note. */
  notice?: { kind: "warning" | "info"; text: string } | null;
}

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
  /**
   * True for a box the rewrite dropped, where `after` is empty and applying the
   * row deletes the slot outright.
   *
   * A rewrite that comes back with fewer characters than the user has boxes
   * open has written a base prompt for the smaller cast, so a box left behind
   * puts a character in the image that nothing in the prompt asked for. It is
   * offered as a ticked row rather than cleared silently, because the box may
   * hold work the user tuned by hand.
   */
  removes: boolean;
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
  /**
   * Ways a saved canon character's card differs from canon, from the
   * rewrite's CANON field. Non-empty puts the keep saved / use canon choice
   * on the review.
   */
  canon: string[];
  base: NaiEnhanceRow;
  uc: NaiEnhanceRow;
  characters: NaiEnhanceCharacterRow[];
}

interface UndoSnapshot {
  positivePrompt: string;
  negativePrompt: string;
  characters: NovelAiCharacter[];
  /** Present only when "Curate it" changed the generation settings too. */
  curate?: {
    checkpoint: string;
    steps: number;
    cfg: number;
    width: number;
    height: number;
    batchSize: number;
    novelaiSettings: NovelAiSettings;
  };
}

/** Matches the text-enhance undo window in PromptInputs. */
const UNDO_WINDOW_MS = 10000;

/**
 * How many reference images one rewrite may carry.
 *
 * Mirrors `MAX_VISION_IMAGES` in the Rust vision layer, which enforces it for
 * real: every image is inlined into the request body, and no provider reads a
 * long image list as carefully as a short one. Four is also as many thumbnails
 * as fit on one row of the modal without shrinking them past recognition.
 */
export const NAI_MAX_REFERENCES = 4;

/**
 * One attached image.
 *
 * `base64` is bare PNG payload with no `data:` prefix, already downscaled in
 * the browser by `fileToNovelAiBase64` -- the same helper the NovelAI reference
 * fields use, so a picked gallery image costs the same here as it does there.
 * `label` is the user's optional handle for it ("outfit"), which appears in the
 * manifest line of the user turn so they can name an image instead of counting
 * to it. Blank is the normal case and means the model refers to it by number.
 */
export interface NaiEnhanceReference {
  /** Stable across reorder-free edits, so `{#each}` keys do not remount inputs. */
  id: string;
  base64: string;
  label: string;
}

let referenceSeq = 0;

class NaiEnhanceStore {
  stage = $state<NaiEnhanceStage | null>(null);
  /** Which rewrite the modal is running. Fixed for the life of one opening. */
  flow = $state<EnhanceFlow>("nai");
  /** What the user typed: a prompt to rewrite, or instructions for a new one. */
  input = $state("");
  /** True while the rewrite is in flight, so the modal stays open and busy. */
  busy = $state(false);
  pending = $state<NaiEnhancePending | null>(null);
  generalPending = $state<GeneralEnhancePending | null>(null);
  showUndo = $state(false);
  /** Which flow the undo pill belongs to, for its label. */
  undoFlow = $state<EnhanceFlow>("nai");
  /**
   * Attached reference images, in the order the model will see them.
   *
   * Lives here rather than in the modal so that a rewrite triggered from the
   * input stage still has them during the review stage, where the modal has
   * swapped over to the diff and no longer renders the picker.
   */
  references = $state<NaiEnhanceReference[]>([]);
  /**
   * V5 flow only: applying also switches to V5 Full with the Curated-leaning
   * settings in `NOVELAI_CURATE`. Off on every opening, so it only happens
   * when ticked for this rewrite.
   */
  curate = $state(false);
  /**
   * V5 flow only: which side wins when a saved canon character's card
   * disagrees with canon. Saved on every opening; the review offers the switch
   * once the rewrite has listed differences.
   */
  canonPriority = $state<NaiCanonPriority>("saved");

  private snapshot: UndoSnapshot | null = null;
  private undoTimer: ReturnType<typeof setTimeout> | null = null;

  get isOpen(): boolean {
    return this.stage !== null;
  }

  get canAddReference(): boolean {
    return this.references.length < NAI_MAX_REFERENCES;
  }

  /** How many more images will fit, for the gallery picker's own cap. */
  get referenceSlotsLeft(): number {
    return Math.max(0, NAI_MAX_REFERENCES - this.references.length);
  }

  /**
   * Attach images, silently dropping any past the cap.
   *
   * Takes a list because two of the three sources hand over more than one at a
   * time: a multi-select file picker and the gallery picker. Dropping the tail
   * rather than refusing the batch keeps a five-image drop from being a no-op.
   */
  addReferences(base64: string[]): void {
    const room = this.referenceSlotsLeft;
    if (room <= 0) return;
    const added = base64
      .filter((b) => b.trim() !== "")
      .slice(0, room)
      .map((b) => ({ id: `ref-${++referenceSeq}`, base64: b, label: "" }));
    if (added.length === 0) return;
    this.references = [...this.references, ...added];
  }

  removeReference(id: string): void {
    this.references = this.references.filter((r) => r.id !== id);
  }

  setReferenceLabel(id: string, label: string): void {
    this.references = this.references.map((r) =>
      r.id === id ? { ...r, label } : r,
    );
  }

  clearReferences(): void {
    this.references = [];
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
  openInput(flow: EnhanceFlow = "nai"): void {
    this.flow = flow;
    this.stage = "input";
    this.input = "";
    this.busy = false;
    this.pending = null;
    this.generalPending = null;
    this.references = [];
    this.curate = false;
    this.canonPriority = "saved";
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

  showGeneralReview(pending: GeneralEnhancePending): void {
    this.generalPending = pending;
    this.busy = false;
    this.stage = "review";
  }

  dismiss(): void {
    this.stage = null;
    this.pending = null;
    this.generalPending = null;
    this.busy = false;
    this.input = "";
    this.references = [];
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

    this.takeSnapshot(this.curate);

    if (p.base.selected) generation.positivePrompt = p.base.after;
    if (p.uc.selected) generation.negativePrompt = p.uc.after;

    const chars = generation.novelaiSettings.characters.map((c) => ({ ...c }));
    // Deletions are collected and applied after the writes rather than spliced
    // as they are met, so that every `targetIndex` keeps pointing at the box it
    // was built against. Appended slots land past the end and are never in the
    // set, so the filter cannot catch one.
    const dropped = new Set<number>();
    for (const row of p.characters) {
      if (!row.selected) continue;
      if (row.removes) {
        if (row.targetIndex !== null) dropped.add(row.targetIndex);
      } else if (row.targetIndex !== null && row.targetIndex < chars.length) {
        chars[row.targetIndex] = { ...chars[row.targetIndex], prompt: row.after };
      } else if (chars.length < novelAiMaxCharacters(generation.checkpoint)) {
        chars.push({ ...createNovelAiCharacter(), prompt: row.after });
      }
    }
    generation.updateNovelAiSettings({
      characters: chars.filter((_, i) => !dropped.has(i)),
    });
    // After the rows, so the avoids land on the undesired content the rewrite
    // wrote rather than being overwritten by it.
    if (this.curate) generation.applyNovelAiCurate();
    generation.saveSettings();

    this.finishApply("nai");
  }

  /**
   * Write the general rewrite into the prompt box and close. `append` keeps
   * the current prompt and adds the rewrite after it, the way Compose did.
   */
  applyGeneral(how: "replace" | "append"): void {
    const p = this.generalPending;
    if (!p) return;
    this.takeSnapshot();
    const current = generation.positivePrompt?.trim();
    generation.positivePrompt =
      how === "append" && current ? `${current}, ${p.after}` : p.after;
    generation.saveSettings();
    this.finishApply(this.flow);
  }

  private takeSnapshot(withSettings = false): void {
    this.snapshot = {
      positivePrompt: generation.positivePrompt,
      negativePrompt: generation.negativePrompt,
      characters: generation.novelaiSettings.characters.map((c) => ({
        ...c,
        center: { ...c.center },
      })),
      curate: withSettings
        ? {
            checkpoint: generation.checkpoint,
            steps: generation.steps,
            cfg: generation.cfg,
            width: generation.width,
            height: generation.height,
            batchSize: generation.batchSize,
            novelaiSettings: $state.snapshot(generation.novelaiSettings),
          }
        : undefined,
    };
  }

  private finishApply(flow: EnhanceFlow): void {
    this.stage = null;
    this.pending = null;
    this.generalPending = null;
    this.input = "";
    this.references = [];
    this.undoFlow = flow;
    this.showUndo = true;
    if (this.undoTimer) clearTimeout(this.undoTimer);
    this.undoTimer = setTimeout(() => (this.showUndo = false), UNDO_WINDOW_MS);
  }

  /** Restore everything the last apply touched, including untouched slots. */
  undo(): void {
    const snap = this.snapshot;
    if (snap) {
      const prev = snap.curate;
      if (prev) {
        if (generation.checkpoint !== prev.checkpoint) generation.selectNovelAiModel(prev.checkpoint);
        generation.steps = prev.steps;
        generation.cfg = prev.cfg;
        generation.width = prev.width;
        generation.height = prev.height;
        generation.batchSize = prev.batchSize;
        generation.updateNovelAiSettings(prev.novelaiSettings);
      }
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
