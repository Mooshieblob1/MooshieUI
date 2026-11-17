import { generation } from "./generation.svelte.js";
import { enhancerSessions } from "./enhancerSessions.svelte.js";
import { createNovelAiCharacter } from "./generation.svelte.js";
import { naiV5Variant, novelAiMaxCharacters } from "../utils/novelaiModels.js";
import { NAI_VARIANT_BUDGET } from "../utils/naiPrompt.js";
import { parseNaiResponse } from "../utils/naiParse.js";
import type { NaiParsedResponse } from "../utils/naiParse.js";
import type { EnhancerFlow } from "../utils/enhancerSession.js";
import { splitReply } from "../utils/enhanceChat.js";
import { resolveNaiLanguage } from "../utils/naiLanguage.js";
import {
  findOutfitOverlaps,
  isolateOutfits,
  keepTagIn,
  withOppositeOutfitUc,
  positionFromX,
  type OutfitFields,
} from "../utils/naiOutfits.js";
import type { NovelAiCharacter } from "../types/index.js";
import type { NovelAiSettings } from "./generation.svelte.js";
import type { NaiVariant } from "../utils/naiPrompt.js";
import type { NaiCanonPriority } from "../utils/naiSavedCharacters.js";
import type { NaiLanguage } from "../utils/naiLanguage.js";

/**
 * The Enhance chat: a conversation with the prompt assistant in a modal chat
 * window opened from the prompt box, for every image model (`general`), NovelAI V5 (`nai`) and
 * video (`h3`). Each flow keeps its own conversation, which is the persistent
 * Enhance session (`enhancerSessions`): what the window shows is exactly what is
 * sent back to the model as history.
 *
 * Every answer is still a prompt in the target model's own format, and the
 * model may talk back in a reply alongside it, or with a reply alone when the
 * user only asked something. Nothing is written to the prompt box until the
 * user applies an answer. The general and H3 answers replace (or append to)
 * the prompt box in one click. A V5 answer touches the base prompt, the
 * undesired content and every character box at once, so besides a one-click
 * Apply it opens the per-field review: a diff with a checkbox per field, where
 * only the ticked rows are written, so hand-written character work is never
 * discarded unseen.
 *
 * A feature store, so depending on `generation` is allowed. Nothing may depend
 * on this one in the other direction. It holds no reference to the prompt
 * assistant: `utils/enhanceChatRunner.ts` makes the calls, so this stays plain
 * state over the conversation and the review.
 */

/** Which part of the window is showing: the conversation, or a V5 answer's per-field review. */
export type EnhanceChatView = "chat" | "review";

export type EnhanceFlow = "nai" | "general" | "h3";

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
  /** The box's own undesired content now, and what applying the row writes there. */
  ucBefore: string;
  ucAfter: string;
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
  /** The chat answer this review came from, marked applied when it is. Absent for Isolate outfits. */
  answerAt?: number;
  /** Run once when the rewrite is applied: saves what it learned to memory. */
  onApplied?: () => void;
}

/** Rows whose text the outfit tools read and write: every box not being removed. */
function liveRows(p: NaiEnhancePending): number[] {
  return p.characters.flatMap((c, i) => (c.removes ? [] : [i]));
}

/** The pending rewrite's base, boxes and box UCs, in `liveRows` order. */
function outfitFields(p: NaiEnhancePending): OutfitFields {
  const rows = liveRows(p).map((i) => p.characters[i]);
  return { base: p.base.after, boxes: rows.map((r) => r.after), ucs: rows.map((r) => r.ucAfter) };
}

/**
 * Write outfit tool output back into the pending rows. A row whose text changed
 * is ticked, since the change is what the user just asked for.
 */
function withOutfitFields(p: NaiEnhancePending, f: OutfitFields): NaiEnhancePending {
  const rows = liveRows(p);
  const characters = p.characters.map((c, i) => {
    const k = rows.indexOf(i);
    if (k === -1) return c;
    const after = f.boxes[k];
    const ucAfter = f.ucs[k];
    if (after === c.after && ucAfter === c.ucAfter) return c;
    return { ...c, after, ucAfter, selected: true };
  });
  const base =
    f.base === p.base.after ? p.base : { ...p.base, after: f.base, selected: true };
  return { ...p, base, characters };
}

/** Placement words for each live row's box, from its grid position when positions are on. */
function rowPositions(p: NaiEnhancePending): (string | null)[] {
  const settings = generation.novelaiSettings;
  return liveRows(p).map((i) => {
    const target = p.characters[i].targetIndex;
    if (!settings.use_coords || target === null) return null;
    const box = settings.characters[target];
    return box ? positionFromX(box.center.x) : null;
  });
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

/** How long the undo pill under the prompt box stays up after an apply. */
const UNDO_WINDOW_MS = 10000;

/**
 * How many reference images one rewrite may carry.
 *
 * Mirrors `MAX_VISION_IMAGES` in the Rust vision layer, which enforces it for
 * real: every image is inlined into the request body, and no provider reads a
 * long image list as carefully as a short one. Four is also as many thumbnails
 * as fit on one row of the chat composer without shrinking them past recognition.
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

/** The session each Enhance flow keeps its conversation in. */
export function sessionFlowOf(flow: EnhanceFlow): EnhancerFlow {
  return flow === "general" ? "enhance" : flow;
}

/** The Enhance flow for what is being generated right now. */
export function currentEnhanceFlow(): EnhanceFlow {
  if (generation.mode === "video") return "h3";
  return generation.isNovelAi && naiV5Variant(generation.checkpoint) ? "nai" : "general";
}

/** What one request sent, kept so the answer can be asked for again. */
export interface ChatRequest {
  text: string;
  references: NaiEnhanceReference[];
}

/**
 * What only the answer that just arrived knows: the warnings, the V5 notes and
 * canon differences, and what applying it saves to memory. Held for this run
 * of the app; an answer reloaded from the saved session is its reply and
 * prompt alone, which is all Apply needs.
 */
export interface ChatAnswerMeta {
  /** An H3 format miss, or the idle-loop note. */
  notice?: { kind: "warning" | "info"; text: string } | null;
  /** V5: the rewrite's `NOTE:` line. */
  note?: string;
  /** V5: validator problems that survived the retry. */
  problems?: string[];
  /** V5: how saved canon characters differ from canon. Non-empty offers the redo choice. */
  canon?: string[];
  /** V5: the variant it was written for, and whether Curate it was on. */
  variant?: NaiVariant;
  language?: NaiLanguage;
  curate?: boolean;
  /** Run once when the answer is applied: saves what it learned to memory. */
  onApplied?: () => void;
  /** The request, for Retry and the canon redo. */
  request?: ChatRequest;
  applied?: boolean;
}

/** One exchange as the window shows it. */
export interface ChatExchange {
  at: number;
  /** What the user sent, as the session stores it. */
  user: string;
  /** The model's words to the user. */
  reply: string;
  /** The prompt in the target format. Empty for a reply-only answer. */
  prompt: string;
  meta: ChatAnswerMeta | undefined;
}

export type EnhanceLength = "short" | "medium" | "detailed";

class EnhanceChatStore {
  /** Whether the chat window is showing. Closing it keeps the conversation and any answer on its way. */
  open = $state(false);
  view = $state<EnhanceChatView>("chat");
  /** Which conversation the window shows, picked from the model when it opens. */
  flow = $state<EnhanceFlow>("general");
  /** The message being typed. */
  input = $state("");
  /** The flow whose answer is on its way, or null when nothing is running. */
  busyFlow = $state<EnhanceFlow | null>(null);
  /** The message on its way, shown in the conversation until its answer lands. */
  sending = $state<{ flow: EnhanceFlow; text: string; references: NaiEnhanceReference[] } | null>(null);
  /** The newest failure, shown at the end of its flow's conversation. */
  error = $state<{ flow: EnhanceFlow; text: string } | null>(null);
  /** Per-answer extras, by the answer's session timestamp. */
  meta = $state<Record<number, ChatAnswerMeta>>({});
  pending = $state<NaiEnhancePending | null>(null);
  showUndo = $state(false);
  /** Which flow the undo pill belongs to, for its label. */
  undoFlow = $state<EnhanceFlow>("nai");
  /** Images attached to the message being typed, in the order the model will see them. */
  references = $state<NaiEnhanceReference[]>([]);
  /**
   * V5 only: answers are written for V5 Full, and applying one also switches to
   * V5 Full with the Curated-leaning settings in `NOVELAI_CURATE`. Off each
   * time the window opens, so it only happens when ticked.
   */
  curate = $state(false);
  /**
   * V5 only: which side wins when a saved canon character's card disagrees
   * with canon. Back to saved each time the window opens; an answer that lists
   * differences offers the switch as a redo.
   */
  canonPriority = $state<NaiCanonPriority>("saved");
  /**
   * Whether saved characters' memory notes go out with the request. On each
   * time the window opens; unticked, a request works from the text and saved
   * looks alone, the clean start for a composition that keeps failing.
   */
  useMemory = $state(true);
  /** General only, not saved: how long a prompt to write, and whether to add artist tags. */
  length = $state<EnhanceLength>("medium");
  includeArtists = $state(false);

  private snapshot: UndoSnapshot | null = null;
  private undoTimer: ReturnType<typeof setTimeout> | null = null;
  /**
   * The request in flight, if any. Stop and New chat abort it, which stops it
   * on the backend as well, so the next message starts at once.
   */
  private request: AbortController | null = null;

  get busy(): boolean {
    return this.busyFlow !== null;
  }

  /** Open the chat window on a flow's conversation. */
  openChat(flow: EnhanceFlow = currentEnhanceFlow()): void {
    if (!this.open) {
      this.curate = false;
      this.canonPriority = "saved";
      this.useMemory = true;
    }
    this.flow = flow;
    this.view = "chat";
    this.open = true;
  }

  /** Close the window. The conversation stays, and an answer on its way still lands in it. */
  close(): void {
    this.open = false;
    this.view = "chat";
    this.pending = null;
  }

  backToChat(): void {
    this.view = "chat";
    this.pending = null;
  }

  /**
   * Mark a request as started and hand back the signal its calls should carry.
   * Any request still running is cancelled first: only one answer can land.
   */
  beginRequest(flow: EnhanceFlow, text: string, references: NaiEnhanceReference[]): AbortSignal {
    this.cancelRequest();
    this.request = new AbortController();
    this.busyFlow = flow;
    this.sending = { flow, text, references };
    if (this.error?.flow === flow) this.error = null;
    return this.request.signal;
  }

  /**
   * Mark the request that owns `signal` as done. A cancelled one is ignored:
   * cancelling already cleared the state, and a newer request may own it now.
   */
  endRequest(signal: AbortSignal): void {
    if (this.request?.signal !== signal) return;
    this.request = null;
    this.busyFlow = null;
    this.sending = null;
  }

  /** Stop the answer on its way. What was sent goes back into the box. */
  stop(): void {
    const sent = this.sending;
    this.cancelRequest();
    if (sent && sent.flow === this.flow && !this.input.trim() && this.references.length === 0) {
      this.input = sent.text;
      this.references = sent.references;
    }
  }

  private cancelRequest(): void {
    this.request?.abort();
    this.request = null;
    this.busyFlow = null;
    this.sending = null;
  }

  /**
   * Add a finished exchange to its flow's conversation. Returns its timestamp,
   * or null when the session refused it (an empty side).
   */
  addAnswer(flow: EnhanceFlow, user: string, assistant: string, meta: ChatAnswerMeta): number | null {
    const at = enhancerSessions.record(sessionFlowOf(flow), user, assistant);
    if (at === null) return null;
    this.meta = { ...this.meta, [at]: meta };
    return at;
  }

  /** Show a failure at the end of the conversation, and give the user their message back. */
  fail(flow: EnhanceFlow, text: string, request: ChatRequest): void {
    this.error = { flow, text };
    if (flow === this.flow && !this.input.trim() && this.references.length === 0) {
      this.input = request.text;
      this.references = request.references;
    }
  }

  dismissError(): void {
    this.error = null;
  }

  /** A flow's conversation, oldest first. */
  exchanges(flow: EnhanceFlow): ChatExchange[] {
    return enhancerSessions.turns(sessionFlowOf(flow)).map((t) => {
      const { reply, prompt } = splitReply(t.assistant);
      return { at: t.at, user: t.user, reply, prompt, meta: this.meta[t.at] };
    });
  }

  /** Take back a flow's newest exchange, so it can be asked again. */
  takeBackLast(flow: EnhanceFlow): void {
    const turns = enhancerSessions.turns(sessionFlowOf(flow));
    const last = turns[turns.length - 1];
    if (!last) return;
    enhancerSessions.dropLast(sessionFlowOf(flow));
    const { [last.at]: _, ...rest } = this.meta;
    this.meta = rest;
  }

  /** Start the flow's conversation over, stopping an answer still on its way. */
  newChat(flow: EnhanceFlow): void {
    if (this.busyFlow === flow) this.cancelRequest();
    const gone = new Set(enhancerSessions.turns(sessionFlowOf(flow)).map((t) => t.at));
    enhancerSessions.clear(sessionFlowOf(flow));
    this.meta = Object.fromEntries(Object.entries(this.meta).filter(([at]) => !gone.has(Number(at))));
    if (this.error?.flow === flow) this.error = null;
    if (this.view === "review") this.backToChat();
  }

  private markApplied(at: number | undefined): void {
    if (at === undefined) return;
    const m = this.meta[at] ?? {};
    const onApplied = m.onApplied;
    this.meta = { ...this.meta, [at]: { ...m, applied: true, onApplied: undefined } };
    // Memory is saved once, by the first apply.
    onApplied?.();
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
    this.references = this.references.map((r) => (r.id === id ? { ...r, label } : r));
  }

  /** Paste the prompt box into the message, for the "tidy up what I have" case. */
  copyExistingPrompt(): void {
    this.input = generation.positivePrompt ?? "";
  }

  // ---- Applying an answer ------------------------------------------------

  /**
   * Write a general or H3 answer into the prompt box. `append` keeps the
   * current prompt and adds the answer after it, the way Compose did.
   */
  applyText(at: number, prompt: string, how: "replace" | "append"): void {
    if (!prompt.trim()) return;
    this.takeSnapshot();
    const current = generation.positivePrompt?.trim();
    generation.positivePrompt = how === "append" && current ? `${current}, ${prompt}` : prompt;
    generation.saveSettings();
    this.markApplied(at);
    this.finishApply(this.flow === "h3" ? "h3" : "general");
  }

  /** A V5 answer's fields, read back from the conversation. */
  private naiFields(at: number): NaiParsedResponse | null {
    const ex = this.exchanges("nai").find((e) => e.at === at);
    if (!ex || !ex.prompt.trim()) return null;
    const parsed = parseNaiResponse(ex.prompt);
    return parsed.base.trim() ? parsed : null;
  }

  /**
   * The per-field review for a V5 answer, built against the boxes as they are
   * now. Every row starts ticked: the answer is one coherent prompt, and half
   * applying it leaves a base prompt describing characters that the boxes no
   * longer match.
   */
  private naiPending(at: number): NaiEnhancePending | null {
    const parsed = this.naiFields(at);
    const variant = naiV5Variant(generation.checkpoint);
    if (!parsed || !variant) return null;
    const m = this.meta[at];
    const boxes = generation.novelaiSettings.characters;
    return {
      variant,
      language: m?.language ?? resolveNaiLanguage(generation.naiEnhanceLanguage, parsed.base),
      budget: NAI_VARIANT_BUDGET[m?.curate ? "full" : variant],
      note: m?.note ?? "",
      problems: m?.problems ?? [],
      canon: m?.canon ?? [],
      answerAt: at,
      onApplied: undefined,
      base: { before: generation.positivePrompt, after: parsed.base, selected: true },
      uc: { before: generation.negativePrompt, after: parsed.uc, selected: true },
      characters: [
        ...parsed.characters.map((after, i) => ({
          before: boxes[i]?.prompt ?? "",
          after,
          selected: true,
          targetIndex: i < boxes.length ? i : null,
          removes: false,
          ucBefore: boxes[i]?.negative_prompt ?? "",
          ucAfter: parsed.characterUcs[i] ?? boxes[i]?.negative_prompt ?? "",
        })),
        // Boxes the answer came back short of. The base prompt it wrote
        // describes the cast it returned, so a box left standing puts a
        // character in the image that nothing asked for. Offered as a ticked
        // removal row rather than cleared behind the user's back, and only
        // for boxes with something in them.
        ...boxes
          .map((box, i) => ({ box, i }))
          .slice(parsed.characters.length)
          .filter(({ box }) => (box.prompt ?? "").trim() !== "")
          .map(({ box, i }) => ({
            before: box.prompt ?? "",
            after: "",
            selected: true,
            targetIndex: i,
            removes: true,
            ucBefore: box.negative_prompt ?? "",
            ucAfter: "",
          })),
      ],
    };
  }

  /** Open a V5 answer's per-field review. False when it has no fields or the model is not V5. */
  reviewAnswer(at: number): boolean {
    const p = this.naiPending(at);
    if (!p) return false;
    this.curate = !!this.meta[at]?.curate;
    this.pending = p;
    this.view = "review";
    return true;
  }

  /** Apply every field of a V5 answer, as the review would with nothing unticked. */
  applyAnswer(at: number): boolean {
    const p = this.naiPending(at);
    if (!p) return false;
    this.curate = !!this.meta[at]?.curate;
    this.pending = p;
    this.apply();
    return true;
  }

  // ---- The V5 review ------------------------------------------------------

  /** True once at least one row is ticked, which is what enables Apply. */
  get hasSelection(): boolean {
    const p = this.pending;
    if (!p) return false;
    return p.base.selected || p.uc.selected || p.characters.some((c) => c.selected);
  }

  /**
   * Every field that would be sent if the user applied right now.
   *
   * Concatenated the way NovelAI counts it, so the review's bar reflects the
   * selection rather than the whole answer. Unticked rows keep their current
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
   * Isolate outfits on the answer under review: restated outfit and hair tags
   * leave the base prompt, unplaced boxes get their grid side, and every box
   * gets the opposite-outfit UC. False when there was nothing to change.
   */
  isolatePending(): boolean {
    const p = this.pending;
    if (!p) return false;
    const before = outfitFields(p);
    const after = isolateOutfits(before, rowPositions(p));
    if (JSON.stringify(after) === JSON.stringify(before)) return false;
    this.pending = withOutfitFields(p, after);
    return true;
  }

  /** Add the opposite-outfit UC alone, for the similar-bodies tip. */
  fillOppositeUc(): void {
    const p = this.pending;
    if (!p) return;
    const f = outfitFields(p);
    this.pending = withOutfitFields(p, { ...f, ucs: withOppositeOutfitUc(f.boxes, f.ucs) });
  }

  /**
   * Settle one overlap chip. `row` is the character row the tag belongs to, or
   * null for a chip on the base prompt (the tag stays in whichever boxes have it).
   */
  resolveOverlap(tag: string, row: number | null): void {
    const p = this.pending;
    if (!p) return;
    const keep = row === null ? null : liveRows(p).indexOf(row);
    this.pending = withOutfitFields(p, keepTagIn(outfitFields(p), tag, keep === -1 ? null : keep));
  }

  /**
   * Isolate outfits on the current prompt, with no request: the result opens
   * in the review as if an answer had proposed it. False when there is nothing
   * to fix, or the model is not V5.
   */
  openIsolateReview(): boolean {
    const variant = naiV5Variant(generation.checkpoint);
    const boxes = generation.novelaiSettings.characters;
    if (!variant || boxes.filter((b) => b.prompt.trim()).length < 2) return false;
    const base = generation.positivePrompt ?? "";
    const uc = generation.negativePrompt ?? "";
    const pending: NaiEnhancePending = {
      variant,
      language: resolveNaiLanguage(generation.naiEnhanceLanguage, base),
      budget: NAI_VARIANT_BUDGET[variant],
      note: "",
      problems: [],
      canon: [],
      base: { before: base, after: base, selected: false },
      uc: { before: uc, after: uc, selected: false },
      characters: boxes.map((b, i) => ({
        before: b.prompt,
        after: b.prompt,
        selected: false,
        targetIndex: i,
        removes: false,
        ucBefore: b.negative_prompt,
        ucAfter: b.negative_prompt,
      })),
    };
    const fields = outfitFields(pending);
    const fixed = isolateOutfits(fields, rowPositions(pending));
    // Overlaps between boxes are not auto-fixed (matching clothes can be on
    // purpose), so they still open the review, where their chips settle them.
    if (
      JSON.stringify(fixed) === JSON.stringify(fields) &&
      findOutfitOverlaps(fixed.base, fixed.boxes).length === 0
    ) {
      return false;
    }
    this.openChat("nai");
    this.curate = false;
    this.pending = withOutfitFields(pending, fixed);
    this.view = "review";
    return true;
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
   * Write the ticked rows and close the window.
   *
   * Character slots are built in one pass rather than through
   * `addNovelAiCharacter`, so that an answer proposing three new characters
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
        chars[row.targetIndex] = {
          ...chars[row.targetIndex],
          prompt: row.after,
          negative_prompt: row.ucAfter,
        };
      } else if (chars.length < novelAiMaxCharacters(generation.checkpoint)) {
        chars.push({ ...createNovelAiCharacter(), prompt: row.after, negative_prompt: row.ucAfter });
      }
    }
    generation.updateNovelAiSettings({
      characters: chars.filter((_, i) => !dropped.has(i)),
    });
    // After the rows, so the avoids land on the undesired content the answer
    // wrote rather than being overwritten by it.
    if (this.curate) generation.applyNovelAiCurate();
    generation.saveSettings();

    this.markApplied(p.answerAt);
    this.finishApply("nai");
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

  /**
   * Close the window once an answer is in the prompt box, the way the Enhance
   * modal always has: the next step is generating. Enhance reopens it on the
   * same conversation.
   */
  private finishApply(flow: EnhanceFlow): void {
    this.close();
    this.undoFlow = flow;
    this.showUndo = true;
    if (this.undoTimer) clearTimeout(this.undoTimer);
    this.undoTimer = setTimeout(() => (this.showUndo = false), UNDO_WINDOW_MS);
  }

  /**
   * Restore everything the last apply touched, including untouched slots. The
   * conversation keeps the answer: it can be applied again from there.
   */
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

export const enhanceChat = new EnhanceChatStore();
