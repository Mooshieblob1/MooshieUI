import { generation } from "./generation.svelte.js";
import { locale } from "./locale.svelte.js";
import type { LoraEntry } from "../types/index.js";

export interface CellSnapshot {
  positivePrompt: string;
  negativePrompt: string;
  checkpoint: string;
  vae: string;
  loras: LoraEntry[];
  samplerName: string;
  scheduler: string;
  steps: number;
  cfg: number;
  seed: number;
  width: number;
  height: number;
  denoise: number;
  batchSize: number;
}

const CELL_COLORS = [
  "rgb(99, 102, 241)",   // indigo
  "rgb(244, 63, 94)",    // rose
  "rgb(34, 197, 94)",    // green
  "rgb(251, 191, 36)",   // amber
  "rgb(168, 85, 247)",   // purple
  "rgb(6, 182, 212)",    // cyan
  "rgb(249, 115, 22)",   // orange
  "rgb(236, 72, 153)",   // pink
  "rgb(16, 185, 129)",   // emerald
  "rgb(59, 130, 246)",   // blue
  "rgb(245, 158, 11)",   // yellow
  "rgb(139, 92, 246)",   // violet
  "rgb(20, 184, 166)",   // teal
  "rgb(239, 68, 68)",    // red
  "rgb(132, 204, 22)",   // lime
  "rgb(217, 70, 239)",   // fuchsia
];

const MAX_CELLS = 16;

class CompareStore {
  enabled = $state(false);
  cells = $state<CellSnapshot[]>([]);
  activeIndex = $state(0);
  cols = $state(1);

  get cellCount(): number {
    return this.cells.length;
  }

  get rows(): number {
    return Math.ceil(this.cells.length / this.cols);
  }

  get activeColor(): string {
    return this.cellColor(this.activeIndex);
  }

  get canAddColumn(): boolean {
    return this.rows * (this.cols + 1) <= MAX_CELLS;
  }

  get canAddRow(): boolean {
    return this.cells.length + this.cols <= MAX_CELLS;
  }

  /** Color based on grid position — stable regardless of grid dimensions. */
  cellColor(index: number): string {
    const col = index % this.cols;
    const row = Math.floor(index / this.cols);
    return CELL_COLORS[(row * 4 + col) % CELL_COLORS.length];
  }

  /** Spreadsheet-style label: columns = A, B, C..., rows = 1, 2, 3... */
  cellLabel(index: number): string {
    const col = index % this.cols;
    const row = Math.floor(index / this.cols);
    const letter = String.fromCharCode(65 + col); // A, B, C...
    return `${letter}${row + 1}`;
  }

  snapshotFromGeneration(): CellSnapshot {
    return {
      positivePrompt: generation.positivePrompt,
      negativePrompt: generation.negativePrompt,
      checkpoint: generation.checkpoint,
      vae: generation.vae,
      loras: generation.loras.map((l) => ({ ...l })),
      samplerName: generation.samplerName,
      scheduler: generation.scheduler,
      steps: generation.steps,
      cfg: generation.cfg,
      seed: generation.seed,
      width: generation.width,
      height: generation.height,
      denoise: generation.denoise,
      batchSize: generation.batchSize,
    };
  }

  applyToGeneration(snap: CellSnapshot) {
    generation.positivePrompt = snap.positivePrompt;
    generation.negativePrompt = snap.negativePrompt;
    generation.checkpoint = snap.checkpoint;
    generation.vae = snap.vae;
    generation.loras = snap.loras.map((l) => ({ ...l }));
    generation.samplerName = snap.samplerName;
    generation.scheduler = snap.scheduler;
    generation.steps = snap.steps;
    generation.cfg = snap.cfg;
    generation.seed = snap.seed;
    generation.width = snap.width;
    generation.height = snap.height;
    generation.denoise = snap.denoise;
    generation.batchSize = snap.batchSize;
  }

  saveActiveCell() {
    if (!this.enabled) return;
    if (this.activeIndex < this.cells.length) {
      const snap = this.snapshotFromGeneration();
      this.cells = this.cells.map((c, i) => (i === this.activeIndex ? snap : c));
    }
  }

  selectCell(index: number) {
    if (index === this.activeIndex) return;
    this.saveActiveCell();
    this.activeIndex = index;
    const snap = this.cells[index];
    if (snap) this.applyToGeneration(snap);
  }

  toggle() {
    if (this.enabled) {
      this.enabled = false;
    } else {
      this.enable();
    }
  }

  enable() {
    const snap = this.snapshotFromGeneration();
    this.cells = [snap];
    this.cols = 1;
    this.activeIndex = 0;
    this.enabled = true;
  }

  private cloneTemplate(): CellSnapshot {
    const template = this.cells[this.activeIndex] ?? this.snapshotFromGeneration();
    return { ...template, loras: template.loras.map((l) => ({ ...l })) };
  }

  addColumn() {
    const currentRows = this.rows;
    if (currentRows * (this.cols + 1) > MAX_CELLS) return;
    this.saveActiveCell();
    const oldCols = this.cols;
    const newCells: CellSnapshot[] = [];
    for (let r = 0; r < currentRows; r++) {
      for (let c = 0; c < oldCols; c++) {
        const idx = r * oldCols + c;
        newCells.push(idx < this.cells.length ? this.cells[idx] : this.cloneTemplate());
      }
      // Clone the last cell in this row (adjacent left neighbor)
      const lastInRow = newCells[newCells.length - 1]!;
      newCells.push({ ...lastInRow, loras: lastInRow.loras.map(l => ({ ...l })) });
    }
    const activeRow = Math.floor(this.activeIndex / oldCols);
    const activeCol = this.activeIndex % oldCols;
    this.cells = newCells;
    this.cols = oldCols + 1;
    this.activeIndex = activeRow * this.cols + activeCol;
  }

  addRow() {
    if (this.cells.length + this.cols > MAX_CELLS) return;
    this.saveActiveCell();
    // Clone each cell from the last row (adjacent above neighbor)
    const lastRowStart = (this.rows - 1) * this.cols;
    const newCells = Array.from({ length: this.cols }, (_, c) => {
      const src = this.cells[lastRowStart + c] ?? this.cloneTemplate();
      return { ...src, loras: src.loras.map(l => ({ ...l })) };
    });
    this.cells = [...this.cells, ...newCells];
  }

  removeColumn(colIndex: number) {
    if (this.cols <= 1) return;
    const oldCols = this.cols;
    const currentRows = this.rows;
    const activeRow = Math.floor(this.activeIndex / oldCols);
    const activeCol = this.activeIndex % oldCols;
    const newCells: CellSnapshot[] = [];
    for (let r = 0; r < currentRows; r++) {
      for (let c = 0; c < oldCols; c++) {
        if (c === colIndex) continue;
        const idx = r * oldCols + c;
        if (idx < this.cells.length) newCells.push(this.cells[idx]);
      }
    }
    const newCols = oldCols - 1;
    const newActiveCol = activeCol > colIndex ? activeCol - 1 : activeCol >= newCols ? newCols - 1 : activeCol;
    this.cells = newCells;
    this.cols = newCols;
    this.activeIndex = activeRow * newCols + newActiveCol;
    if (this.activeIndex >= this.cells.length) this.activeIndex = this.cells.length - 1;
    const snap = this.cells[this.activeIndex];
    if (snap) this.applyToGeneration(snap);
  }

  removeRow(rowIndex: number) {
    if (this.rows <= 1) return;
    const oldCols = this.cols;
    const activeRow = Math.floor(this.activeIndex / oldCols);
    const activeCol = this.activeIndex % oldCols;
    const newCells: CellSnapshot[] = [];
    for (let r = 0; r < this.rows; r++) {
      if (r === rowIndex) continue;
      for (let c = 0; c < oldCols; c++) {
        const idx = r * oldCols + c;
        if (idx < this.cells.length) newCells.push(this.cells[idx]);
      }
    }
    const newActiveRow = activeRow > rowIndex ? activeRow - 1 : activeRow >= Math.ceil(newCells.length / oldCols) ? Math.ceil(newCells.length / oldCols) - 1 : activeRow;
    this.cells = newCells;
    this.activeIndex = newActiveRow * oldCols + activeCol;
    if (this.activeIndex >= this.cells.length) this.activeIndex = this.cells.length - 1;
    const snap = this.cells[this.activeIndex];
    if (snap) this.applyToGeneration(snap);
  }

  cellSummary(index: number): string {
    const snap = this.cells[index];
    if (!snap) return "";
    const ref = this.cells[0];
    if (!ref || index === 0) {
      return this.briefSummary(snap);
    }
    const diffs: string[] = [];
    if (snap.checkpoint !== ref.checkpoint) {
      const name = snap.checkpoint.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? snap.checkpoint;
      diffs.push(name.length > 18 ? name.slice(0, 16) + "…" : name);
    }
    if (snap.seed !== ref.seed && snap.seed !== -1) diffs.push(`seed ${snap.seed}`);
    if (snap.steps !== ref.steps) diffs.push(`${snap.steps} steps`);
    if (snap.cfg !== ref.cfg) diffs.push(`cfg ${snap.cfg}`);
    if (snap.samplerName !== ref.samplerName) diffs.push(snap.samplerName);
    if (snap.width !== ref.width || snap.height !== ref.height) diffs.push(`${snap.width}×${snap.height}`);
    if (snap.denoise !== ref.denoise) diffs.push(`d${snap.denoise}`);
    if (snap.positivePrompt !== ref.positivePrompt) diffs.push(locale.t("compare.cell_prompt_differs"));
    if (diffs.length === 0) return locale.t("compare.cell_same");
    return diffs.slice(0, 3).join(", ");
  }

  private briefSummary(snap: CellSnapshot): string {
    const parts: string[] = [];
    if (snap.checkpoint) {
      const name = snap.checkpoint.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? snap.checkpoint;
      parts.push(name.length > 18 ? name.slice(0, 16) + "…" : name);
    }
    parts.push(`${snap.steps}s`);
    parts.push(`cfg ${snap.cfg}`);
    return parts.slice(0, 3).join(", ");
  }

  // --- Grid batch tracking (non-reactive, used imperatively) ---
  private _gridBatchPromptIds: string[] = [];
  private _gridBatchCols = 0;
  private _gridBatchRows = 0;
  private _gridBatchImages = new Map<string, { blob: Blob; url: string }>();
  private _gridBatchSnapshots: CellSnapshot[] = [];

  startGridBatch(promptIds: string[], rows: number, cols: number, snapshots: CellSnapshot[], failedCells: number[] = []) {
    this._gridBatchPromptIds = promptIds;
    this._gridBatchSnapshots = snapshots;
    // If some cells failed, adjust grid dimensions to fit the successful cell count
    const successCount = promptIds.length;
    if (failedCells.length > 0 || successCount !== rows * cols) {
      this._gridBatchCols = Math.min(cols, successCount);
      this._gridBatchRows = this._gridBatchCols > 0 ? Math.ceil(successCount / this._gridBatchCols) : 0;
    } else {
      this._gridBatchCols = cols;
      this._gridBatchRows = rows;
    }
    this._gridBatchImages = new Map();
  }

  isGridPrompt(promptId: string): boolean {
    return this._gridBatchPromptIds.includes(promptId);
  }

  /** Record a completed grid cell image. Returns stitching data when all cells are done, null otherwise. */
  addGridResult(promptId: string, image: { blob: Blob; url: string }): {
    images: { blob: Blob; url: string }[];
    rows: number;
    cols: number;
    cellLabels: string[];
  } | null {
    if (!this._gridBatchPromptIds.includes(promptId)) return null;
    this._gridBatchImages.set(promptId, image);
    if (this._gridBatchImages.size < this._gridBatchPromptIds.length) return null;
    const ordered = this._gridBatchPromptIds.map(pid => this._gridBatchImages.get(pid)!);
    const cellLabels = this.computeCellLabels();
    const result = {
      images: ordered,
      rows: this._gridBatchRows,
      cols: this._gridBatchCols,
      cellLabels,
    };
    this.clearGridBatch();
    return result;
  }

  /** Compute a per-cell label showing only what's unique about each cell vs the reference (cell 0). */
  private computeCellLabels(): string[] {
    const snaps = this._gridBatchSnapshots;
    const ref = snaps[0];
    if (!ref) return snaps.map(() => "");

    return snaps.map((snap, i) => {
      if (i === 0 || !snap) return locale.t("compare.cell_base");
      const diffs: string[] = [];

      if (snap.checkpoint !== ref.checkpoint) {
        const name = snap.checkpoint.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? snap.checkpoint;
        diffs.push(name.length > 30 ? name.slice(0, 28) + "…" : name);
      }
      if (snap.positivePrompt !== ref.positivePrompt) {
        diffs.push(CompareStore.uniqueTags(ref.positivePrompt, snap.positivePrompt));
      }
      if (snap.negativePrompt !== ref.negativePrompt) {
        diffs.push(locale.t("compare.cell_neg_prefix") + CompareStore.uniqueTags(ref.negativePrompt, snap.negativePrompt));
      }
      if (snap.samplerName !== ref.samplerName) diffs.push(snap.samplerName);
      if (snap.scheduler !== ref.scheduler) diffs.push(snap.scheduler);
      if (snap.steps !== ref.steps) diffs.push(`${snap.steps} steps`);
      if (snap.cfg !== ref.cfg) diffs.push(`cfg ${snap.cfg}`);
      if (snap.seed !== ref.seed && snap.seed !== -1) diffs.push(`seed ${snap.seed}`);
      if (snap.width !== ref.width || snap.height !== ref.height) diffs.push(`${snap.width}×${snap.height}`);
      if (snap.denoise !== ref.denoise) diffs.push(`denoise ${snap.denoise}`);
      if (snap.vae !== ref.vae) {
        const vName = snap.vae ? (snap.vae.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? snap.vae) : locale.t("compare.cell_default_vae");
        diffs.push(vName);
      }
      if (JSON.stringify(snap.loras) !== JSON.stringify(ref.loras)) diffs.push(locale.t("compare.cell_loras_differ"));

      return diffs.length === 0 ? locale.t("compare.cell_same") : diffs.join(", ");
    });
  }

  /** Return only the tags that differ between ref and target prompts. */
  private static uniqueTags(refPrompt: string, targetPrompt: string): string {
    const toTags = (p: string) => p.split(",").map(t => t.trim()).filter(Boolean);
    const refTags = new Set(toTags(refPrompt));
    const targetTags = toTags(targetPrompt);
    // Tags in target that aren't in ref (the unique/changed ones)
    const unique = targetTags.filter(t => !refTags.has(t));
    if (unique.length > 0) return unique.slice(0, 4).join(", ") + (unique.length > 4 ? ` +${unique.length - 4}` : "");
    // If no new tags but prompts differ (reordered or removed), show what was removed
    const removedTags = [...refTags].filter(t => !new Set(targetTags).has(t));
    if (removedTags.length > 0) return "−" + removedTags.slice(0, 3).join(", ");
    return locale.t("compare.cell_prompt_changed");
  }

  clearGridBatch() {
    this._gridBatchPromptIds = [];
    this._gridBatchCols = 0;
    this._gridBatchRows = 0;
    this._gridBatchImages = new Map();
    this._gridBatchSnapshots = [];
  }
}

export const compare = new CompareStore();
