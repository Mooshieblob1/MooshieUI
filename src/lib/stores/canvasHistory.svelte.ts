import Konva from "konva";

interface HistoryEntry {
  layerId: string;
  imageData: string; // base64 data URL from layer.toDataURL()
}

const MAX_HISTORY = 64;

class CanvasHistoryStore {
  undoStack = $state<HistoryEntry[][]>([]);
  redoStack = $state<HistoryEntry[][]>([]);

  private _konvaLayers: Map<string, Konva.Layer> | null = null;
  private _canvasWidth = 0;
  private _canvasHeight = 0;

  setRefs(konvaLayers: Map<string, Konva.Layer>, canvasWidth: number, canvasHeight: number) {
    this._konvaLayers = konvaLayers;
    this._canvasWidth = canvasWidth;
    this._canvasHeight = canvasHeight;
  }

  get canUndo(): boolean {
    return this.undoStack.length > 0;
  }

  get canRedo(): boolean {
    return this.redoStack.length > 0;
  }

  // Snapshot specific layers before a destructive operation
  snapshotLayers(layerIds: string[]) {
    if (!this._konvaLayers) return;

    const entries: HistoryEntry[] = [];
    for (const id of layerIds) {
      const kLayer = this._konvaLayers.get(id);
      if (!kLayer) continue;

      // Save current transform, reset for export
      const origScale = kLayer.scaleX();
      const origX = kLayer.x();
      const origY = kLayer.y();
      kLayer.scaleX(1);
      kLayer.scaleY(1);
      kLayer.x(0);
      kLayer.y(0);

      const dataUrl = kLayer.toDataURL({
        pixelRatio: 1,
        width: this._canvasWidth,
        height: this._canvasHeight,
      });

      // Restore transform
      kLayer.scaleX(origScale);
      kLayer.scaleY(origScale);
      kLayer.x(origX);
      kLayer.y(origY);

      entries.push({ layerId: id, imageData: dataUrl });
    }

    if (entries.length > 0) {
      this.undoStack = [...this.undoStack.slice(-(MAX_HISTORY - 1)), entries];
      // Clear redo when a new action is taken
      this.redoStack = [];
    }
  }

  // Snapshot a single layer (convenience)
  snapshot(layerId: string) {
    this.snapshotLayers([layerId]);
  }

  async undo() {
    if (!this.canUndo || !this._konvaLayers) return;

    const entries = this.undoStack[this.undoStack.length - 1];

    // Before restoring, snapshot current state for redo
    const redoEntries: HistoryEntry[] = [];
    for (const entry of entries) {
      const kLayer = this._konvaLayers.get(entry.layerId);
      if (!kLayer) continue;

      const origScale = kLayer.scaleX();
      const origX = kLayer.x();
      const origY = kLayer.y();
      kLayer.scaleX(1);
      kLayer.scaleY(1);
      kLayer.x(0);
      kLayer.y(0);

      const dataUrl = kLayer.toDataURL({
        pixelRatio: 1,
        width: this._canvasWidth,
        height: this._canvasHeight,
      });

      kLayer.scaleX(origScale);
      kLayer.scaleY(origScale);
      kLayer.x(origX);
      kLayer.y(origY);

      redoEntries.push({ layerId: entry.layerId, imageData: dataUrl });
    }

    this.redoStack = [...this.redoStack, redoEntries];
    this.undoStack = this.undoStack.slice(0, -1);

    // Restore layers from snapshot
    await this._restoreEntries(entries);
  }

  async redo() {
    if (!this.canRedo || !this._konvaLayers) return;

    const entries = this.redoStack[this.redoStack.length - 1];

    // Before restoring, snapshot current state for undo
    const undoEntries: HistoryEntry[] = [];
    for (const entry of entries) {
      const kLayer = this._konvaLayers.get(entry.layerId);
      if (!kLayer) continue;

      const origScale = kLayer.scaleX();
      const origX = kLayer.x();
      const origY = kLayer.y();
      kLayer.scaleX(1);
      kLayer.scaleY(1);
      kLayer.x(0);
      kLayer.y(0);

      const dataUrl = kLayer.toDataURL({
        pixelRatio: 1,
        width: this._canvasWidth,
        height: this._canvasHeight,
      });

      kLayer.scaleX(origScale);
      kLayer.scaleY(origScale);
      kLayer.x(origX);
      kLayer.y(origY);

      undoEntries.push({ layerId: entry.layerId, imageData: dataUrl });
    }

    this.undoStack = [...this.undoStack, undoEntries];
    this.redoStack = this.redoStack.slice(0, -1);

    await this._restoreEntries(entries);
  }

  private async _restoreEntries(entries: HistoryEntry[]) {
    if (!this._konvaLayers) return;

    for (const entry of entries) {
      const kLayer = this._konvaLayers.get(entry.layerId);
      if (!kLayer) continue;

      // Clear the layer
      kLayer.destroyChildren();

      // Load the snapshot image
      const img = await this._loadImage(entry.imageData);
      const kImage = new Konva.Image({
        image: img,
        x: 0,
        y: 0,
        width: this._canvasWidth,
        height: this._canvasHeight,
        listening: false,
      });
      kLayer.add(kImage);
      kLayer.batchDraw();
    }
  }

  private _loadImage(src: string): Promise<HTMLImageElement> {
    return new Promise((resolve, reject) => {
      const img = new Image();
      img.onload = () => resolve(img);
      img.onerror = reject;
      img.src = src;
    });
  }

  clear() {
    this.undoStack = [];
    this.redoStack = [];
  }
}

export const canvasHistory = new CanvasHistoryStore();
