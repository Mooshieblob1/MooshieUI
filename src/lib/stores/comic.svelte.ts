// Comic book builder state: multi-page books persisted per user (lightweight:
// gallery filenames only, never raw PNG bytes) and per-page runtime caches
// (object URLs, generation statuses, undo history) keyed by pageId.
// Generation, checks, assembly and export live in utils/comicRunner.ts.
//
// A leaf store: it imports no other store.

import { userScopedKey } from "../utils/ipc.js";
import {
  LAYOUT_TEMPLATES,
  MAX_PANELS,
  buildTemplate,
  isLayout,
  newId,
  newPanel,
  panelIds,
  panelRects,
  readingOrder,
  removePanel,
  setRatio,
  splitPanel,
  type CastMember,
  type ComicPanel,
  type ContinuityIssue,
  type LayoutNode,
  type LayoutTemplate,
  type PageShape,
  type Rect,
  type RefMode,
  type SplitDir,
} from "../utils/comicLayout.js";

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
  layout: LayoutNode;
  panels: Record<string, ComicPanel>;
  /** Optional setting override for this page; blank inherits the book setting. */
  settingOverride: string;
  /** Gallery filename per panel. */
  images: Record<string, string>;
  /** Assembled or one-image page gallery filename. */
  pageImage: string | null;
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
  panels: Record<string, ComicPanel>;
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

function ensurePanelsFor(layout: LayoutNode, existing: Record<string, ComicPanel>): Record<string, ComicPanel> {
  const ids = panelIds(layout);
  const next: Record<string, ComicPanel> = {};
  for (const id of ids) {
    const p = existing[id];
    next[id] = p ? { ...newPanel(id), ...p, id } : newPanel(id);
  }
  return next;
}

function createPage(partial?: Partial<ComicPageState>): ComicPageState {
  const layout = partial?.layout && isLayout(partial.layout) ? partial.layout : defaultLayout();
  const panels = ensurePanelsFor(layout, partial?.panels ?? {});
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
  return {
    id: typeof partial?.id === "string" && partial.id ? partial.id : newId("pg"),
    title: typeof partial?.title === "string" ? partial.title : "",
    layout,
    panels,
    settingOverride: typeof partial?.settingOverride === "string" ? partial.settingOverride : "",
    images,
    pageImage,
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
  if (rt.pageUrl) URL.revokeObjectURL(rt.pageUrl);
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

  get layout(): LayoutNode {
    return this.activePage.layout;
  }
  set layout(v: LayoutNode) {
    this.activePage.layout = v;
  }

  get panels(): Record<string, ComicPanel> {
    return this.activePage.panels;
  }
  set panels(v: Record<string, ComicPanel>) {
    this.activePage.panels = v;
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

  orderedPanelsForPage(page: ComicPageState, rightToLeft = this.activeBook.rightToLeft): ComicPanel[] {
    const rects = panelRects(page.layout);
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
    return panelRects(this.layout);
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

  get canUndo(): boolean {
    return this.rt().history.length > 0;
  }

  get panelCount(): number {
    return panelIds(this.layout).length;
  }

  // --- layout edits ---

  private remember() {
    const rt = this.ensureRt(this.activePage.id);
    rt.history = [...rt.history.slice(-(HISTORY_LIMIT - 1)), JSON.stringify({ layout: this.layout, panels: this.panels })];
  }

  /** Make sure every leaf has a panel record and no stale record lingers. */
  private ensurePanels() {
    const next = ensurePanelsFor(this.layout, this.panels);
    this.panels = next;
    const rt = this.ensureRt(this.activePage.id);
    if (rt.selectedId && !next[rt.selectedId]) rt.selectedId = null;
  }

  applyTemplate(template: LayoutTemplate) {
    this.remember();
    const previous = this.orderedPanels;
    this.layout = buildTemplate(template);
    const rects = panelRects(this.layout);
    const ids = readingOrder(rects, this.rightToLeft);
    const next: Record<string, ComicPanel> = {};
    ids.forEach((id, i) => {
      const old = previous[i];
      next[id] = old ? { ...old, id } : newPanel(id);
    });
    this.panels = next;
    for (const r of Object.values(this.results)) if (r.url) URL.revokeObjectURL(r.url);
    this.results = {};
    this.activePage.images = {};
    this.selectedId = ids[0] ?? null;
    this.clearPage();
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
    this.layout = removePanel(this.layout, id);
    this.ensurePanels();
    const gone = this.results[id];
    if (gone?.url) URL.revokeObjectURL(gone.url);
    const { [id]: _gone, ...rest } = this.results;
    this.results = rest;
    const { [id]: _goneImg, ...restImgs } = this.activePage.images;
    this.activePage.images = restImgs;
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
      const state = JSON.parse(last) as { layout: LayoutNode; panels: Record<string, ComicPanel> };
      if (!isLayout(state.layout)) return;
      this.layout = state.layout;
      this.panels = state.panels;
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
      layout: JSON.parse(JSON.stringify(src.layout)) as LayoutNode,
      panels: clonedPanels,
      settingOverride: src.settingOverride,
      images: {},
      pageImage: null,
    });
    this.ensureRt(copy.id);
    const pages = [...this.activeBook.pages];
    pages.splice(index + 1, 0, copy);
    this.activeBook.pages = pages;
    this.activeBook.activePageIndex = index + 1;
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
    this.persistSoon();
  }

  // --- book management ---

  newBook(title?: string) {
    if (this.running) return;
    const book = createBook({ title: title?.trim() || undefined }, this.books.length + 1);
    this.books = [...this.books, book];
    this.activeBookId = book.id;
    this.ensureAllRuntimes();
    this.persist();
  }

  switchBook(id: string) {
    if (this.running) return;
    if (!this.books.some((b) => b.id === id)) return;
    this.activeBookId = id;
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
    this.ensureAllRuntimes();
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
    rt.results = {};
    this.activePage.images = {};
    this.layout = defaultLayout();
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
          panels: data.panels,
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
