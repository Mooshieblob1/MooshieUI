import { uploadImageBytes } from "../utils/api.js";
import { generation } from "./generation.svelte.js";
import { locale } from "./locale.svelte.js";

export type ToolType = "brush" | "eraser" | "rectFill" | "lasso" | "eyedropper" | "move" | "view" | "transform";

export interface CanvasLayer {
  id: string;
  name: string;
  type: "raster" | "mask";
  visible: boolean;
  opacity: number;
  locked: boolean;
  order: number;
}

export interface BrushSettings {
  size: number;
  opacity: number;
}

export interface BoundingBox {
  x: number;
  y: number;
  width: number;
  height: number;
  locked: boolean;
}

export interface CanvasStagingEntry {
  url: string;
  owned: boolean;
}

export interface InpaintBaseSnapshot {
  previewUrl: string | null;
  uploadedInputName: string | null;
  width: number;
  height: number;
  maskSnapshotUrl: string | null;
  owned: boolean;
}

export interface CanvasViewport {
  zoom: number;
  panX: number;
  panY: number;
}

export interface TransformState {
  isMoving: boolean;
  targetLayerId: string | null;
  startX: number;
  startY: number;
  deltaX: number;
  deltaY: number;
}

let nextLayerId = 0;
function genLayerId(): string {
  return `layer_${++nextLayerId}`;
}

class CanvasStore {
  // Tool state
  activeTool = $state<ToolType>("brush");
  previousTool = $state<ToolType | null>(null);
  brushSettings = $state<BrushSettings>({ size: 20, opacity: 1 });
  foregroundColor = $state("#ffffff");
  backgroundColor = $state("#000000");

  // Layers
  layers = $state<CanvasLayer[]>([]);
  activeLayerId = $state<string | null>(null);

  // Per-layer pixel thumbnails (data URLs), keyed by layer id
  layerThumbnails = $state<Record<string, string>>({});

  // Canvas document dimensions
  canvasWidth = $state(1024);
  canvasHeight = $state(1024);

  // Viewport
  viewport = $state<CanvasViewport>({ zoom: 1, panX: 0, panY: 0 });

  // Bounding box (generation region)
  boundingBox = $state<BoundingBox>({ x: 0, y: 0, width: 1024, height: 1024, locked: false });

  // Mask overlay
  maskOverlayColor = $state("#ff3333");
  maskOverlayOpacity = $state(0.45);
  maskOverlayVisible = $state(true);

  // UI state
  isCanvasMode = $state(false);
  isPointerOverStage = $state(false);
  inpaintDrawMode = $state<"mask" | "regular">("regular");
  showGrid = $state(false);
  showRuleOfThirds = $state(false);
  showCheckerboard = $state(true);
  cursorPos = $state<{ x: number; y: number } | null>(null);
  referenceImageUrl = $state<string | null>(null);
  originalInpaintInputImageName = $state<string | null>(null);
  originalInpaintWidth = $state<number | null>(null);
  originalInpaintHeight = $state<number | null>(null);
  preparedInpaintPreviewUrl = $state<string | null>(null);
  preparedInpaintOwned = $state(false);
  inpaintSourceVersion = $state(0);
  persistedMaskPreviewUrl = $state<string | null>(null);
  // Base-image undo history for iterative inpainting. Each entry is a base that
  // was inpainted plus the mask that was applied to it, so the user can step
  // back and so the same mask can be re-hydrated onto the incoming result.
  inpaintBaseHistory = $state<InpaintBaseSnapshot[]>([]);
  // A tinted mask snapshot waiting to be re-hydrated onto the freshly-rebuilt
  // mask layer after a base swap (consumed by CanvasStage).
  pendingMaskRestoreUrl = $state<string | null>(null);
  // The latest inpaint result, held for DISPLAY ONLY. Pressing "Generate" always
  // re-rolls the current base + mask (never this result); it is only shown as the
  // canvas background so the user can preview it. "Apply" promotes it to the base.
  pendingResultPreviewUrl = $state<string | null>(null);
  pendingResultOwned = $state(false);
  pendingResultInputName = $state<string | null>(null);
  pendingResultWidth = $state<number | null>(null);
  pendingResultHeight = $state<number | null>(null);
  // While a finished inpaint result is being previewed, the editable mask strokes
  // are hidden so the clean result is visible. This flips true once the user starts
  // painting more mask, bringing the mask back so they can see what they're editing.
  maskEditedSinceResult = $state(false);

  // Staging
  stagingImages = $state<CanvasStagingEntry[]>([]);
  stagingIndex = $state(0);
  isStagingActive = $state(false);

  // Move/transform
  transform = $state<TransformState>({
    isMoving: false,
    targetLayerId: null,
    startX: 0,
    startY: 0,
    deltaX: 0,
    deltaY: 0,
  });

  // Reference to the Konva stage (set by CanvasStage)
  private _stageRef: any = null;

  setStageRef(stage: any) {
    this._stageRef = stage;
  }

  getStageRef(): any {
    return this._stageRef;
  }

  // Derived
  get activeLayer(): CanvasLayer | null {
    return this.layers.find((l) => l.id === this.activeLayerId) ?? null;
  }

  get visibleLayers(): CanvasLayer[] {
    return this.layers.filter((l) => l.visible).sort((a, b) => a.order - b.order);
  }

  get sortedLayers(): CanvasLayer[] {
    return [...this.layers].sort((a, b) => b.order - a.order);
  }

  get zoomPercent(): number {
    return Math.round(this.viewport.zoom * 100);
  }

  // Colors
  swapColors() {
    const tmp = this.foregroundColor;
    this.foregroundColor = this.backgroundColor;
    this.backgroundColor = tmp;
  }

  resetColors() {
    this.foregroundColor = "#ffffff";
    this.backgroundColor = "#000000";
  }

  // Tools
  setTool(tool: ToolType) {
    if (tool !== this.activeTool) {
      this.previousTool = this.activeTool;
      this.activeTool = tool;
    }
  }

  restorePreviousTool() {
    if (this.previousTool) {
      this.activeTool = this.previousTool;
      this.previousTool = null;
    }
  }

  beginMove(layerId: string, startX: number, startY: number) {
    this.transform = {
      isMoving: true,
      targetLayerId: layerId,
      startX,
      startY,
      deltaX: 0,
      deltaY: 0,
    };
  }

  updateMove(currentX: number, currentY: number) {
    if (!this.transform.isMoving) return;
    this.transform = {
      ...this.transform,
      deltaX: currentX - this.transform.startX,
      deltaY: currentY - this.transform.startY,
    };
  }

  endMove() {
    this.transform = {
      isMoving: false,
      targetLayerId: null,
      startX: 0,
      startY: 0,
      deltaX: 0,
      deltaY: 0,
    };
  }

  private revokeOwnedUrls(urls: string[]) {
    const seen = new Set<string>();
    for (const url of urls) {
      if (!url || seen.has(url)) continue;
      seen.add(url);
      URL.revokeObjectURL(url);
    }
  }

  private clearPreparedInpaintOverride() {
    if (this.preparedInpaintOwned && this.preparedInpaintPreviewUrl) {
      URL.revokeObjectURL(this.preparedInpaintPreviewUrl);
    }
    this.preparedInpaintPreviewUrl = null;
    this.preparedInpaintOwned = false;
  }

  // Discard the display-only pending inpaint result, revoking its owned URL.
  private clearPendingInpaintResult() {
    if (this.pendingResultOwned && this.pendingResultPreviewUrl) {
      URL.revokeObjectURL(this.pendingResultPreviewUrl);
    }
    this.pendingResultPreviewUrl = null;
    this.pendingResultOwned = false;
    this.pendingResultInputName = null;
    this.pendingResultWidth = null;
    this.pendingResultHeight = null;
    this.maskEditedSinceResult = false;
  }

  setInpaintOriginalSource(source: {
    previewUrl: string;
    width: number;
    height: number;
    uploadedInputName: string | null;
  } | null) {
    this.clearPreparedInpaintOverride();
    this.clearPendingInpaintResult();
    this.clearInpaintBaseHistory();
    this.inpaintSourceVersion += 1;

    if (!source) {
      this.originalInpaintInputImageName = null;
      this.originalInpaintWidth = null;
      this.originalInpaintHeight = null;
      this.referenceImageUrl = null;
      return;
    }

    this.referenceImageUrl = source.previewUrl;
    this.originalInpaintInputImageName = source.uploadedInputName;
    this.originalInpaintWidth = source.width;
    this.originalInpaintHeight = source.height;
    generation.inputImage = source.uploadedInputName;
    generation.width = source.width;
    generation.height = source.height;
    this.clearMask();
    this.initCanvas(source.width, source.height);
  }

  setPreparedInpaintOverride(source: {
    previewUrl: string;
    width: number;
    height: number;
    uploadedInputName: string | null;
    owned: boolean;
  }) {
    // Swapping to an explicit new base discards any display-only pending result.
    this.clearPendingInpaintResult();
    // Snapshot the outgoing base and the mask applied to it so the user can undo
    // back to it, and so the same mask can be re-hydrated onto the incoming
    // result (letting "Generate" re-roll the same region without repainting).
    const outgoingMask = this.snapshotInpaintMask();
    this.inpaintBaseHistory = [
      ...this.inpaintBaseHistory,
      {
        previewUrl: this.preparedInpaintPreviewUrl ?? this.referenceImageUrl,
        uploadedInputName: generation.inputImage,
        width: generation.width,
        height: generation.height,
        maskSnapshotUrl: outgoingMask,
        // Only a prepared preview is an owned object URL; the session-original
        // referenceImageUrl is owned elsewhere and must not be revoked here.
        owned: this.preparedInpaintPreviewUrl ? this.preparedInpaintOwned : false,
      },
    ];

    // Swap in the new base. Do NOT revoke the outgoing prepared URL: the history
    // entry above now owns it.
    this.preparedInpaintPreviewUrl = source.previewUrl;
    this.preparedInpaintOwned = source.owned;
    generation.inputImage = source.uploadedInputName;
    generation.width = source.width;
    generation.height = source.height;

    // Rebuild layers for the new base, then re-hydrate the mask onto the fresh
    // mask layer. Clear the persisted (non-editable) overlay so the mask isn't
    // drawn twice.
    this.persistedMaskPreviewUrl = null;
    this.initCanvas(source.width, source.height);
    this.pendingMaskRestoreUrl = outgoingMask;
  }

  // Called on inpaint completion. Holds the result for DISPLAY ONLY: it becomes
  // the canvas background so the user can preview it, but the generation base
  // (generation.inputImage) and the editable mask are left untouched, so the next
  // "Generate" re-rolls the ORIGINAL base + mask instead of iterating on the
  // result. Promoting the result to the base is an explicit "Apply" action.
  setPendingInpaintResult(source: {
    previewUrl: string;
    width: number;
    height: number;
    uploadedInputName: string | null;
    owned: boolean;
  }) {
    // A superseded re-roll: revoke the previous pending preview before replacing.
    this.clearPendingInpaintResult();
    this.pendingResultPreviewUrl = source.previewUrl;
    this.pendingResultOwned = source.owned;
    this.pendingResultInputName = source.uploadedInputName;
    this.pendingResultWidth = source.width;
    this.pendingResultHeight = source.height;
  }

  // Promote the pending inpaint result to be the new base: checkpoint the current
  // base + its mask for undo, adopt the result as the base, then start a fresh
  // mask on top. Ownership of the pending preview URL transfers to the prepared
  // override (so it is NOT revoked here).
  applyInpaintResult() {
    if (!this.pendingResultPreviewUrl || this.pendingResultWidth == null || this.pendingResultHeight == null) {
      return;
    }

    const outgoingMask = this.snapshotInpaintMask();
    this.inpaintBaseHistory = [
      ...this.inpaintBaseHistory,
      {
        previewUrl: this.preparedInpaintPreviewUrl ?? this.referenceImageUrl,
        uploadedInputName: generation.inputImage,
        width: generation.width,
        height: generation.height,
        maskSnapshotUrl: outgoingMask,
        // Only a prepared preview is an owned object URL; the session-original
        // referenceImageUrl is owned elsewhere and must not be revoked here.
        owned: this.preparedInpaintPreviewUrl ? this.preparedInpaintOwned : false,
      },
    ];

    // Transfer the pending result into the prepared override. Do NOT revoke the
    // pending URL: the prepared override now owns it. Do NOT revoke the outgoing
    // prepared URL either: the history entry above now owns it.
    this.preparedInpaintPreviewUrl = this.pendingResultPreviewUrl;
    this.preparedInpaintOwned = this.pendingResultOwned;
    generation.inputImage = this.pendingResultInputName;
    generation.width = this.pendingResultWidth;
    generation.height = this.pendingResultHeight;

    // Clear pending WITHOUT revoking (ownership was transferred above).
    this.pendingResultPreviewUrl = null;
    this.pendingResultOwned = false;
    this.pendingResultInputName = null;
    this.pendingResultWidth = null;
    this.pendingResultHeight = null;

    // Fresh mask on the new base.
    this.persistedMaskPreviewUrl = null;
    this.clearMask();
    this.initCanvas(generation.width, generation.height);
  }

  get canApplyInpaintResult(): boolean {
    return generation.mode === "inpainting" && this.pendingResultPreviewUrl !== null;
  }

  // Called by the canvas when the user paints/edits the mask. While a pending
  // inpaint result is on screen the mask is hidden; this brings it back so the
  // user can see the region they're adding to.
  markMaskEdited() {
    if (this.pendingResultPreviewUrl && !this.maskEditedSinceResult) {
      this.maskEditedSinceResult = true;
    }
  }

  // Whether the editable inpaint mask strokes should be hidden on the canvas. True
  // only while a finished result is being previewed and the user is not painting
  // more mask. The caller additionally keeps the mask visible during a re-roll
  // (via progress.isGenerating), which the store deliberately does not know about.
  get shouldHideInpaintMask(): boolean {
    return (
      generation.mode === "inpainting" &&
      this.pendingResultPreviewUrl !== null &&
      !this.maskEditedSinceResult
    );
  }

  restoreOriginalInpaintSource() {
    if (!this.originalInpaintInputImageName || this.originalInpaintWidth == null || this.originalInpaintHeight == null) {
      return;
    }
    this.clearPreparedInpaintOverride();
    this.clearPendingInpaintResult();
    this.clearInpaintBaseHistory();
    generation.inputImage = this.originalInpaintInputImageName;
    generation.width = this.originalInpaintWidth;
    generation.height = this.originalInpaintHeight;
    this.clearMask();
    this.initCanvas(this.originalInpaintWidth, this.originalInpaintHeight);
  }

  clearInpaintSession() {
    this.clearMask();
    this.clearPreparedInpaintOverride();
    this.clearPendingInpaintResult();
    this.clearInpaintBaseHistory();
    this.inpaintSourceVersion += 1;
    this.originalInpaintInputImageName = null;
    this.originalInpaintWidth = null;
    this.originalInpaintHeight = null;
    this.referenceImageUrl = null;
  }

  private clearInpaintBaseHistory() {
    for (const entry of this.inpaintBaseHistory) {
      if (entry.owned && entry.previewUrl) URL.revokeObjectURL(entry.previewUrl);
    }
    this.inpaintBaseHistory = [];
  }

  // Step the inpaint base back to the previous image, re-applying the mask that
  // was on it. The current (discarded) prepared preview is revoked.
  undoInpaintBase() {
    if (!this.inpaintBaseHistory.length) return;

    // Stepping back discards any un-applied result being previewed.
    this.clearPendingInpaintResult();

    const entry = this.inpaintBaseHistory[this.inpaintBaseHistory.length - 1];
    this.inpaintBaseHistory = this.inpaintBaseHistory.slice(0, -1);

    // Discard the base we're leaving (the current prepared override, if any).
    if (this.preparedInpaintOwned && this.preparedInpaintPreviewUrl) {
      URL.revokeObjectURL(this.preparedInpaintPreviewUrl);
    }

    if (entry.previewUrl && entry.previewUrl === this.referenceImageUrl) {
      // Stepping back to the session original: no prepared override.
      this.preparedInpaintPreviewUrl = null;
      this.preparedInpaintOwned = false;
    } else {
      this.preparedInpaintPreviewUrl = entry.previewUrl;
      this.preparedInpaintOwned = entry.owned;
    }

    generation.inputImage = entry.uploadedInputName;
    generation.width = entry.width;
    generation.height = entry.height;

    this.persistedMaskPreviewUrl = null;
    this.initCanvas(entry.width, entry.height);
    this.pendingMaskRestoreUrl = entry.maskSnapshotUrl;
  }

  get canUndoInpaintBase(): boolean {
    return generation.mode === "inpainting" && this.inpaintBaseHistory.length > 0;
  }

  get currentPreparedInputImage(): string | null {
    if (generation.mode === "inpainting") {
      return this.preparedInpaintPreviewUrl;
    }
    return this.currentStagingImage;
  }

  get hasResettableInpaintSource(): boolean {
    return generation.mode === "inpainting" && !!this.referenceImageUrl && !!this.originalInpaintInputImageName;
  }

  get resettableInpaintPreviewImage(): string | null {
    if (generation.mode === "inpainting") {
      return this.preparedInpaintPreviewUrl ?? this.referenceImageUrl;
    }
    return this.currentStagingImage;
  }

  clearPreparedInputs() {
    if (generation.mode === "inpainting" && this.hasResettableInpaintSource) {
      this.restoreOriginalInpaintSource();
      return;
    }
    this.clearStaging();
  }

  dismissPreparedInput() {
    if (generation.mode === "inpainting" && this.currentPreparedInputImage) {
      this.restoreOriginalInpaintSource();
      return;
    }
    this.dismissCurrentStaging();
  }

  stageImage(url: string, options?: { owned?: boolean }) {
    if (!url) return;
    this.stagingImages = [
      ...this.stagingImages,
      {
        url,
        owned: options?.owned ?? false,
      },
    ];
    this.stagingIndex = this.stagingImages.length - 1;
    this.isStagingActive = this.stagingImages.length > 0;
  }

  stageBlob(blob: Blob) {
    this.stageImage(URL.createObjectURL(blob), { owned: true });
  }

  clearStaging() {
    for (const entry of this.stagingImages) {
      if (entry.owned) URL.revokeObjectURL(entry.url);
    }
    this.stagingImages = [];
    this.stagingIndex = 0;
    this.isStagingActive = false;
  }

  nextStaging() {
    if (!this.stagingImages.length) return;
    this.stagingIndex = (this.stagingIndex + 1) % this.stagingImages.length;
  }

  prevStaging() {
    if (!this.stagingImages.length) return;
    this.stagingIndex = (this.stagingIndex - 1 + this.stagingImages.length) % this.stagingImages.length;
  }

  dismissCurrentStaging() {
    if (!this.stagingImages.length) return;
    const current = this.stagingImages[this.stagingIndex];
    if (current?.owned) URL.revokeObjectURL(current.url);
    this.stagingImages = this.stagingImages.filter((_, index) => index !== this.stagingIndex);

    if (!this.stagingImages.length) {
      this.stagingIndex = 0;
      this.isStagingActive = false;
      return;
    }

    if (this.stagingIndex >= this.stagingImages.length) {
      this.stagingIndex = this.stagingImages.length - 1;
    }
    this.isStagingActive = true;
  }

  get currentStagingImage(): string | null {
    if (!this.stagingImages.length) return null;
    return this.stagingImages[this.stagingIndex]?.url ?? null;
  }

  get effectiveReferenceImage(): string | null {
    if (generation.mode === "inpainting") {
      // A just-generated result is previewed as the background; below it the base
      // override (or session original) shows through until the user applies/undoes.
      if (this.pendingResultPreviewUrl) return this.pendingResultPreviewUrl;
      if (this.preparedInpaintPreviewUrl) return this.preparedInpaintPreviewUrl;
    }
    return this.currentStagingImage ?? this.referenceImageUrl;
  }

  setReferenceImage(url: string | null) {
    this.referenceImageUrl = url;
  }

  setPersistedMaskPreview(url: string | null) {
    this.persistedMaskPreviewUrl = url;
  }

  clearMask() {
    generation.maskImage = null;
    this.persistedMaskPreviewUrl = null;

    if (!this._stageRef) return;
    const stageLayers = this._stageRef.getLayers?.() ?? [];
    for (const layerMeta of this.layers.filter((l) => l.type === "mask")) {
      const layer = stageLayers.find((l: any) => l.id?.() === layerMeta.id);
      layer?.destroyChildren?.();
      layer?.batchDraw?.();
    }
  }

  // Composite the editable inpaint mask layer(s) into a tinted, transparent-bg
  // data URL so the mask survives a base swap (layers are rebuilt on swap).
  // Returns null when there is no mask layer or the mask is empty.
  snapshotInpaintMask(): string | null {
    const stage = this._stageRef;
    if (!stage) return null;

    const maskMetas = this.layers.filter((l) => l.type === "mask");
    if (!maskMetas.length) return null;

    const stageLayers = stage.getLayers?.() ?? [];
    const offscreen = document.createElement("canvas");
    offscreen.width = this.canvasWidth;
    offscreen.height = this.canvasHeight;
    const ctx = offscreen.getContext("2d");
    if (!ctx) return null;

    let drew = false;
    for (const meta of maskMetas) {
      const layer = stageLayers.find((l: any) => l.id?.() === meta.id);
      if (!layer) continue;

      // The viewport is applied as a layer transform; reset it so the snapshot
      // captures canvas-space pixels, then restore it.
      const origScale = layer.scaleX();
      const origX = layer.x();
      const origY = layer.y();
      layer.scaleX(1);
      layer.scaleY(1);
      layer.x(0);
      layer.y(0);
      try {
        const layerCanvas = layer.toCanvas({
          pixelRatio: 1,
          width: this.canvasWidth,
          height: this.canvasHeight,
        });
        ctx.drawImage(layerCanvas, 0, 0);
        drew = true;
      } catch (error) {
        console.error("Failed to snapshot inpaint mask:", error);
      } finally {
        layer.scaleX(origScale);
        layer.scaleY(origScale);
        layer.x(origX);
        layer.y(origY);
      }
    }

    if (!drew) return null;

    // Skip blank masks so undo never restores an empty mask.
    const data = ctx.getImageData(0, 0, offscreen.width, offscreen.height).data;
    let hasPixels = false;
    for (let i = 3; i < data.length; i += 4) {
      if (data[i] > 0) {
        hasPixels = true;
        break;
      }
    }
    if (!hasPixels) return null;

    return offscreen.toDataURL("image/png");
  }

  setInpaintDrawMode(mode: "mask" | "regular") {
    this.inpaintDrawMode = mode;
  }

  sendActiveLayerToMask(): boolean {
    if (!this._stageRef || !this.activeLayerId) return false;

    const sourceLayerMeta = this.layers.find((l) => l.id === this.activeLayerId);
    if (!sourceLayerMeta) return false;

    let maskLayerMeta = this.layers.find((l) => l.type === "mask");
    if (!maskLayerMeta) {
      const newId = this.addLayer("mask", locale.t("canvas.layer.inpaint_mask"));
      maskLayerMeta = this.layers.find((l) => l.id === newId) ?? undefined;
    }
    if (!maskLayerMeta) return false;

    if (sourceLayerMeta.id === maskLayerMeta.id) {
      this.activeLayerId = maskLayerMeta.id;
      return true;
    }

    const stageLayers = this._stageRef.getLayers?.() ?? [];
    const sourceLayer = stageLayers.find((layer: any) => layer.id?.() === sourceLayerMeta.id);
    const maskLayer = stageLayers.find((layer: any) => layer.id?.() === maskLayerMeta.id);
    if (!sourceLayer || !maskLayer) return false;

    const sourceNodes = sourceLayer.getChildren?.() ?? [];
    if (!sourceNodes.length) return false;

    for (const node of sourceNodes) {
      const gco = node.globalCompositeOperation?.();
      if (gco === "destination-out") continue;

      const clone = node.clone?.();
      if (!clone) continue;

      clone.globalCompositeOperation?.("source-over");
      clone.opacity?.(1);

      if (clone.stroke && typeof clone.stroke === "function") {
        clone.stroke(this.maskOverlayColor);
      }
      if (clone.fill && typeof clone.fill === "function") {
        clone.fill(this.maskOverlayColor);
      }

      maskLayer.add(clone);
    }

    sourceLayer.destroyChildren?.();
    sourceLayer.batchDraw?.();
    maskLayer.batchDraw?.();

    this.activeLayerId = maskLayerMeta.id;
    return true;
  }

  // Brush
  adjustBrushSize(delta: number) {
    this.brushSettings = {
      ...this.brushSettings,
      size: Math.max(1, Math.min(500, this.brushSettings.size + delta)),
    };
  }

  // Layers
  addLayer(type: "raster" | "mask" = "raster", name?: string): string {
    const id = genLayerId();
    const maxOrder = this.layers.reduce((max, l) => Math.max(max, l.order), -1);
    const layerName = name ?? (type === "mask"
      ? locale.t("canvas.layer.inpaint_mask")
      : locale.t("canvas.layer.raster", { n: String(this.layers.filter((l) => l.type === "raster").length + 1) }));

    this.layers = [
      ...this.layers,
      {
        id,
        name: layerName,
        type,
        visible: true,
        opacity: 1,
        locked: false,
        order: maxOrder + 1,
      },
    ];
    this.activeLayerId = id;
    return id;
  }

  removeLayer(id: string) {
    const removed = this.layers.find((l) => l.id === id);
    this.layers = this.layers.filter((l) => l.id !== id);
    this.clearLayerThumbnail(id);
    if (this.activeLayerId === id) {
      if (this.layers.length === 0) {
        this.activeLayerId = null;
      } else if (removed) {
        // Select the surviving layer whose order is nearest to the removed one.
        const nearest = this.layers.reduce((best, l) =>
          Math.abs(l.order - removed.order) < Math.abs(best.order - removed.order) ? l : best
        );
        this.activeLayerId = nearest.id;
      } else {
        this.activeLayerId = this.layers[this.layers.length - 1].id;
      }
    }
  }

  setLayerThumbnail(id: string, dataUrl: string) {
    this.layerThumbnails = { ...this.layerThumbnails, [id]: dataUrl };
  }

  clearLayerThumbnail(id: string) {
    if (!(id in this.layerThumbnails)) return;
    const next = { ...this.layerThumbnails };
    delete next[id];
    this.layerThumbnails = next;
  }

  duplicateLayer(id: string): string | null {
    const layer = this.layers.find((l) => l.id === id);
    if (!layer) return null;
    const newId = genLayerId();
    const maxOrder = this.layers.reduce((max, l) => Math.max(max, l.order), -1);
    this.layers = [
      ...this.layers,
      {
        ...layer,
        id: newId,
        name: `${layer.name} copy`,
        order: maxOrder + 1,
      },
    ];
    this.activeLayerId = newId;
    return newId;
  }

  reorderLayer(id: string, direction: "up" | "down") {
    const sorted = [...this.layers].sort((a, b) => a.order - b.order);
    const idx = sorted.findIndex((l) => l.id === id);
    if (idx < 0) return;

    const swapIdx = direction === "up" ? idx + 1 : idx - 1;
    if (swapIdx < 0 || swapIdx >= sorted.length) return;

    const tmpOrder = sorted[idx].order;
    sorted[idx].order = sorted[swapIdx].order;
    sorted[swapIdx].order = tmpOrder;

    this.layers = [...sorted];
  }

  renameLayer(id: string, name: string) {
    this.layers = this.layers.map((l) => (l.id === id ? { ...l, name } : l));
  }

  toggleLayerVisibility(id: string) {
    this.layers = this.layers.map((l) => (l.id === id ? { ...l, visible: !l.visible } : l));
  }

  setLayerOpacity(id: string, opacity: number) {
    this.layers = this.layers.map((l) => (l.id === id ? { ...l, opacity } : l));
  }

  toggleLayerLock(id: string) {
    this.layers = this.layers.map((l) => (l.id === id ? { ...l, locked: !l.locked } : l));
  }

  setActiveLayer(id: string) {
    this.activeLayerId = id;
  }

  // Clear all content from a layer (via Konva stage ref)
  clearLayer(id: string) {
    if (!this._stageRef) return;
    const layers = this._stageRef.getLayers();
    for (const kLayer of layers) {
      if (kLayer.id() === id) {
        kLayer.destroyChildren();
        kLayer.batchDraw();
        break;
      }
    }
  }

  // Viewport
  zoomIn() {
    this.setZoom(Math.min(20, this.viewport.zoom * 1.2));
  }

  zoomOut() {
    this.setZoom(Math.max(0.05, this.viewport.zoom / 1.2));
  }

  setZoom(zoom: number, centerX?: number, centerY?: number) {
    const oldZoom = this.viewport.zoom;
    const newZoom = Math.max(0.05, Math.min(20, zoom));

    if (centerX !== undefined && centerY !== undefined) {
      // Zoom toward the cursor position
      const scale = newZoom / oldZoom;
      this.viewport = {
        zoom: newZoom,
        panX: centerX - (centerX - this.viewport.panX) * scale,
        panY: centerY - (centerY - this.viewport.panY) * scale,
      };
    } else {
      this.viewport = { ...this.viewport, zoom: newZoom };
    }
  }

  zoomToFit(containerWidth: number, containerHeight: number) {
    const scaleX = containerWidth / this.canvasWidth;
    const scaleY = containerHeight / this.canvasHeight;
    const zoom = Math.min(scaleX, scaleY) * 0.9;
    this.viewport = {
      zoom,
      panX: (containerWidth - this.canvasWidth * zoom) / 2,
      panY: (containerHeight - this.canvasHeight * zoom) / 2,
    };
  }

  resetZoom() {
    this.viewport = { zoom: 1, panX: 0, panY: 0 };
  }

  // Canvas init — creates default layers
  initCanvas(width: number, height: number) {
    this.canvasWidth = width;
    this.canvasHeight = height;
    this.layers = [];
    this.activeLayerId = null;
    this.layerThumbnails = {};

    this.addLayer("raster", locale.t("canvas.layer.background"));
    this.addLayer("mask", locale.t("canvas.layer.inpaint_mask"));

    // Set active to the raster layer
    const rasterLayer = this.layers.find((l) => l.type === "raster");
    if (rasterLayer) this.activeLayerId = rasterLayer.id;

    this.boundingBox = { x: 0, y: 0, width, height, locked: false };
  }

  // Export
  async exportLayerAsImage(layerCanvas: HTMLCanvasElement, filename: string): Promise<{ name: string; subfolder: string; type: string }> {
    const blob = await new Promise<Blob>((resolve) => {
      layerCanvas.toBlob((b) => resolve(b!), "image/png");
    });
    const arrayBuffer = await blob.arrayBuffer();
    const bytes = Array.from(new Uint8Array(arrayBuffer));
    return uploadImageBytes(bytes, filename);
  }

  async syncMaskToGeneration(maskCanvas: HTMLCanvasElement | null, uploadToComfy: boolean = true): Promise<boolean> {
    if (!maskCanvas) {
      generation.maskImage = null;
      this.persistedMaskPreviewUrl = null;
      return false;
    }

    const ctx = maskCanvas.getContext("2d")!;
    const data = ctx.getImageData(0, 0, maskCanvas.width, maskCanvas.height).data;
    let hasMask = false;
    for (let i = 3; i < data.length; i += 4) {
      if (data[i] > 0) {
        hasMask = true;
        break;
      }
    }

    if (!hasMask) {
      generation.maskImage = null;
      this.persistedMaskPreviewUrl = null;
      return false;
    }

    // Convert mask to white-on-black for ComfyUI.
    const exportCanvas = document.createElement("canvas");
    exportCanvas.width = maskCanvas.width;
    exportCanvas.height = maskCanvas.height;
    const exportCtx = exportCanvas.getContext("2d")!;
    exportCtx.fillStyle = "black";
    exportCtx.fillRect(0, 0, exportCanvas.width, exportCanvas.height);
    exportCtx.drawImage(maskCanvas, 0, 0);

    const imgData = exportCtx.getImageData(0, 0, exportCanvas.width, exportCanvas.height);
    const pixels = imgData.data;
    for (let i = 0; i < pixels.length; i += 4) {
      if (pixels[i + 3] > 0 && (pixels[i] > 64 || pixels[i + 1] > 64 || pixels[i + 2] > 64)) {
        pixels[i] = pixels[i + 1] = pixels[i + 2] = 255;
        pixels[i + 3] = 255;
      } else {
        pixels[i] = pixels[i + 1] = pixels[i + 2] = 0;
        pixels[i + 3] = 255;
      }
    }
    exportCtx.putImageData(imgData, 0, 0);

    // Persist/update uploaded mask preview only when we actually sync to ComfyUI.
    if (uploadToComfy) {
      this.persistedMaskPreviewUrl = exportCanvas.toDataURL("image/png");
      const result = await this.exportLayerAsImage(exportCanvas, "canvas_mask.png");
      generation.maskImage = result.name;
    }

    return true;
  }

  // Sync canvas to generation store before generating
  async syncToGeneration(
    getRasterComposite: () => HTMLCanvasElement | null,
    getMaskCanvas: () => HTMLCanvasElement | null
  ) {
    const rasterCanvas = getRasterComposite();
    const maskCanvas = getMaskCanvas();
    const isInpainting = generation.mode === "inpainting";

    let hasRaster = false;
    let hasMask = false;

    // In inpainting mode, keep the currently selected input image as the baseline.
    // This makes denoise behave as expected: only the masked area is reworked.
    if (isInpainting) {
      if (generation.inputImage) {
        hasRaster = true;
      } else if (rasterCanvas) {
        // Last resort only: if no source image exists, fall back to painted raster.
        const ctx = rasterCanvas.getContext("2d")!;
        const data = ctx.getImageData(0, 0, rasterCanvas.width, rasterCanvas.height).data;
        for (let i = 3; i < data.length; i += 4) {
          if (data[i] > 0) { hasRaster = true; break; }
        }
        if (hasRaster) {
          const result = await this.exportLayerAsImage(rasterCanvas, "canvas_input.png");
          generation.inputImage = result.name;
        }
      }
    } else {
      // Non-inpaint modes use raster if present, otherwise staged image fallback.
      if (rasterCanvas) {
        const ctx = rasterCanvas.getContext("2d")!;
        const data = ctx.getImageData(0, 0, rasterCanvas.width, rasterCanvas.height).data;
        // Check if any pixel has non-zero alpha
        for (let i = 3; i < data.length; i += 4) {
          if (data[i] > 0) { hasRaster = true; break; }
        }
        if (hasRaster) {
          const result = await this.exportLayerAsImage(rasterCanvas, "canvas_input.png");
          generation.inputImage = result.name;
        }
      }

      if (!hasRaster && this.currentStagingImage) {
        const response = await fetch(this.currentStagingImage);
        const blob = await response.blob();
        const arrayBuffer = await blob.arrayBuffer();
        const bytes = Array.from(new Uint8Array(arrayBuffer));
        const result = await uploadImageBytes(bytes, "staged_input.png");
        generation.inputImage = result.name;
        hasRaster = true;
      }
    }

    // Export mask
    hasMask = await this.syncMaskToGeneration(maskCanvas, true);

    // Keep the user-selected mode when using canvas flow for image editing modes.
    if (generation.mode === "inpainting") {
      generation.mode = "inpainting";
    } else if (generation.mode === "img2img") {
      generation.mode = "img2img";
    } else if (hasRaster && hasMask) {
      generation.mode = "inpainting";
    } else if (hasRaster) {
      generation.mode = "img2img";
    } else {
      generation.mode = "txt2img";
    }

    // Sync dimensions from bounding box
    generation.width = this.boundingBox.width;
    generation.height = this.boundingBox.height;
  }
}

export const canvas = new CanvasStore();
