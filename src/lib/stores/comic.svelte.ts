// Comic book builder state: multi-page books persisted per user (lightweight:
// gallery filenames only, never raw PNG bytes) and per-page runtime caches
// (object URLs, generation statuses, overflow results, undo history) keyed by
// pageId. Generation, checks, assembly and export live in utils/comicRunner.ts.
//
// A leaf store: it imports no other store.

import { userScopedKey } from "../utils/ipc.js";
import {
  LAYOUT_TEMPLATES,
  MAX_PANELS,
  applyTemplateToPanels,
  buildTemplate,
  clampFreeformBox,
  freeformRects,
  isFreeformBoxes,
  isLayout,
  isPanelNonEmpty,
  layoutForCount,
  newId,
  newPanel,
  pageSize,
  panelIds,
  panelRects,
  readingOrder,
  removePanel,
  reorderZ,
  setRatio,
  splitPanel,
  treeToFreeform,
  type CastMember,
  type ComicPanel,
  type ContinuityIssue,
  type FreeformBox,
  type LayoutMode,
  type LayoutNode,
  type LayoutTemplate,
  type PageShape,
  type Rect,
  type RefMode,
  type SplitDir,
} from "../utils/comicLayout.js";
import {
  bubblesFromDialogue,
  clampBubble,
  newBubble,
  sanitizeBubbles,
  type SpeechBubble,
} from "../utils/comicBubbles.js";

const BOOKS_STORAGE_KEY = "mooshieui.comic.books.v1";
const LEGACY_PAGE_STORAGE_KEY = "mooshieui.comic.page.v1";
const HISTORY_LIMIT = 40;

export type ComicMode = "panels" | "one_image";

export interface PanelResult {
  status: "queued" | "running" | "done" | "failed";
  /** Display URL (an object URL the store owns). */
  url?: string;
  /** Gallery file, so the image survives a restart. */
  galleryFilename?: string | null;
  error?: string;
  /** null until the tagger ran; [] when it found nothing wrong. */
  issues?: ContinuityIssue[] | null;
  checking?: boolean;
  checkError?: string;
}

export interface ComicPageState {
  id: string;
  title: string;
  layoutMode: LayoutMode;
  layout: LayoutNode;
  freeformBoxes: FreeformBox[];
  panels: Record<string, ComicPanel>;
  /** Non-empty panels stashed when shrinking or removing panels; restored on larger layouts. */
  overflowPanels: ComicPanel[];
  /** Optional setting override for this page; blank inherits the book setting. */
  settingOverride: string;
  /** Gallery filename per panel. */
  images: Record<string, string>;
  /** Assembled or one-image page gallery filename. */
  pageImage: string | null;
  /** Client-side vector speech/thought/shout/caption bubbles overlaid on the page. */
  bubbles: SpeechBubble[];
}

export interface ComicBook {
  id: string;
  title: string;
  cast: CastMember[];
  /** Style tags for every panel: "manga, monochrome, screentone". */
  style: string;
  /** Book setting: location, time of day, lighting. Every panel inherits it unless overridden. */
  setting: string;
  pageShape: PageShape;
  /** Space between panels on the assembled page, in output pixels. */
  gutter: number;
  rightToLeft: boolean;
  mode: ComicMode;
  /** Run the tagger on each finished panel and flag what changed. */
  checkContinuity: boolean;
  /** Visual character-reference continuity mode across panels/pages. */
  refMode: RefMode;
  pages: ComicPageState[];
  activePageIndex: number;
}

interface SavedPageLegacy {
  layout: LayoutNode;
  layoutMode?: LayoutMode;
  freeformBoxes?: FreeformBox[];
  panels: Record<string, ComicPanel>;
  overflowPanels?: ComicPanel[];
  cast: CastMember[];
  setting: string;
  style: string;
  pageShape: PageShape;
  gutter: number;
  rightToLeft: boolean;
  mode: ComicMode;
  checkContinuity: boolean;
  usePanel1Ref?: boolean;
  images?: Record<string, string>;
  pageImage?: string | null;
}

export interface PageRuntime {
  results: Record<string, PanelResult>;
  /** Session-only results aligned by index with `overflowPanels`. */
  overflowResults: Array<PanelResult | undefined>;
  pageUrl: string | null;
  pageGalleryFilename: string | null;
  pageStatus: "idle" | "running" | "done" | "failed";
  pageError: string | null;
  storyIssues: Array<{ panel: number; issue: string }> | null;
  storyError: string | null;
  draftUndo: Record<string, { prompt: string; dialogue: string }> | null;
  history: string[];
  selectedId: string | null;
}

function emptyRuntime(): PageRuntime {
  return {
    results: {},
    overflowResults: [],
    pageUrl: null,
    pageGalleryFilename: null,
    pageStatus: "idle",
    pageError: null,
    storyIssues: null,
    storyError: null,
    draftUndo: null,
    history: [],
    selectedId: null,
  };
}

const EMPTY_RUNTIME_FALLBACK: PageRuntime = emptyRuntime();

function defaultLayout(): LayoutNode {
  return buildTemplate(LAYOUT_TEMPLATES.find((t) => t.id === "one_over_two")!);
}

function normalizePanel(id: string, raw: unknown): ComicPanel {
  const base = newPanel(id);
  if (!raw || typeof raw !== "object") return base;
  const obj = raw as Partial<ComicPanel>;
  return {
    ...base,
    ...obj,
    id,
    cast: Array.isArray(obj.cast) ? obj.cast.filter((c): c is string => typeof c === "string") : [],
  };
}

function normalizeOverflowPanels(raw: unknown): ComicPanel[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .filter((p) => p && typeof p === "object")
    .map((p, i) => {
      const id = typeof (p as { id?: unknown }).id === "string" ? (p as { id: string }).id : `overflow_${i}`;
      return normalizePanel(id, p);
    });
}

function ensurePanelsForIds(ids: string[], existing: Record<string, ComicPanel>): Record<string, ComicPanel> {
  const next: Record<string, ComicPanel> = {};
  for (const id of ids) {
    next[id] = existing[id] ? normalizePanel(id, existing[id]) : newPanel(id);
  }
  return next;
}

function createPage(partial?: Partial<ComicPageState>): ComicPageState {
  const layout = partial?.layout && isLayout(partial.layout) ? partial.layout : defaultLayout();
  const freeformBoxes = isFreeformBoxes(partial?.freeformBoxes) ? partial.freeformBoxes : [];
  const layoutMode: LayoutMode =
    partial?.layoutMode === "freeform" && freeformBoxes.length > 0 ? "freeform" : "grid";
  const ids = layoutMode === "freeform" ? freeformBoxes.map((b) => b.id) : panelIds(layout);
  const panels = ensurePanelsForIds(ids, partial?.panels ?? {});
  const overflowPanels = normalizeOverflowPanels(partial?.overflowPanels);
  const images: Record<string, string> = {};
  if (partial?.images && typeof partial.images === "object") {
    for (const [k, v] of Object.entries(partial.images)) {
      if (panels[k] && typeof v === "string" && v && !v.startsWith("data:")) images[k] = v;
    }
  }
  const pageImage =
    typeof partial?.pageImage === "string" && partial.pageImage && !partial.pageImage.startsWith("data:")
      ? partial.pageImage
      : null;
  const bubbles = sanitizeBubbles(partial?.bubbles);
  return {
    id: typeof partial?.id === "string" && partial.id ? partial.id : newId("pg"),
    title: typeof partial?.title === "string" ? partial.title : "",
    layoutMode,
    layout,
    freeformBoxes,
    panels,
    overflowPanels,
    settingOverride: typeof partial?.settingOverride === "string" ? partial.settingOverride : "",
    images,
    pageImage,
    bubbles,
  };
}

function sanitizeCast(raw: unknown): CastMember[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .filter((c): c is Record<string, unknown> => !!c && typeof c === "object" && typeof (c as Record<string, unknown>).id === "string")
    .map((c) => ({
      id: c.id as string,
      name: String(c.name ?? ""),
      tags: String(c.tags ?? ""),
      savedCharacterId: typeof c.savedCharacterId === "string" && c.savedCharacterId ? c.savedCharacterId : null,
      referenceImage: typeof c.referenceImage === "string" && c.referenceImage ? c.referenceImage : null,
    }));
}

function sanitizeRefMode(raw: unknown, legacyUsePanel1Ref?: unknown): RefMode {
  if (raw === "off" || raw === "page1_panel1" || raw === "prev_panel" || raw === "cast") return raw;
  if (legacyUsePanel1Ref === true) return "page1_panel1";
  return "off";
}

function createBook(partial?: Partial<ComicBook>, index = 1): ComicBook {
  const pagesRaw = Array.isArray(partial?.pages) && partial.pages.length > 0 ? partial.pages : [undefined];
  const pages = pagesRaw.map((p) => createPage(p));
  const rawIdx = typeof partial?.activePageIndex === "number" ? Math.floor(partial.activePageIndex) : 0;
  const activePageIndex = Math.max(0, Math.min(pages.length - 1, rawIdx));
  return {
    id: typeof partial?.id === "string" && partial.id ? partial.id : newId("bk"),
    title: typeof partial?.title === "string" && partial.title.trim() ? partial.title : `Book ${index}`,
    cast: sanitizeCast(partial?.cast),
    style: typeof partial?.style === "string" ? partial.style : "",
    setting: typeof partial?.setting === "string" ? partial.setting : "",
    pageShape:
      partial?.pageShape === "portrait" || partial?.pageShape === "square" || partial?.pageShape === "landscape"
        ? partial.pageShape
        : "portrait",
    gutter: typeof partial?.gutter === "number" ? Math.min(80, Math.max(0, partial.gutter)) : 24,
    rightToLeft: typeof partial?.rightToLeft === "boolean" ? partial.rightToLeft : false,
    mode: partial?.mode === "panels" || partial?.mode === "one_image" ? partial.mode : "panels",
    checkContinuity: typeof partial?.checkContinuity === "boolean" ? partial.checkContinuity : true,
    refMode: sanitizeRefMode(partial?.refMode),
    pages,
    activePageIndex,
  };
}

function revokeRuntime(rt: PageRuntime | undefined) {
  if (!rt) return;
  for (const r of Object.values(rt.results)) {
    if (r.url) URL.revokeObjectURL(r.url);
  }
  for (const r of rt.overflowResults) {
    if (r?.url) URL.revokeObjectURL(r.url);
  }
  if (rt.pageUrl) URL.revokeObjectURL(rt.pageUrl);
}

function syncImagesFromResults(page: ComicPageState, results: Record<string, PanelResult>) {
  const images: Record<string, string> = {};
  for (const [id, r] of Object.entries(results)) {
    if (r.galleryFilename && !r.galleryFilename.startsWith("data:")) {
      images[id] = r.galleryFilename;
    }
  }
  page.images = images;
}

class ComicStore {
  books = $state<ComicBook[]>([createBook(undefined, 1)]);
  activeBookId = $state<string>(this.books[0].id);
  private runtimeByPage = $state<Record<string, PageRuntime>>({
    [this.books[0].pages[0].id]: emptyRuntime(),
  });

  // --- global run state (not persisted) ---
  running = $state(false);
  stopRequested = $state(false);
  storyChecking = $state(false);
  drafting = $state(false);
  /** The panel the Prompt Assistant is rewriting on its own, if any. */
  enhancingId = $state<string | null>(null);
  /** Currently selected vector speech bubble on the active page (clears on page switch). */
  selectedBubbleId = $state<string | null>(null);

  private saveTimer: ReturnType<typeof setTimeout> | null = null;
  private loaded = false;

  // --- book / page accessors ---

  get activeBook(): ComicBook {
    return this.books.find((b) => b.id === this.activeBookId) ?? this.books[0];
  }

  get pages(): ComicPageState[] {
    return this.activeBook.pages;
  }

  get activePageIndex(): number {
    const idx = this.activeBook.activePageIndex;
    return Math.max(0, Math.min(this.activeBook.pages.length - 1, idx));
  }

  get activePage(): ComicPageState {
    return this.activeBook.pages[this.activePageIndex] ?? this.activeBook.pages[0];
  }

  private rt(pageId = this.activePage.id): PageRuntime {
    return this.runtimeByPage[pageId] ?? EMPTY_RUNTIME_FALLBACK;
  }

  private ensureRt(pageId: string): PageRuntime {
    if (!this.runtimeByPage[pageId]) {
      this.runtimeByPage = { ...this.runtimeByPage, [pageId]: emptyRuntime() };
    }
    return this.runtimeByPage[pageId];
  }

  private ensureAllRuntimes() {
    const next: Record<string, PageRuntime> = { ...this.runtimeByPage };
    let changed = false;
    for (const book of this.books) {
      for (const page of book.pages) {
        if (!next[page.id]) {
          next[page.id] = emptyRuntime();
          changed = true;
        }
      }
    }
    if (changed) this.runtimeByPage = next;
  }

  // --- proxied active book & page fields ---

  get layoutMode(): LayoutMode {
    return this.activePage.layoutMode;
  }
  set layoutMode(v: LayoutMode) {
    this.activePage.layoutMode = v;
  }

  get layout(): LayoutNode {
    return this.activePage.layout;
  }
  set layout(v: LayoutNode) {
    this.activePage.layout = v;
  }

  get freeformBoxes(): FreeformBox[] {
    return this.activePage.freeformBoxes;
  }
  set freeformBoxes(v: FreeformBox[]) {
    this.activePage.freeformBoxes = v;
  }

  get panels(): Record<string, ComicPanel> {
    return this.activePage.panels;
  }
  set panels(v: Record<string, ComicPanel>) {
    this.activePage.panels = v;
  }

  get overflowPanels(): ComicPanel[] {
    return this.activePage.overflowPanels;
  }
  set overflowPanels(v: ComicPanel[]) {
    this.activePage.overflowPanels = v;
  }

  get bubbles(): SpeechBubble[] {
    return this.activePage.bubbles;
  }
  set bubbles(v: SpeechBubble[]) {
    this.activePage.bubbles = v;
  }

  get overflowResults(): Array<PanelResult | undefined> {
    return this.rt().overflowResults;
  }
  set overflowResults(v: Array<PanelResult | undefined>) {
    this.ensureRt(this.activePage.id).overflowResults = v;
  }

  get cast(): CastMember[] {
    return this.activeBook.cast;
  }
  set cast(v: CastMember[]) {
    this.activeBook.cast = v;
  }

  get setting(): string {
    return this.activeBook.setting;
  }
  set setting(v: string) {
    this.activeBook.setting = v;
  }

  get settingOverride(): string {
    return this.activePage.settingOverride;
  }
  set settingOverride(v: string) {
    this.activePage.settingOverride = v;
  }

  /** Effective setting for a page (page override if set, else book setting). */
  effectiveSettingFor(page: ComicPageState = this.activePage, book: ComicBook = this.activeBook): string {
    return page.settingOverride.trim() || book.setting;
  }

  get effectiveSetting(): string {
    return this.effectiveSettingFor();
  }

  get style(): string {
    return this.activeBook.style;
  }
  set style(v: string) {
    this.activeBook.style = v;
  }

  get pageShape(): PageShape {
    return this.activeBook.pageShape;
  }
  set pageShape(v: PageShape) {
    this.activeBook.pageShape = v;
  }

  get gutter(): number {
    return this.activeBook.gutter;
  }
  set gutter(v: number) {
    this.activeBook.gutter = v;
  }

  get rightToLeft(): boolean {
    return this.activeBook.rightToLeft;
  }
  set rightToLeft(v: boolean) {
    this.activeBook.rightToLeft = v;
  }

  get mode(): ComicMode {
    return this.activeBook.mode;
  }
  set mode(v: ComicMode) {
    this.activeBook.mode = v;
  }

  get checkContinuity(): boolean {
    return this.activeBook.checkContinuity;
  }
  set checkContinuity(v: boolean) {
    this.activeBook.checkContinuity = v;
  }

  get refMode(): RefMode {
    return this.activeBook.refMode;
  }
  set refMode(v: RefMode) {
    this.activeBook.refMode = v;
  }

  get selectedId(): string | null {
    return this.rt().selectedId;
  }
  set selectedId(v: string | null) {
    this.ensureRt(this.activePage.id).selectedId = v;
  }

  get results(): Record<string, PanelResult> {
    return this.rt().results;
  }
  set results(v: Record<string, PanelResult>) {
    this.ensureRt(this.activePage.id).results = v;
  }

  get pageUrl(): string | null {
    return this.rt().pageUrl;
  }
  set pageUrl(v: string | null) {
    this.ensureRt(this.activePage.id).pageUrl = v;
  }

  get pageGalleryFilename(): string | null {
    return this.rt().pageGalleryFilename ?? this.activePage.pageImage;
  }
  set pageGalleryFilename(v: string | null) {
    this.ensureRt(this.activePage.id).pageGalleryFilename = v;
    this.activePage.pageImage = v;
  }

  get pageStatus(): "idle" | "running" | "done" | "failed" {
    return this.rt().pageStatus;
  }
  set pageStatus(v: "idle" | "running" | "done" | "failed") {
    this.ensureRt(this.activePage.id).pageStatus = v;
  }

  get pageError(): string | null {
    return this.rt().pageError;
  }
  set pageError(v: string | null) {
    this.ensureRt(this.activePage.id).pageError = v;
  }

  get storyIssues(): Array<{ panel: number; issue: string }> | null {
    return this.rt().storyIssues;
  }
  set storyIssues(v: Array<{ panel: number; issue: string }> | null) {
    this.ensureRt(this.activePage.id).storyIssues = v;
  }

  get storyError(): string | null {
    return this.rt().storyError;
  }
  set storyError(v: string | null) {
    this.ensureRt(this.activePage.id).storyError = v;
  }

  get draftUndo(): Record<string, { prompt: string; dialogue: string }> | null {
    return this.rt().draftUndo;
  }
  set draftUndo(v: Record<string, { prompt: string; dialogue: string }> | null) {
    this.ensureRt(this.activePage.id).draftUndo = v;
  }

  resultsForPage(pageId: string): Record<string, PanelResult> {
    return this.rt(pageId).results;
  }

  pageUrlForPage(pageId: string): string | null {
    return this.rt(pageId).pageUrl;
  }

  rectsForPage(page: ComicPageState): Map<string, Rect> {
    return page.layoutMode === "freeform" ? freeformRects(page.freeformBoxes) : panelRects(page.layout);
  }

  orderedPanelsForPage(page: ComicPageState, rightToLeft = this.activeBook.rightToLeft): ComicPanel[] {
    const rects = this.rectsForPage(page);
    return readingOrder(rects, rightToLeft)
      .map((id) => page.panels[id])
      .filter((p): p is ComicPanel => !!p);
  }

  pageHasContent(page: ComicPageState, book: ComicBook = this.activeBook): boolean {
    const rt = this.rt(page.id);
    if (book.mode === "one_image") {
      return !!(rt.pageUrl || page.pageImage);
    }
    return this.orderedPanelsForPage(page, book.rightToLeft).some(
      (p) => p.enabled && !!(rt.results[p.id]?.url || page.images[p.id]),
    );
  }

  // --- derived ---

  get rects(): Map<string, Rect> {
    return this.layoutMode === "freeform" ? freeformRects(this.freeformBoxes) : panelRects(this.layout);
  }

  /** Panel ids in reading order. */
  get order(): string[] {
    return readingOrder(this.rects, this.rightToLeft);
  }

  /** Panels in reading order. */
  get orderedPanels(): ComicPanel[] {
    return this.order.map((id) => this.panels[id]).filter((p): p is ComicPanel => !!p);
  }

  get enabledPanels(): ComicPanel[] {
    return this.orderedPanels.filter((p) => p.enabled);
  }

  numberOf(id: string): number {
    return this.order.indexOf(id) + 1;
  }

  get selected(): ComicPanel | null {
    return this.selectedId ? (this.panels[this.selectedId] ?? null) : null;
  }

  get selectedBubble(): SpeechBubble | null {
    return this.selectedBubbleId ? (this.bubbles.find((b) => b.id === this.selectedBubbleId) ?? null) : null;
  }

  get canUndo(): boolean {
    return this.rt().history.length > 0;
  }

  get panelCount(): number {
    return this.layoutMode === "freeform" ? this.freeformBoxes.length : panelIds(this.layout).length;
  }

  // --- layout edits ---

  private remember() {
    const rt = this.ensureRt(this.activePage.id);
    rt.history = [
      ...rt.history.slice(-(HISTORY_LIMIT - 1)),
      JSON.stringify({
        layout: this.layout,
        panels: this.panels,
        overflowPanels: this.overflowPanels,
        layoutMode: this.layoutMode,
        freeformBoxes: this.freeformBoxes,
        bubbles: this.bubbles,
      }),
    ];
  }

  /** Make sure every leaf/freeform box has a panel record and no stale record lingers. */
  private ensurePanels() {
    const ids = this.layoutMode === "freeform" ? this.freeformBoxes.map((b) => b.id) : panelIds(this.layout);
    const next = ensurePanelsForIds(ids, this.panels);
    this.panels = next;
    const rt = this.ensureRt(this.activePage.id);
    if (rt.selectedId && !next[rt.selectedId]) rt.selectedId = null;
  }

  applyTemplate(template: LayoutTemplate) {
    this.remember();
    // Keep the prompts already written, in reading order, for the new panels,
    // and stash any overflow panels so switching back to a larger layout restores them.
    const ordered = this.orderedPanels;
    const orderedResults = ordered.map((p) => this.results[p.id]);
    this.layout = buildTemplate(template);
    this.layoutMode = "grid";
    this.freeformBoxes = [];
    const rects = panelRects(this.layout);
    const ids = readingOrder(rects, this.rightToLeft);
    const applied = applyTemplateToPanels(
      ordered,
      orderedResults,
      this.overflowPanels,
      this.overflowResults,
      ids,
    );
    this.panels = applied.panels;
    this.results = applied.results;
    this.overflowPanels = applied.overflowPanels;
    this.overflowResults = applied.overflowResults;
    syncImagesFromResults(this.activePage, this.results);
    this.selectedId = ids[0] ?? null;
    this.clearPage();
    this.persist();
  }

  setLayoutMode(mode: LayoutMode) {
    if (mode === this.layoutMode) return;
    this.remember();
    if (mode === "freeform") {
      this.freeformBoxes = treeToFreeform(this.layout, pageSize(this.pageShape), this.gutter, this.rightToLeft);
      this.layoutMode = "freeform";
      this.ensurePanels();
      this.persist();
      return;
    }
    const previous = this.orderedPanels;
    const prevResults = this.results;
    const prevSelectedIdx = this.selectedId ? previous.findIndex((p) => p.id === this.selectedId) : -1;
    this.layout = layoutForCount(previous.length || 1);
    this.layoutMode = "grid";
    this.freeformBoxes = [];
    const ids = readingOrder(panelRects(this.layout), this.rightToLeft);
    const nextPanels: Record<string, ComicPanel> = {};
    const nextResults: Record<string, PanelResult> = {};
    ids.forEach((id, i) => {
      const old = previous[i];
      nextPanels[id] = old ? { ...old, id } : newPanel(id);
      if (old && prevResults[old.id]) nextResults[id] = prevResults[old.id];
    });
    this.panels = nextPanels;
    this.results = nextResults;
    syncImagesFromResults(this.activePage, nextResults);
    this.selectedId = prevSelectedIdx >= 0 ? (ids[prevSelectedIdx] ?? ids[0] ?? null) : null;
    this.persist();
  }

  addFreeformPanel() {
    if (this.layoutMode !== "freeform" || this.panelCount >= MAX_PANELS) return;
    this.remember();
    const id = newId("p");
    const offset = (this.freeformBoxes.length % 5) * 0.04;
    const maxZ = this.freeformBoxes.reduce((m, b) => Math.max(m, b.z), -1);
    const box = clampFreeformBox({
      id,
      x: 0.1 + offset,
      y: 0.1 + offset,
      w: 0.35,
      h: 0.35,
      z: maxZ + 1,
    });
    this.freeformBoxes = [...this.freeformBoxes, box];
    if (this.overflowPanels.length > 0) {
      const [restored, ...restPanels] = this.overflowPanels;
      const [restoredResult, ...restResults] = this.overflowResults;
      this.overflowPanels = restPanels;
      this.overflowResults = restResults;
      this.panels = { ...this.panels, [id]: { ...restored, id } };
      if (restoredResult) {
        this.results = { ...this.results, [id]: restoredResult };
        if (restoredResult.galleryFilename) {
          this.activePage.images = { ...this.activePage.images, [id]: restoredResult.galleryFilename };
        }
      }
    }
    this.ensurePanels();
    this.selectedId = id;
    this.persist();
  }

  beginFreeformEdit() {
    this.remember();
  }

  updateFreeformBox(id: string, rect: Rect) {
    if (this.layoutMode !== "freeform") return;
    this.freeformBoxes = this.freeformBoxes.map((b) => (b.id === id ? clampFreeformBox({ ...b, ...rect }) : b));
    this.persistSoon();
  }

  reorderPanelZ(id: string, action: "forward" | "backward" | "front" | "back") {
    if (this.layoutMode !== "freeform") return;
    if (!this.freeformBoxes.some((b) => b.id === id)) return;
    this.remember();
    this.freeformBoxes = reorderZ(this.freeformBoxes, id, action);
    this.persist();
  }

  split(dir: SplitDir) {
    const id = this.selectedId;
    if (!id || this.panelCount >= MAX_PANELS) return;
    this.remember();
    const newPanelId = newId("p");
    this.layout = splitPanel(this.layout, id, dir, newPanelId);
    this.ensurePanels();
    this.selectedId = newPanelId;
    this.persist();
  }

  removeSelected() {
    const id = this.selectedId;
    if (!id || this.panelCount <= 1) return;
    this.remember();
    const removed = this.panels[id];
    const removedResult = this.results[id];
    if (this.layoutMode === "freeform") {
      this.freeformBoxes = this.freeformBoxes
        .filter((b) => b.id !== id)
        .sort((a, b) => a.z - b.z)
        .map((b, i) => ({ ...b, z: i }));
    } else {
      this.layout = removePanel(this.layout, id);
    }
    this.ensurePanels();
    const { [id]: _gone, ...rest } = this.results;
    this.results = rest;
    const { [id]: _goneImg, ...restImgs } = this.activePage.images;
    this.activePage.images = restImgs;
    if (removed && isPanelNonEmpty(removed)) {
      this.overflowPanels = [removed, ...this.overflowPanels];
      this.overflowResults = [removedResult, ...this.overflowResults];
    } else if (removedResult?.url) {
      URL.revokeObjectURL(removedResult.url);
    }
    this.selectedId = null;
    this.persist();
  }

  /** Call once when a divider drag starts, so the whole drag is one undo step. */
  beginResize() {
    this.remember();
  }

  resize(splitId: string, ratio: number) {
    this.layout = setRatio(this.layout, splitId, ratio);
    this.persistSoon();
  }

  undo() {
    const rt = this.ensureRt(this.activePage.id);
    const last = rt.history[rt.history.length - 1];
    if (!last) return;
    rt.history = rt.history.slice(0, -1);
    try {
      const state = JSON.parse(last) as {
        layout: LayoutNode;
        panels: Record<string, ComicPanel>;
        overflowPanels?: unknown[];
        layoutMode?: LayoutMode;
        freeformBoxes?: FreeformBox[];
        bubbles?: unknown[];
      };
      if (!isLayout(state.layout)) return;
      this.layout = state.layout;
      this.panels = state.panels;
      this.overflowPanels = normalizeOverflowPanels(state.overflowPanels);
      this.overflowResults = this.overflowPanels.map((_, i) => this.overflowResults[i]);
      if (Array.isArray(state.bubbles)) {
        this.bubbles = sanitizeBubbles(state.bubbles);
        if (this.selectedBubbleId && !this.bubbles.some((b) => b.id === this.selectedBubbleId)) {
          this.selectedBubbleId = null;
        }
      }
      if (state.layoutMode === "freeform" && isFreeformBoxes(state.freeformBoxes) && state.freeformBoxes.length > 0) {
        this.layoutMode = "freeform";
        this.freeformBoxes = state.freeformBoxes;
      } else {
        this.layoutMode = "grid";
        this.freeformBoxes = isFreeformBoxes(state.freeformBoxes) ? state.freeformBoxes : [];
      }
      this.ensurePanels();
      this.persist();
    } catch {
      /* a broken snapshot is skipped */
    }
  }

  // --- page management ---

  addPage() {
    if (this.running) return;
    const page = createPage();
    this.ensureRt(page.id);
    this.activeBook.pages = [...this.activeBook.pages, page];
    this.activeBook.activePageIndex = this.activeBook.pages.length - 1;
    this.selectedBubbleId = null;
    this.persist();
  }

  duplicatePage(index: number) {
    if (this.running) return;
    const src = this.activeBook.pages[index];
    if (!src) return;
    const clonedPanels: Record<string, ComicPanel> = {};
    for (const [id, p] of Object.entries(src.panels)) {
      clonedPanels[id] = { ...p, cast: [...p.cast] };
    }
    const copy = createPage({
      title: src.title ? `${src.title} (copy)` : "",
      layoutMode: src.layoutMode,
      layout: JSON.parse(JSON.stringify(src.layout)) as LayoutNode,
      freeformBoxes: src.freeformBoxes.map((b) => ({ ...b })),
      panels: clonedPanels,
      overflowPanels: src.overflowPanels.map((p) => ({ ...p, cast: [...p.cast] })),
      settingOverride: src.settingOverride,
      images: {},
      pageImage: null,
      bubbles: src.bubbles.map((b) => ({ ...b, id: newId("b") })),
    });
    this.ensureRt(copy.id);
    const pages = [...this.activeBook.pages];
    pages.splice(index + 1, 0, copy);
    this.activeBook.pages = pages;
    this.activeBook.activePageIndex = index + 1;
    this.selectedBubbleId = null;
    this.persist();
  }

  deletePage(index: number) {
    if (this.running || this.activeBook.pages.length <= 1) return;
    const victim = this.activeBook.pages[index];
    if (!victim) return;
    revokeRuntime(this.runtimeByPage[victim.id]);
    const { [victim.id]: _drop, ...restRt } = this.runtimeByPage;
    this.runtimeByPage = restRt;
    const pages = this.activeBook.pages.filter((_, i) => i !== index);
    this.activeBook.pages = pages;
    const cur = this.activeBook.activePageIndex;
    if (cur >= pages.length) this.activeBook.activePageIndex = pages.length - 1;
    else if (index < cur) this.activeBook.activePageIndex = cur - 1;
    this.selectedBubbleId = null;
    this.persist();
  }

  movePage(from: number, to: number) {
    if (this.running) return;
    const pages = [...this.activeBook.pages];
    if (from < 0 || from >= pages.length || to < 0 || to >= pages.length || from === to) return;
    const activeId = this.activePage.id;
    const [moved] = pages.splice(from, 1);
    pages.splice(to, 0, moved);
    this.activeBook.pages = pages;
    this.activeBook.activePageIndex = Math.max(0, pages.findIndex((p) => p.id === activeId));
    this.persist();
  }

  selectPage(index: number) {
    if (index < 0 || index >= this.activeBook.pages.length) return;
    this.activeBook.activePageIndex = index;
    this.ensureRt(this.activePage.id);
    this.selectedBubbleId = null;
    this.persistSoon();
  }

  // --- book management ---

  newBook(title?: string) {
    if (this.running) return;
    const book = createBook({ title: title?.trim() || undefined }, this.books.length + 1);
    this.books = [...this.books, book];
    this.activeBookId = book.id;
    this.selectedBubbleId = null;
    this.ensureAllRuntimes();
    this.persist();
  }

  switchBook(id: string) {
    if (this.running) return;
    if (!this.books.some((b) => b.id === id)) return;
    this.activeBookId = id;
    this.selectedBubbleId = null;
    this.ensureAllRuntimes();
    this.persistSoon();
  }

  renameBook(title: string, id = this.activeBookId) {
    const clean = title.trim();
    if (!clean) return;
    const book = this.books.find((b) => b.id === id);
    if (!book) return;
    book.title = clean;
    this.persistSoon();
  }

  deleteBook(id = this.activeBookId) {
    if (this.running) return;
    const target = this.books.find((b) => b.id === id);
    if (!target) return;
    for (const p of target.pages) {
      revokeRuntime(this.runtimeByPage[p.id]);
    }
    const nextRt = { ...this.runtimeByPage };
    for (const p of target.pages) delete nextRt[p.id];
    this.runtimeByPage = nextRt;

    if (this.books.length <= 1) {
      const fresh = createBook(undefined, 1);
      this.books = [fresh];
      this.activeBookId = fresh.id;
    } else {
      const nextBooks = this.books.filter((b) => b.id !== id);
      this.books = nextBooks;
      if (this.activeBookId === id) this.activeBookId = nextBooks[0].id;
    }
    this.selectedBubbleId = null;
    this.ensureAllRuntimes();
    this.persist();
  }

  // --- vector speech bubbles ---

  addBubble(panelId?: string): string {
    this.remember();
    const rect = panelId ? this.rects.get(panelId) : undefined;
    const stagger = (this.bubbles.length % 5) * 0.025;
    let bubble: SpeechBubble;
    if (rect) {
      const w = Math.min(0.32, Math.max(0.08, rect.w * 0.65));
      const h = Math.min(0.16, Math.max(0.05, rect.h * 0.32));
      const x = rect.x + Math.max(0.01, (rect.w - w) * 0.5 + (stagger - 0.02));
      const y = rect.y + Math.min(rect.h - h - 0.01, 0.02 + stagger);
      bubble = newBubble({
        panelId,
        x,
        y,
        w,
        h,
        tailX: x + w * 0.5,
        tailY: Math.min(rect.y + rect.h - 0.02, y + h + 0.06),
      });
    } else {
      bubble = newBubble({
        x: 0.34 + stagger,
        y: 0.14 + stagger,
        w: 0.28,
        h: 0.14,
      });
    }
    this.bubbles = [...this.bubbles, bubble];
    this.selectedBubbleId = bubble.id;
    this.persist();
    return bubble.id;
  }

  syncDialogueToBubbles(panelId: string, clearDialogue = false): SpeechBubble[] {
    const panel = this.panels[panelId];
    const rect = this.rects.get(panelId);
    if (!panel || !rect) return [];
    const created = bubblesFromDialogue(panelId, rect, panel.dialogue);
    if (created.length === 0 && !clearDialogue) return [];
    this.remember();
    if (clearDialogue) {
      this.panels = { ...this.panels, [panelId]: { ...panel, dialogue: "" } };
    }
    if (created.length > 0) {
      this.bubbles = [...this.bubbles, ...created];
      this.selectedBubbleId = created[0].id;
    }
    this.persist();
    return created;
  }

  beginBubbleEdit() {
    this.remember();
  }

  updateBubble(id: string, patch: Partial<SpeechBubble>) {
    this.bubbles = this.bubbles.map((b) => (b.id === id ? clampBubble({ ...b, ...patch, id }) : b));
    this.persistSoon();
  }

  removeBubble(id: string) {
    if (!this.bubbles.some((b) => b.id === id)) return;
    this.remember();
    this.bubbles = this.bubbles.filter((b) => b.id !== id);
    if (this.selectedBubbleId === id) this.selectedBubbleId = null;
    this.persist();
  }

  // --- panel, cast and book fields ---

  updatePanel(id: string, patch: Partial<ComicPanel>) {
    const panel = this.panels[id];
    if (!panel) return;
    this.panels = { ...this.panels, [id]: { ...panel, ...patch, id } };
    this.persistSoon();
  }

  toggleCastInPanel(panelId: string, castId: string) {
    const panel = this.panels[panelId];
    if (!panel) return;
    const cast = panel.cast.includes(castId) ? panel.cast.filter((c) => c !== castId) : [...panel.cast, castId];
    this.updatePanel(panelId, { cast });
  }

  addCastMember(initial?: Partial<CastMember>): string {
    const id = newId("c");
    this.cast = [
      ...this.cast,
      {
        id,
        name: initial?.name ?? "",
        tags: initial?.tags ?? "",
        savedCharacterId: initial?.savedCharacterId ?? null,
        referenceImage: initial?.referenceImage ?? null,
      },
    ];
    this.persist();
    return id;
  }

  addCastMemberFromCharacter(character: {
    id: string;
    name: string;
    prompt: string;
    reference: string | null;
    thumbnail: string | null;
  }): string {
    return this.addCastMember({
      name: character.name,
      tags: character.prompt,
      savedCharacterId: character.id,
      referenceImage: character.reference ?? character.thumbnail ?? null,
    });
  }

  updateCastMember(id: string, patch: Partial<CastMember>) {
    this.cast = this.cast.map((c) => (c.id === id ? { ...c, ...patch, id } : c));
    this.persistSoon();
  }

  removeCastMember(id: string) {
    this.cast = this.cast.filter((c) => c.id !== id);
    for (const page of this.activeBook.pages) {
      const next: Record<string, ComicPanel> = {};
      for (const [pid, p] of Object.entries(page.panels)) {
        next[pid] = { ...p, cast: p.cast.filter((c) => c !== id) };
      }
      page.panels = next;
    }
    this.persist();
  }

  setPageField<
    K extends
      | "setting"
      | "settingOverride"
      | "style"
      | "pageShape"
      | "gutter"
      | "rightToLeft"
      | "mode"
      | "checkContinuity"
      | "refMode",
  >(key: K, value: ComicStore[K]) {
    (this as ComicStore)[key] = value;
    this.persistSoon();
  }

  // --- results ---

  setResult(id: string, patch: Partial<PanelResult>, pageId = this.activePage.id) {
    const rt = this.ensureRt(pageId);
    const current = rt.results[id];
    if (patch.url && current?.url && current.url !== patch.url) URL.revokeObjectURL(current.url);
    rt.results = { ...rt.results, [id]: { ...({ status: "queued" } as PanelResult), ...current, ...patch } };
    if (patch.galleryFilename !== undefined) {
      const page = this.findPageById(pageId);
      if (page) {
        if (patch.galleryFilename) page.images = { ...page.images, [id]: patch.galleryFilename };
        else {
          const { [id]: _drop, ...rest } = page.images;
          page.images = rest;
        }
      }
      this.persistSoon();
    }
  }

  clearResult(id: string, pageId = this.activePage.id) {
    const rt = this.ensureRt(pageId);
    const { [id]: _drop, ...rest } = rt.results;
    rt.results = rest;
  }

  setPage(url: string | null, galleryFilename: string | null = null, pageId = this.activePage.id) {
    const rt = this.ensureRt(pageId);
    if (rt.pageUrl && rt.pageUrl !== url) URL.revokeObjectURL(rt.pageUrl);
    rt.pageUrl = url;
    rt.pageGalleryFilename = galleryFilename;
    const page = this.findPageById(pageId);
    if (page) page.pageImage = galleryFilename;
    this.persistSoon();
  }

  clearPage(pageId = this.activePage.id) {
    this.setPage(null, null, pageId);
    const rt = this.ensureRt(pageId);
    rt.pageStatus = "idle";
    rt.pageError = null;
  }

  private findPageById(pageId: string): ComicPageState | undefined {
    for (const b of this.books) {
      const found = b.pages.find((p) => p.id === pageId);
      if (found) return found;
    }
    return undefined;
  }

  /** Start over on the active page: one default layout, empty prompts. The cast and setting stay. */
  resetPanels() {
    this.remember();
    const rt = this.ensureRt(this.activePage.id);
    for (const r of Object.values(rt.results)) if (r.url) URL.revokeObjectURL(r.url);
    for (const r of rt.overflowResults) if (r?.url) URL.revokeObjectURL(r.url);
    rt.results = {};
    rt.overflowResults = [];
    this.activePage.images = {};
    this.activePage.overflowPanels = [];
    this.bubbles = [];
    this.selectedBubbleId = null;
    this.layout = defaultLayout();
    this.layoutMode = "grid";
    this.freeformBoxes = [];
    this.panels = {};
    this.ensurePanels();
    rt.storyIssues = null;
    this.clearPage();
    this.persist();
  }

  // --- persistence ---

  /** Saved gallery files on the active page, for comicRunner to load back after a restart. */
  get savedImages(): Record<string, string> {
    return this.activePage.images;
  }
  get savedPageImage(): string | null {
    return this.activePage.pageImage;
  }

  load() {
    if (this.loaded) return;
    this.loaded = true;
    try {
      const booksRaw = localStorage.getItem(userScopedKey(BOOKS_STORAGE_KEY));
      if (booksRaw) {
        const parsed = JSON.parse(booksRaw) as { books?: unknown[]; activeBookId?: unknown };
        if (Array.isArray(parsed.books) && parsed.books.length > 0) {
          const books = parsed.books.map((b, i) => createBook(b as Partial<ComicBook>, i + 1));
          this.books = books;
          this.activeBookId =
            typeof parsed.activeBookId === "string" && books.some((b) => b.id === parsed.activeBookId)
              ? parsed.activeBookId
              : books[0].id;
          this.ensureAllRuntimes();
          return;
        }
      }

      // Migrate legacy single-page storage (`mooshieui.comic.page.v1`) into Book 1 Page 1.
      const legacyRaw = localStorage.getItem(userScopedKey(LEGACY_PAGE_STORAGE_KEY));
      if (legacyRaw) {
        const data = JSON.parse(legacyRaw) as Partial<SavedPageLegacy>;
        const page = createPage({
          layout: data.layout,
          layoutMode: data.layoutMode,
          freeformBoxes: data.freeformBoxes,
          panels: data.panels,
          overflowPanels: data.overflowPanels,
          images: data.images,
          pageImage: data.pageImage ?? null,
        });
        const book = createBook(
          {
            title: "Book 1",
            cast: data.cast,
            setting: data.setting,
            style: data.style,
            pageShape: data.pageShape,
            gutter: data.gutter,
            rightToLeft: data.rightToLeft,
            mode: data.mode,
            checkContinuity: data.checkContinuity,
            refMode: sanitizeRefMode(undefined, data.usePanel1Ref),
            pages: [page],
            activePageIndex: 0,
          },
          1,
        );
        this.books = [book];
        this.activeBookId = book.id;
        this.ensureAllRuntimes();
        this.persist();
        return;
      }
    } catch (e) {
      console.warn("Comic books could not be restored:", e);
    }
    this.ensureAllRuntimes();
  }

  persist() {
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = null;
    // Strip any accidental non-filename values from images/pageImage before writing.
    const serializedBooks = this.books.map((b) => ({
      ...b,
      pages: b.pages.map((p) => {
        const cleanImages: Record<string, string> = {};
        for (const [k, v] of Object.entries(p.images)) {
          if (typeof v === "string" && v && !v.startsWith("data:")) cleanImages[k] = v;
        }
        return {
          ...p,
          images: cleanImages,
          pageImage: typeof p.pageImage === "string" && p.pageImage && !p.pageImage.startsWith("data:") ? p.pageImage : null,
        };
      }),
    }));
    try {
      localStorage.setItem(
        userScopedKey(BOOKS_STORAGE_KEY),
        JSON.stringify({ books: serializedBooks, activeBookId: this.activeBookId }),
      );
    } catch {
      /* storage full or blocked: the book still works for this session */
    }
  }

  persistSoon() {
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => this.persist(), 400);
  }
}

export const comic = new ComicStore();
