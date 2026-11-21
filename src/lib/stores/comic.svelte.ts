// Comic page builder state: the page being laid out (persisted per user) and
// the state of the current run (not persisted). Generation, checks and page
// assembly live in utils/comicRunner.ts, which drives this store.
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
  type SplitDir,
} from "../utils/comicLayout.js";

const STORAGE_KEY = "mooshieui.comic.page.v1";
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

interface SavedPage {
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
  /** Gallery file per panel. */
  images?: Record<string, string>;
  pageImage?: string | null;
}

function defaultLayout(): LayoutNode {
  return buildTemplate(LAYOUT_TEMPLATES.find((t) => t.id === "one_over_two")!);
}

class ComicStore {
  layout = $state<LayoutNode>(defaultLayout());
  panels = $state<Record<string, ComicPanel>>({});
  cast = $state<CastMember[]>([]);
  /** Page setting: location, time of day, lighting. Every panel inherits it. */
  setting = $state("");
  /** Style tags for every panel: "manga, monochrome, screentone". */
  style = $state("");
  pageShape = $state<PageShape>("portrait");
  /** Space between panels on the assembled page, in output pixels. */
  gutter = $state(24);
  rightToLeft = $state(false);
  mode = $state<ComicMode>("panels");
  /** Run the tagger on each finished panel and flag what changed. */
  checkContinuity = $state(true);
  /** Pass Panel 1's generated image as a character reference to later panels. */
  usePanel1Ref = $state(false);
  selectedId = $state<string | null>(null);

  // --- run state (not persisted) ---
  results = $state<Record<string, PanelResult>>({});
  /** The assembled page, or the one-image page. */
  pageUrl = $state<string | null>(null);
  pageGalleryFilename = $state<string | null>(null);
  pageStatus = $state<"idle" | "running" | "done" | "failed">("idle");
  pageError = $state<string | null>(null);
  running = $state(false);
  stopRequested = $state(false);
  storyIssues = $state<Array<{ panel: number; issue: string }> | null>(null);
  storyChecking = $state(false);
  storyError = $state<string | null>(null);
  drafting = $state(false);
  /** The panel the Prompt Assistant is rewriting on its own, if any. */
  enhancingId = $state<string | null>(null);
  /** Panel prompts and dialogue before the last draft or panel rewrite, for undoing it. */
  draftUndo = $state<Record<string, { prompt: string; dialogue: string }> | null>(null);

  private history: string[] = [];
  private saveTimer: ReturnType<typeof setTimeout> | null = null;
  private loaded = false;

  constructor() {
    this.ensurePanels();
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
    return this.history.length > 0;
  }

  get panelCount(): number {
    return panelIds(this.layout).length;
  }

  // --- layout edits ---

  private remember() {
    this.history = [...this.history.slice(-(HISTORY_LIMIT - 1)), JSON.stringify({ layout: this.layout, panels: this.panels })];
  }

  /** Make sure every leaf has a panel record and no stale record lingers. */
  private ensurePanels() {
    const ids = panelIds(this.layout);
    const next: Record<string, ComicPanel> = {};
    for (const id of ids) next[id] = this.panels[id] ?? newPanel(id);
    this.panels = next;
    if (this.selectedId && !next[this.selectedId]) this.selectedId = null;
  }

  applyTemplate(template: LayoutTemplate) {
    this.remember();
    // Keep the prompts already written, in reading order, for the new panels.
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
    this.results = {};
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
    const { [id]: _gone, ...rest } = this.results;
    this.results = rest;
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
    const last = this.history[this.history.length - 1];
    if (!last) return;
    this.history = this.history.slice(0, -1);
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

  // --- panel and page fields ---

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

  addCastMember() {
    this.cast = [...this.cast, { id: newId("c"), name: "", tags: "" }];
    this.persist();
  }

  updateCastMember(id: string, patch: Partial<CastMember>) {
    this.cast = this.cast.map((c) => (c.id === id ? { ...c, ...patch, id } : c));
    this.persistSoon();
  }

  removeCastMember(id: string) {
    this.cast = this.cast.filter((c) => c.id !== id);
    const next: Record<string, ComicPanel> = {};
    for (const [pid, p] of Object.entries(this.panels)) next[pid] = { ...p, cast: p.cast.filter((c) => c !== id) };
    this.panels = next;
    this.persist();
  }

  setPageField<K extends "setting" | "style" | "pageShape" | "gutter" | "rightToLeft" | "mode" | "checkContinuity" | "usePanel1Ref">(
    key: K,
    value: ComicStore[K],
  ) {
    (this as ComicStore)[key] = value;
    this.persistSoon();
  }

  // --- results ---

  setResult(id: string, patch: Partial<PanelResult>) {
    const current = this.results[id];
    if (patch.url && current?.url && current.url !== patch.url) URL.revokeObjectURL(current.url);
    this.results = { ...this.results, [id]: { ...({ status: "queued" } as PanelResult), ...current, ...patch } };
    if (patch.galleryFilename !== undefined) this.persistSoon();
  }

  setPage(url: string | null, galleryFilename: string | null = null) {
    if (this.pageUrl && this.pageUrl !== url) URL.revokeObjectURL(this.pageUrl);
    this.pageUrl = url;
    this.pageGalleryFilename = galleryFilename;
    this.persistSoon();
  }

  clearPage() {
    this.setPage(null);
    this.pageStatus = "idle";
    this.pageError = null;
  }

  /** Start over: one default layout, empty prompts. The cast and setting stay. */
  resetPanels() {
    this.remember();
    for (const r of Object.values(this.results)) if (r.url) URL.revokeObjectURL(r.url);
    this.results = {};
    this.layout = defaultLayout();
    this.panels = {};
    this.ensurePanels();
    this.storyIssues = null;
    this.clearPage();
    this.persist();
  }

  // --- persistence ---

  /** Saved gallery files, for comicRunner to load back after a restart. */
  savedImages: Record<string, string> = {};
  savedPageImage: string | null = null;

  load() {
    if (this.loaded) return;
    this.loaded = true;
    try {
      const raw = localStorage.getItem(userScopedKey(STORAGE_KEY));
      if (!raw) return;
      const data = JSON.parse(raw) as Partial<SavedPage>;
      if (data.layout && isLayout(data.layout)) this.layout = data.layout;
      if (data.panels && typeof data.panels === "object") {
        const panels: Record<string, ComicPanel> = {};
        for (const [id, p] of Object.entries(data.panels)) {
          panels[id] = { ...newPanel(id), ...(p as Partial<ComicPanel>), id };
        }
        this.panels = panels;
      }
      if (Array.isArray(data.cast)) {
        this.cast = data.cast
          .filter((c) => c && typeof c.id === "string")
          .map((c) => ({ id: c.id, name: String(c.name ?? ""), tags: String(c.tags ?? "") }));
      }
      if (typeof data.setting === "string") this.setting = data.setting;
      if (typeof data.style === "string") this.style = data.style;
      if (data.pageShape === "portrait" || data.pageShape === "square" || data.pageShape === "landscape") this.pageShape = data.pageShape;
      if (typeof data.gutter === "number") this.gutter = Math.min(80, Math.max(0, data.gutter));
      if (typeof data.rightToLeft === "boolean") this.rightToLeft = data.rightToLeft;
      if (data.mode === "panels" || data.mode === "one_image") this.mode = data.mode;
      if (typeof data.checkContinuity === "boolean") this.checkContinuity = data.checkContinuity;
      if (typeof data.usePanel1Ref === "boolean") this.usePanel1Ref = data.usePanel1Ref;
      this.savedImages = data.images && typeof data.images === "object" ? data.images : {};
      this.savedPageImage = typeof data.pageImage === "string" ? data.pageImage : null;
    } catch (e) {
      console.warn("Comic page could not be restored:", e);
    }
    this.ensurePanels();
  }

  persist() {
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = null;
    const images: Record<string, string> = {};
    for (const [id, r] of Object.entries(this.results)) if (r.galleryFilename) images[id] = r.galleryFilename;
    const data: SavedPage = {
      layout: this.layout,
      panels: this.panels,
      cast: this.cast,
      setting: this.setting,
      style: this.style,
      pageShape: this.pageShape,
      gutter: this.gutter,
      rightToLeft: this.rightToLeft,
      mode: this.mode,
      checkContinuity: this.checkContinuity,
      usePanel1Ref: this.usePanel1Ref,
      images,
      pageImage: this.pageGalleryFilename,
    };
    try {
      localStorage.setItem(userScopedKey(STORAGE_KEY), JSON.stringify(data));
    } catch {
      /* storage full or blocked: the page still works for this session */
    }
  }

  persistSoon() {
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => this.persist(), 400);
  }
}

export const comic = new ComicStore();
