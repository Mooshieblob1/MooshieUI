import { uploadImageBytes } from "../utils/api.js";
import { generation } from "./generation.svelte.js";

export type ToolType = "brush" | "eraser" | "rectFill" | "eyedropper" | "move" | "view" | "transform";

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
  persistedMaskPreviewUrl = $state<string | null>(null);

  // Staging
  stagingImages = $state<string[]>([]);
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

  stageImage(url: string) {
    if (!url) return;
    this.stagingImages = [...this.stagingImages, url];
    this.stagingIndex = this.stagingImages.length - 1;
    this.isStagingActive = this.stagingImages.length > 0;
  }

  clearStaging() {
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
    return this.stagingImages[this.stagingIndex] ?? null;
  }

  get effectiveReferenceImage(): string | null {
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

  setInpaintDrawMode(mode: "mask" | "regular") {
    this.inpaintDrawMode = mode;
  }

  sendActiveLayerToMask(): boolean {
    if (!this._stageRef || !this.activeLayerId) return false;

    const sourceLayerMeta = this.layers.find((l) => l.id === this.activeLayerId);
    if (!sourceLayerMeta) return false;

    let maskLayerMeta = this.layers.find((l) => l.type === "mask");
    if (!maskLayerMeta) {
      const newId = this.addLayer("mask", "Inpaint Mask");
      maskLayerMeta = this.layers.find((l) => l.id === newId) ?? null;
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
    const layerName = name ?? (type === "mask" ? "Inpaint Mask" : `Layer ${this.layers.filter((l) => l.type === "raster").length + 1}`);

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
    this.layers = this.layers.filter((l) => l.id !== id);
    if (this.activeLayerId === id) {
      this.activeLayerId = this.layers.length > 0 ? this.layers[this.layers.length - 1].id : null;
    }
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

    this.addLayer("raster", "Background");
    this.addLayer("mask", "Inpaint Mask");

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

  async syncMaskToGeneration(maskCanvas: HTMLCanvasElement | null): Promise<boolean> {
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

    // Persist a preview source for mask visualization across remounts.
    this.persistedMaskPreviewUrl = exportCanvas.toDataURL("image/png");

    const result = await this.exportLayerAsImage(exportCanvas, "canvas_mask.png");
    generation.maskImage = result.name;
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

    // In inpainting mode, keep the original/staged image as the baseline input.
    // This makes denoise behave as expected: only the masked area is reworked.
    if (isInpainting) {
      if (this.currentStagingImage) {
        const response = await fetch(this.currentStagingImage);
        const blob = await response.blob();
        const arrayBuffer = await blob.arrayBuffer();
        const bytes = Array.from(new Uint8Array(arrayBuffer));
        const result = await uploadImageBytes(bytes, "staged_input.png");
        generation.inputImage = result.name;
        hasRaster = true;
      } else if (generation.inputImage) {
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
    hasMask = await this.syncMaskToGeneration(maskCanvas);

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
