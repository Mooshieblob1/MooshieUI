<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import Konva from "konva";
  import { generation } from "../../stores/generation.svelte.js";
  import { canvas, type ToolType } from "../../stores/canvas.svelte.js";
  import { canvasHistory } from "../../stores/canvasHistory.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import ColorTooltip from "../ui/ColorTooltip.svelte";

  // Hide the editable inpaint mask while a finished result is being previewed, so
  // the clean output is visible. Keep it shown before any result, during a re-roll
  // (progress.isGenerating), and once the user paints more mask (markMaskEdited).
  const hideInpaintMask = $derived(canvas.shouldHideInpaintMask && !progress.isGenerating);

  let containerEl: HTMLDivElement | undefined = $state();
  let stage: Konva.Stage | null = null;

  // Tooltip state
  let tooltipVisible = $state(false);
  let tooltipColor = $state("#000000");
  let tooltipPos = $state({ x: 0, y: 0 });
  let tooltipRaf: number | null = null;

  // Konva layers keyed by canvas layer ID
  let konvaLayers = new Map<string, Konva.Layer>();
  // Background layer (checkerboard)
  let bgLayer: Konva.Layer | null = null;
  let checkerRect: Konva.Rect | null = null;
  let borderRect: Konva.Rect | null = null;
  let checkerPatternCanvas: HTMLCanvasElement | null = null;
  // Reference image layer (under paint layers)
  let refLayer: Konva.Layer | null = null;
  let refImageNode: Konva.Image | null = null;
  let lastRefSource: string | null = null;
  // Persisted mask preview layer (shows last exported/uploaded mask after remount)
  let persistedMaskLayer: Konva.Layer | null = null;
  let persistedMaskNode: Konva.Image | null = null;
  let lastMaskSource: string | null = null;
  // UI overlay layer (brush cursor, bounding box)
  let uiLayer: Konva.Layer | null = null;

  // Drawing state (not reactive — performance-critical)
  let isDrawing = false;
  let isPanning = false;
  let isSpacePanning = false;
  let currentLine: Konva.Line | null = null;
  let activeStrokeTool: "brush" | "eraser" | null = null;
  let lastPointerPos: { x: number; y: number } | null = null;
  let brushCursor: Konva.Circle | null = null;

  // Rectangle tool state
  let isDrawingRect = false;
  let rectStartPos: { x: number; y: number } | null = null;
  let rectPreview: Konva.Rect | null = null;
  let isMovingLayer = false;
  let moveStartPos: { x: number; y: number } | null = null;
  let moveNodeStarts: Array<{ node: Konva.Node; x: number; y: number }> = [];
  let viewportRaf: number | null = null;

  // Lasso tool state (points kept in canvas space; preview drawn on the UI layer in screen space)
  let isLasso = false;
  let lassoPoints: number[] = [];
  let lassoPreviewLine: Konva.Line | null = null;

  // Alt-held quick eyedropper
  let isAltEyedropper = false;

  // Per-layer thumbnail regeneration (RAF-deduped per layer id)
  let thumbRafs = new Map<string, number>();
  let thumbInitialized = new Set<string>();

  // Container size
  let containerW = 0;
  let containerH = 0;

  onMount(() => {
    if (!containerEl) return;
    initStage();
    const observer = new ResizeObserver(handleResize);
    observer.observe(containerEl);
    return () => observer.disconnect();
  });

  onDestroy(() => {
    if (tooltipRaf !== null) {
      cancelAnimationFrame(tooltipRaf);
    }
    canvas.isPointerOverStage = false;
    if (viewportRaf !== null) {
      cancelAnimationFrame(viewportRaf);
      viewportRaf = null;
    }
    for (const raf of thumbRafs.values()) {
      cancelAnimationFrame(raf);
    }
    thumbRafs.clear();
    // Drop the restore callback so a stale closure can't fire after unmount.
    canvasHistory.setOnRestored(null);
    if (stage) {
      stage.destroy();
      stage = null;
    }
  });

  function scheduleViewportApply() {
    if (viewportRaf !== null) return;
    viewportRaf = requestAnimationFrame(() => {
      viewportRaf = null;
      applyViewport();
    });
  }

  function updateTooltip(e: Konva.KonvaEventObject<MouseEvent | TouchEvent>) {
    tooltipRaf = null;
    if (!stage || isDrawing || isPanning || isDrawingRect || isMovingLayer) {
      tooltipVisible = false;
      return;
    }
    
    const pointerPos = stage.getPointerPosition();
    if (!pointerPos) {
      tooltipVisible = false;
      return;
    }
    
    // Sample color from all layers
    const compositeCanvas = stage.toCanvas({ pixelRatio: 1 });
    const ctx = compositeCanvas.getContext("2d")!;
    const pixel = ctx.getImageData(Math.round(pointerPos.x), Math.round(pointerPos.y), 1, 1).data;

    if (pixel[3] > 0) {
      const hex = `#${pixel[0].toString(16).padStart(2, "0")}${pixel[1].toString(16).padStart(2, "0")}${pixel[2].toString(16).padStart(2, "0")}`;
      tooltipColor = hex;
      tooltipPos = { x: pointerPos.x + 15, y: pointerPos.y + 15 };
      tooltipVisible = true;
    } else {
      tooltipVisible = false;
    }
  }

  function initStage() {
    if (!containerEl) return;

    const rect = containerEl.getBoundingClientRect();
    containerW = rect.width;
    containerH = rect.height;

    stage = new Konva.Stage({
      container: containerEl,
      width: containerW,
      height: containerH,
    });

    canvas.setStageRef(stage);

    // Background layer (checkerboard)
    bgLayer = new Konva.Layer({ listening: false });
    stage.add(bgLayer);
    drawCheckerboard();

    // Reference image layer (input/staged image underlay)
    refLayer = new Konva.Layer({ listening: false });
    stage.add(refLayer);
    updateReferenceImage(canvas.effectiveReferenceImage);

    // Persisted mask preview layer (sits above reference, below paint layers)
    persistedMaskLayer = new Konva.Layer({ listening: false });
    stage.add(persistedMaskLayer);
    updatePersistedMaskOverlay(canvas.persistedMaskPreviewUrl);

    // Create Konva layers for each canvas layer
    syncKonvaLayers();

    // UI overlay layer
    uiLayer = new Konva.Layer({ listening: false });
    stage.add(uiLayer);

    // Brush cursor
    brushCursor = new Konva.Circle({
      radius: canvas.brushSettings.size / 2,
      stroke: "#ffffff",
      strokeWidth: 1.5,
      dash: [4, 4],
      visible: false,
      listening: false,
    });
    uiLayer.add(brushCursor);

    // Set history refs
    canvasHistory.setRefs(konvaLayers, canvas.canvasWidth, canvas.canvasHeight);
    canvasHistory.setOnRestored((layerIds) => {
      for (const id of layerIds) scheduleThumbRefresh(id);
    });

    // Apply initial viewport
    applyViewport();

    // Fit canvas to view
    canvas.zoomToFit(containerW, containerH);
    applyViewport();

    // Event handlers
    stage.on("mousedown touchstart", handlePointerDown);
    stage.on("mousemove touchmove", handlePointerMove);
    stage.on("mouseup touchend", handlePointerUp);
    stage.on("mouseenter", handlePointerEnter);
    stage.on("mouseleave", handlePointerLeave);
    stage.on("wheel", handleWheel);
    stage.on("contextmenu", (e) => e.evt.preventDefault());

    reorderStageLayers();
  }

  function reorderStageLayers() {
    if (!stage) return;

    bgLayer?.moveToBottom();
    if (refLayer) {
      refLayer.moveToBottom();
      refLayer.moveUp();
    }
    if (persistedMaskLayer) {
      persistedMaskLayer.moveToBottom();
      persistedMaskLayer.moveUp();
      persistedMaskLayer.moveUp();
    }

    const sorted = [...canvas.layers].sort((a, b) => a.order - b.order);
    for (const layer of sorted) {
      const kLayer = konvaLayers.get(layer.id);
      if (kLayer) kLayer.moveToTop();
    }

    uiLayer?.moveToTop();
  }

  function updateReferenceImage(url: string | null) {
    if (!refLayer) return;

    if (!url) {
      lastRefSource = null;
      if (refImageNode) {
        refImageNode.destroy();
        refImageNode = null;
      }
      refLayer.batchDraw();
      return;
    }

    lastRefSource = url;
    const img = new Image();
    img.onload = () => {
      if (!refLayer || lastRefSource !== url) return;

      const imageRatio = img.naturalWidth / img.naturalHeight;
      const canvasRatio = canvas.canvasWidth / canvas.canvasHeight;

      let drawWidth = canvas.canvasWidth;
      let drawHeight = canvas.canvasHeight;
      if (imageRatio > canvasRatio) {
        drawHeight = canvas.canvasWidth / imageRatio;
      } else {
        drawWidth = canvas.canvasHeight * imageRatio;
      }

      const offsetX = (canvas.canvasWidth - drawWidth) / 2;
      const offsetY = (canvas.canvasHeight - drawHeight) / 2;

      if (!refImageNode) {
        refImageNode = new Konva.Image({
          image: img,
          x: offsetX,
          y: offsetY,
          width: drawWidth,
          height: drawHeight,
          listening: false,
          opacity: 0.95,
        });
        refLayer.add(refImageNode);
      } else {
        refImageNode.image(img);
        refImageNode.x(offsetX);
        refImageNode.y(offsetY);
        refImageNode.width(drawWidth);
        refImageNode.height(drawHeight);
      }

      reorderStageLayers();
      refLayer.batchDraw();
    };
    img.onerror = () => {
      if (!refLayer || lastRefSource !== url) return;
      if (refImageNode) {
        refImageNode.destroy();
        refImageNode = null;
        refLayer.batchDraw();
      }
    };
    img.src = url;
  }

  function parseHexColor(hex: string): { r: number; g: number; b: number } {
    const clean = hex.replace("#", "");
    const value = clean.length === 3
      ? clean.split("").map((ch) => ch + ch).join("")
      : clean;
    const num = Number.parseInt(value, 16);
    if (!Number.isFinite(num)) return { r: 255, g: 51, b: 51 };
    return {
      r: (num >> 16) & 255,
      g: (num >> 8) & 255,
      b: num & 255,
    };
  }

  function updatePersistedMaskOverlay(url: string | null) {
    if (!persistedMaskLayer) return;

    if (!url || !canvas.maskOverlayVisible) {
      lastMaskSource = null;
      if (persistedMaskNode) {
        persistedMaskNode.destroy();
        persistedMaskNode = null;
      }
      persistedMaskLayer.batchDraw();
      return;
    }

    lastMaskSource = url;
    const img = new Image();
    img.onload = () => {
      if (!persistedMaskLayer || lastMaskSource !== url) return;

      const overlayCanvas = document.createElement("canvas");
      overlayCanvas.width = img.naturalWidth;
      overlayCanvas.height = img.naturalHeight;
      const ctx = overlayCanvas.getContext("2d");
      if (!ctx) return;

      ctx.drawImage(img, 0, 0);
      const data = ctx.getImageData(0, 0, overlayCanvas.width, overlayCanvas.height);
      const pixels = data.data;
      const color = parseHexColor(canvas.maskOverlayColor);
      const baseAlpha = Math.max(0, Math.min(1, canvas.maskOverlayOpacity));

      for (let i = 0; i < pixels.length; i += 4) {
        const maskValue = pixels[i];
        if (maskValue > 8) {
          pixels[i] = color.r;
          pixels[i + 1] = color.g;
          pixels[i + 2] = color.b;
          pixels[i + 3] = Math.round(maskValue * baseAlpha);
        } else {
          pixels[i + 3] = 0;
        }
      }
      ctx.putImageData(data, 0, 0);

      const overlayImg = new Image();
      overlayImg.onload = () => {
        if (!persistedMaskLayer || lastMaskSource !== url) return;

        if (!persistedMaskNode) {
          persistedMaskNode = new Konva.Image({
            image: overlayImg,
            x: 0,
            y: 0,
            width: canvas.canvasWidth,
            height: canvas.canvasHeight,
            listening: false,
          });
          persistedMaskLayer.add(persistedMaskNode);
        } else {
          persistedMaskNode.image(overlayImg);
          persistedMaskNode.x(0);
          persistedMaskNode.y(0);
          persistedMaskNode.width(canvas.canvasWidth);
          persistedMaskNode.height(canvas.canvasHeight);
        }

        reorderStageLayers();
        persistedMaskLayer.batchDraw();
      };
      overlayImg.src = overlayCanvas.toDataURL("image/png");
    };
    img.onerror = () => {
      if (!persistedMaskLayer || lastMaskSource !== url) return;
      if (persistedMaskNode) {
        persistedMaskNode.destroy();
        persistedMaskNode = null;
        persistedMaskLayer.batchDraw();
      }
    };
    img.src = url;
  }

  function handleResize() {
    if (!containerEl || !stage) return;
    const rect = containerEl.getBoundingClientRect();
    containerW = rect.width;
    containerH = rect.height;
    stage.width(containerW);
    stage.height(containerH);
    drawCheckerboard();
  }

  function drawCheckerboard() {
    if (!bgLayer) return;
    bgLayer.destroyChildren();

    // Use one pattern-filled rect instead of thousands of tiles for smooth panning.
    if (!checkerPatternCanvas) {
      const tileSize = 16;
      const pattern = document.createElement("canvas");
      pattern.width = tileSize * 2;
      pattern.height = tileSize * 2;
      const ctx = pattern.getContext("2d");
      if (ctx) {
        ctx.fillStyle = "#1a1a1a";
        ctx.fillRect(0, 0, pattern.width, pattern.height);
        ctx.fillStyle = "#2a2a2a";
        ctx.fillRect(tileSize, 0, tileSize, tileSize);
        ctx.fillRect(0, tileSize, tileSize, tileSize);
      }
      checkerPatternCanvas = pattern;
    }

    if (canvas.showCheckerboard && checkerPatternCanvas) {
      checkerRect = new Konva.Rect({
        x: 0,
        y: 0,
        width: canvas.canvasWidth,
        height: canvas.canvasHeight,
        fillPatternImage: checkerPatternCanvas as unknown as HTMLImageElement,
        fillPatternRepeat: "repeat",
        listening: false,
      });
      bgLayer.add(checkerRect);
    }

    // Canvas border
    borderRect = new Konva.Rect({
      x: 0,
      y: 0,
      width: canvas.canvasWidth,
      height: canvas.canvasHeight,
      stroke: "#555",
      strokeWidth: 1 / canvas.viewport.zoom,
      listening: false,
    });
    bgLayer.add(borderRect);

    bgLayer.batchDraw();
  }

  // Sync Konva layers with canvas store layers
  function syncKonvaLayers() {
    if (!stage) return;

    const sorted = [...canvas.layers].sort((a, b) => a.order - b.order);

    for (const layer of sorted) {
      const effectiveVisible =
        layer.visible && !(layer.type === "mask" && hideInpaintMask);
      if (!konvaLayers.has(layer.id)) {
        const kLayer = new Konva.Layer({
          id: layer.id,
          opacity: layer.opacity,
          visible: effectiveVisible,
        });

        // Clip to canvas bounds
        kLayer.clip({
          x: 0,
          y: 0,
          width: canvas.canvasWidth,
          height: canvas.canvasHeight,
        });

        stage.add(kLayer);

        konvaLayers.set(layer.id, kLayer);
      } else {
        const kLayer = konvaLayers.get(layer.id)!;
        kLayer.opacity(layer.opacity);
        kLayer.visible(effectiveVisible);
      }
    }

    // Remove any Konva layers that no longer exist in store
    for (const [id, kLayer] of konvaLayers) {
      if (!canvas.layers.find((l) => l.id === id)) {
        kLayer.destroy();
        konvaLayers.delete(id);
      }
    }

    reorderStageLayers();
  }

  function applyViewport() {
    if (!stage) return;
    const { zoom, panX, panY } = canvas.viewport;

    if (borderRect) {
      borderRect.strokeWidth(1 / zoom);
    }

    // Apply to all content layers (bg + reference + persisted mask + canvas layers), but NOT the UI layer.
    const layers = [bgLayer, refLayer, persistedMaskLayer, ...konvaLayers.values()];
    for (const layer of layers) {
      if (!layer) continue;
      layer.scaleX(zoom);
      layer.scaleY(zoom);
      layer.x(panX);
      layer.y(panY);
    }

    stage.batchDraw();
  }

  // Get pointer position in canvas coordinates (accounting for zoom/pan)
  function getCanvasPos(e: Konva.KonvaEventObject<MouseEvent | TouchEvent>): { x: number; y: number } | null {
    if (!stage) return null;
    const pointerPos = stage.getPointerPosition();
    if (!pointerPos) return null;

    const { zoom, panX, panY } = canvas.viewport;
    return {
      x: (pointerPos.x - panX) / zoom,
      y: (pointerPos.y - panY) / zoom,
    };
  }

  function getActiveKonvaLayer(): Konva.Layer | null {
    if (!canvas.activeLayerId) return null;
    return konvaLayers.get(canvas.activeLayerId) ?? null;
  }

  function isInpaintMaskMode(): boolean {
    return generation.mode === "inpainting" && canvas.inpaintDrawMode === "mask";
  }

  function isInpaintingMode(): boolean {
    return generation.mode === "inpainting";
  }

  function getMaskTargetLayer(): { layer: (typeof canvas.layers)[number]; kLayer: Konva.Layer } | null {
    const maskLayer = canvas.layers.find((l) => l.type === "mask");
    if (!maskLayer || maskLayer.locked) return null;

    const kLayer = konvaLayers.get(maskLayer.id) ?? null;
    if (!kLayer) return null;

    return { layer: maskLayer, kLayer };
  }

  function getDrawingTargetLayer(): { layer: (typeof canvas.layers)[number]; kLayer: Konva.Layer } | null {
    if (isInpaintMaskMode()) {
      return getMaskTargetLayer();
    }

    const layer = canvas.activeLayer;
    if (!layer || layer.locked) return null;

    const kLayer = getActiveKonvaLayer();
    if (!kLayer) return null;

    return { layer, kLayer };
  }

  async function autoCommitMaskIfNeeded() {
    if (!isInpaintMaskMode()) return;
    try {
      await canvas.syncMaskToGeneration(getMaskCanvas(), false);
    } catch (error) {
      console.error("Failed to auto-sync inpaint mask:", error);
    }
  }

  // Regenerate the small pixel preview shown for a layer in the layer panel.
  function refreshLayerThumbnail(id: string) {
    const kLayer = konvaLayers.get(id);
    if (!kLayer) return;

    const w = canvas.canvasWidth;
    const h = canvas.canvasHeight;
    const maxDim = Math.max(w, h);
    if (maxDim <= 0) return;

    // The viewport is applied as a layer transform; reset it so the thumbnail
    // captures canvas-space pixels at a fixed scale, then restore it.
    const origScale = kLayer.scaleX();
    const origX = kLayer.x();
    const origY = kLayer.y();
    kLayer.scaleX(1);
    kLayer.scaleY(1);
    kLayer.x(0);
    kLayer.y(0);

    let url: string | null = null;
    try {
      url = kLayer.toDataURL({
        pixelRatio: 64 / maxDim,
        width: w,
        height: h,
        x: 0,
        y: 0,
      });
    } catch (error) {
      console.error("Failed to generate layer thumbnail:", error);
    } finally {
      kLayer.scaleX(origScale);
      kLayer.scaleY(origScale);
      kLayer.x(origX);
      kLayer.y(origY);
    }

    if (url) canvas.setLayerThumbnail(id, url);
  }

  // Coalesce thumbnail regeneration to one per frame per layer so painting
  // never regenerates a thumbnail mid-stroke.
  function scheduleThumbRefresh(id: string) {
    if (thumbRafs.has(id)) return;
    const raf = requestAnimationFrame(() => {
      thumbRafs.delete(id);
      refreshLayerThumbnail(id);
    });
    thumbRafs.set(id, raf);
  }

  function loadImageEl(src: string): Promise<HTMLImageElement> {
    return new Promise((resolve, reject) => {
      const img = new Image();
      img.onload = () => resolve(img);
      img.onerror = reject;
      img.src = src;
    });
  }

  // Re-hydrate a preserved inpaint mask (tinted, transparent-bg data URL) onto
  // the freshly-rebuilt mask layer after a base swap or an inpaint-base undo.
  async function restoreMaskFromSnapshot(url: string) {
    if (!stage) return;
    const maskMeta = canvas.layers.find((l) => l.type === "mask");
    if (!maskMeta) return;
    const kLayer = konvaLayers.get(maskMeta.id);
    if (!kLayer) return;

    let img: HTMLImageElement;
    try {
      img = await loadImageEl(url);
    } catch {
      return;
    }

    // The layer may have been rebuilt again while the image loaded; bail if so.
    if (!stage || konvaLayers.get(maskMeta.id) !== kLayer) return;

    kLayer.destroyChildren();
    const kImage = new Konva.Image({
      image: img,
      x: 0,
      y: 0,
      width: canvas.canvasWidth,
      height: canvas.canvasHeight,
      listening: false,
    });
    kLayer.add(kImage);
    kLayer.batchDraw();
    scheduleThumbRefresh(maskMeta.id);
  }

  // Drawing handlers
  function handlePointerDown(e: Konva.KonvaEventObject<MouseEvent | TouchEvent>) {
    const evt = e.evt as MouseEvent;

    // Middle mouse → pan
    if (evt.button === 1) {
      isPanning = true;
      lastPointerPos = stage!.getPointerPosition();
      e.evt.preventDefault();
      return;
    }

    // Right click → ignore (context menu)
    const isTemporaryInpaintErase = evt.button === 2 && isInpaintingMode();
    if (evt.button === 2 && !isTemporaryInpaintErase) return;
    if (isTemporaryInpaintErase) e.evt.preventDefault();

    const tool = isTemporaryInpaintErase ? "eraser" : canvas.activeTool;
    const pos = getCanvasPos(e);
    if (!pos) return;

    if (tool === "view") {
      isPanning = true;
      lastPointerPos = stage!.getPointerPosition();
      return;
    }

    if (tool === "brush" || tool === "eraser") {
      const target = getDrawingTargetLayer();
      if (!target) return;
      const { layer, kLayer } = target;

      // Painting more mask over a previewed result brings the mask back on screen.
      if (layer.type === "mask") canvas.markMaskEdited();

      // Snapshot for undo before drawing
      canvasHistory.snapshot(layer.id);

      isDrawing = true;
      activeStrokeTool = tool;

      const inpaintMaskMode = isInpaintMaskMode();

      const color = tool === "eraser"
        ? "#000000"
        : inpaintMaskMode
          ? canvas.maskOverlayColor
          : layer.type === "mask"
          ? canvas.maskOverlayColor
          : canvas.foregroundColor;

      const drawOpacity = tool === "eraser"
        ? 1
        : inpaintMaskMode
          ? Math.min(canvas.brushSettings.opacity, 0.45)
          : canvas.brushSettings.opacity;

      currentLine = new Konva.Line({
        stroke: color,
        strokeWidth: canvas.brushSettings.size,
        opacity: drawOpacity,
        globalCompositeOperation: tool === "eraser" ? "destination-out" : "source-over",
        lineCap: "round",
        lineJoin: "round",
        tension: 0,
        points: [pos.x, pos.y, pos.x, pos.y],
        listening: false,
      });

      kLayer.add(currentLine);
      kLayer.batchDraw();
    }

    if (tool === "rectFill") {
      const target = getDrawingTargetLayer();
      if (!target) return;
      const { layer } = target;

      if (layer.type === "mask") canvas.markMaskEdited();

      // Snapshot for undo before rect fill
      canvasHistory.snapshot(layer.id);

      isDrawingRect = true;
      rectStartPos = pos;

      const inpaintMaskMode = isInpaintMaskMode();

      // Create preview rect on UI layer
      const color = inpaintMaskMode
        ? canvas.maskOverlayColor
        : layer.type === "mask"
          ? canvas.maskOverlayColor
          : canvas.foregroundColor;
      rectPreview = new Konva.Rect({
        x: pos.x,
        y: pos.y,
        width: 0,
        height: 0,
        fill: color,
        opacity: inpaintMaskMode ? 0.35 : 0.4,
        listening: false,
      });
      uiLayer?.add(rectPreview);
    }

    if (tool === "eyedropper") {
      sampleColor(pos);
    }

    if (tool === "lasso") {
      const target = getDrawingTargetLayer();
      if (!target) return;
      const { layer } = target;

      if (layer.type === "mask") canvas.markMaskEdited();

      // Snapshot for undo before committing the lasso fill.
      canvasHistory.snapshot(layer.id);

      isLasso = true;
      lassoPoints = [pos.x, pos.y];

      const inpaintMaskMode = isInpaintMaskMode();
      const color = inpaintMaskMode
        ? canvas.maskOverlayColor
        : layer.type === "mask"
          ? canvas.maskOverlayColor
          : canvas.foregroundColor;

      // Preview lives on the unscaled UI layer, so points are in screen space.
      const { zoom, panX, panY } = canvas.viewport;
      lassoPreviewLine = new Konva.Line({
        points: [pos.x * zoom + panX, pos.y * zoom + panY],
        stroke: color,
        strokeWidth: 1.5,
        dash: [4, 4],
        closed: false,
        listening: false,
      });
      uiLayer?.add(lassoPreviewLine);
      uiLayer?.batchDraw();
    }

    if (tool === "move") {
      const layer = canvas.activeLayer;
      if (!layer || layer.locked) return;

      const kLayer = getActiveKonvaLayer();
      if (!kLayer) return;

      canvasHistory.snapshot(layer.id);
      canvas.beginMove(layer.id, pos.x, pos.y);

      isMovingLayer = true;
      moveStartPos = pos;
      moveNodeStarts = kLayer.getChildren().map((node) => ({
        node,
        x: node.x(),
        y: node.y(),
      }));
    }
  }

  function handlePointerMove(e: Konva.KonvaEventObject<MouseEvent | TouchEvent>) {
    if (!stage) return;

    const pointerPos = stage.getPointerPosition();
    if (!pointerPos) return;

    // Update canvas cursor position
    const canvasPos = getCanvasPos(e);
    canvas.cursorPos = canvasPos;

    // Update brush cursor
    if (brushCursor && canvasPos) {
      const tool = activeStrokeTool ?? canvas.activeTool;
      const showCursor = tool === "brush" || tool === "eraser";
      brushCursor.visible(showCursor);
      if (showCursor) {
        brushCursor.x(canvasPos.x);
        brushCursor.y(canvasPos.y);
        brushCursor.radius(canvas.brushSettings.size / 2);
        brushCursor.stroke(tool === "eraser" ? "#ffffff" : canvas.foregroundColor);
        // Position brush cursor in screen space within UI layer
        const { zoom, panX, panY } = canvas.viewport;
        brushCursor.x(canvasPos.x * zoom + panX);
        brushCursor.y(canvasPos.y * zoom + panY);
        brushCursor.radius((canvas.brushSettings.size * zoom) / 2);
        brushCursor.strokeWidth(1.5);
        uiLayer?.batchDraw();
      }
    }

    // Panning
    if (isPanning && lastPointerPos) {
      const dx = pointerPos.x - lastPointerPos.x;
      const dy = pointerPos.y - lastPointerPos.y;
      canvas.viewport = {
        ...canvas.viewport,
        panX: canvas.viewport.panX + dx,
        panY: canvas.viewport.panY + dy,
      };
      lastPointerPos = pointerPos;
      scheduleViewportApply();
      return;
    }

    // Drawing
    if (isDrawing && currentLine) {
      const pos = getCanvasPos(e);
      if (!pos) return;

      const points = currentLine.points();
      currentLine.points([...points, pos.x, pos.y]);
      // Redraw the layer the stroke actually lives on. In inpaint-mask mode the
      // line is added to the mask layer, not the active layer, so redrawing the
      // active layer here would leave the in-progress stroke invisible.
      currentLine.getLayer()?.batchDraw();
    }

    // Rectangle preview
    if (isDrawingRect && rectPreview && rectStartPos) {
      const pos = getCanvasPos(e);
      if (!pos) return;

      const x = Math.min(rectStartPos.x, pos.x);
      const y = Math.min(rectStartPos.y, pos.y);
      const w = Math.abs(pos.x - rectStartPos.x);
      const h = Math.abs(pos.y - rectStartPos.y);

      // Position in screen space for UI layer
      const { zoom, panX, panY } = canvas.viewport;
      rectPreview.x(x * zoom + panX);
      rectPreview.y(y * zoom + panY);
      rectPreview.width(w * zoom);
      rectPreview.height(h * zoom);
      uiLayer?.batchDraw();
    }

    if (isMovingLayer && moveStartPos) {
      const pos = getCanvasPos(e);
      if (!pos) return;

      const dx = pos.x - moveStartPos.x;
      const dy = pos.y - moveStartPos.y;
      canvas.updateMove(pos.x, pos.y);

      for (const entry of moveNodeStarts) {
        entry.node.x(entry.x + dx);
        entry.node.y(entry.y + dy);
      }
      getActiveKonvaLayer()?.batchDraw();
    }

    // Lasso preview (append screen-space point to the dashed outline)
    if (isLasso && lassoPreviewLine) {
      const pos = getCanvasPos(e);
      if (!pos) return;

      lassoPoints = [...lassoPoints, pos.x, pos.y];
      const { zoom, panX, panY } = canvas.viewport;
      const prev = lassoPreviewLine.points();
      lassoPreviewLine.points([...prev, pos.x * zoom + panX, pos.y * zoom + panY]);
      uiLayer?.batchDraw();
    }

    if (tooltipRaf === null) {
      tooltipRaf = requestAnimationFrame(() => updateTooltip(e));
    }
  }

  function handlePointerUp(e: Konva.KonvaEventObject<MouseEvent | TouchEvent>) {
    if (isPanning) {
      isPanning = false;
      lastPointerPos = null;
      return;
    }
    
    if (tooltipVisible) {
      tooltipVisible = false;
    }

    let shouldAutoCommitMask = false;

    if (isDrawing) {
      // Capture the layer the stroke lives on before clearing currentLine.
      const strokeLayerId = currentLine?.getLayer()?.id();
      isDrawing = false;
      currentLine = null;
      activeStrokeTool = null;
      shouldAutoCommitMask = true;
      if (strokeLayerId) scheduleThumbRefresh(strokeLayerId);
    }
    
    if (isDrawingRect && rectStartPos) {
      isDrawingRect = false;
      const pos = getCanvasPos(e);
      if (pos) {
        // Create final rect on the active Konva layer
        const target = getDrawingTargetLayer();
        if (target) {
          const { layer, kLayer } = target;
          const x = Math.min(rectStartPos.x, pos.x);
          const y = Math.min(rectStartPos.y, pos.y);
          const w = Math.abs(pos.x - rectStartPos.x);
          const h = Math.abs(pos.y - rectStartPos.y);

          if (w > 1 && h > 1) {
            const inpaintMaskMode = isInpaintMaskMode();

            const color = inpaintMaskMode
              ? canvas.maskOverlayColor
              : layer.type === "mask"
                ? canvas.maskOverlayColor
                : canvas.foregroundColor;
            const rect = new Konva.Rect({
              x, y, width: w, height: h,
              fill: color,
              opacity: inpaintMaskMode
                ? Math.min(canvas.brushSettings.opacity, 0.45)
                : canvas.brushSettings.opacity,
              listening: false,
            });
            kLayer.add(rect);
            kLayer.batchDraw();
            shouldAutoCommitMask = true;
            scheduleThumbRefresh(layer.id);
          }
        }
      }

      // Remove preview from UI layer
      if (rectPreview) {
        rectPreview.destroy();
        rectPreview = null;
        uiLayer?.batchDraw();
      }
      rectStartPos = null;
    }

    if (isLasso) {
      isLasso = false;
      const target = getDrawingTargetLayer();
      if (target && lassoPoints.length >= 6) {
        const { layer, kLayer } = target;
        const inpaintMaskMode = isInpaintMaskMode();
        const color = inpaintMaskMode
          ? canvas.maskOverlayColor
          : layer.type === "mask"
            ? canvas.maskOverlayColor
            : canvas.foregroundColor;
        const shape = new Konva.Line({
          points: [...lassoPoints],
          closed: true,
          fill: color,
          opacity: inpaintMaskMode
            ? Math.min(canvas.brushSettings.opacity, 0.45)
            : canvas.brushSettings.opacity,
          listening: false,
        });
        kLayer.add(shape);
        kLayer.batchDraw();
        shouldAutoCommitMask = true;
        scheduleThumbRefresh(layer.id);
      }
      lassoPoints = [];
      if (lassoPreviewLine) {
        lassoPreviewLine.destroy();
        lassoPreviewLine = null;
        uiLayer?.batchDraw();
      }
    }

    if (isMovingLayer) {
      isMovingLayer = false;
      moveStartPos = null;
      moveNodeStarts = [];
      canvas.endMove();
    }

    if (shouldAutoCommitMask) {
      void autoCommitMaskIfNeeded();
    }
  }

  function handlePointerLeave() {
    tooltipVisible = false;
    canvas.isPointerOverStage = false;
    canvas.cursorPos = null;
    if (brushCursor) {
      brushCursor.visible(false);
      uiLayer?.batchDraw();
    }

    if (isSpacePanning) {
      isSpacePanning = false;
      canvas.restorePreviousTool();
    }

    if (isPanning) {
      isPanning = false;
      lastPointerPos = null;
    }

    if (isDrawing) {
      isDrawing = false;
      currentLine = null;
      activeStrokeTool = null;
    }

    if (isDrawingRect) {
      isDrawingRect = false;
      rectStartPos = null;
      if (rectPreview) {
        rectPreview.destroy();
        rectPreview = null;
        uiLayer?.batchDraw();
      }
    }

    if (isLasso) {
      isLasso = false;
      lassoPoints = [];
      if (lassoPreviewLine) {
        lassoPreviewLine.destroy();
        lassoPreviewLine = null;
        uiLayer?.batchDraw();
      }
    }

    if (isMovingLayer) {
      isMovingLayer = false;
      moveStartPos = null;
      moveNodeStarts = [];
      canvas.endMove();
    }
  }

  function handlePointerEnter() {
    canvas.isPointerOverStage = true;
  }

  function handleWheel(e: Konva.KonvaEventObject<WheelEvent>) {
    e.evt.preventDefault();
    const pointerPos = stage!.getPointerPosition();
    if (!pointerPos) return;

    const delta = e.evt.deltaY;
    const scaleBy = 1.08;
    const oldZoom = canvas.viewport.zoom;
    const newZoom = delta > 0 ? oldZoom / scaleBy : oldZoom * scaleBy;

    canvas.setZoom(newZoom, pointerPos.x, pointerPos.y);
    scheduleViewportApply();
  }

  function sampleColor(pos: { x: number; y: number }) {
    if (!stage) return;

    // Composite all visible layers
    const compositeCanvas = stage.toCanvas({
      pixelRatio: 1,
    });
    const ctx = compositeCanvas.getContext("2d")!;
    const { zoom, panX, panY } = canvas.viewport;
    const screenX = pos.x * zoom + panX;
    const screenY = pos.y * zoom + panY;
    const pixel = ctx.getImageData(Math.round(screenX), Math.round(screenY), 1, 1).data;

    if (pixel[3] > 0) {
      const hex = `#${pixel[0].toString(16).padStart(2, "0")}${pixel[1].toString(16).padStart(2, "0")}${pixel[2].toString(16).padStart(2, "0")}`;
      canvas.foregroundColor = hex;
    }
  }

  // Space bar pan support
  function handleKeyDown(e: KeyboardEvent) {
    // Escape cancels an in-progress lasso even if the pointer left the stage.
    if (e.code === "Escape" && isLasso) {
      isLasso = false;
      lassoPoints = [];
      if (lassoPreviewLine) {
        lassoPreviewLine.destroy();
        lassoPreviewLine = null;
        uiLayer?.batchDraw();
      }
      return;
    }

    if (!canvas.isPointerOverStage) return;

    if (e.code === "Space" && !isSpacePanning && !e.repeat) {
      isSpacePanning = true;
      canvas.setTool("view");
      e.preventDefault();
    }

    // Hold Alt for a quick eyedropper; release restores the previous tool.
    if (e.altKey && !isAltEyedropper && !e.repeat && canvas.activeTool !== "eyedropper") {
      isAltEyedropper = true;
      canvas.setTool("eyedropper");
      e.preventDefault();
    }
  }

  function handleKeyUp(e: KeyboardEvent) {
    if (e.code === "Space" && isSpacePanning) {
      isSpacePanning = false;
      canvas.restorePreviousTool();
    }

    if (isAltEyedropper && !e.altKey) {
      isAltEyedropper = false;
      canvas.restorePreviousTool();
    }
  }

  // Reactive effects
  $effect(() => {
    // Re-sync Konva layers when canvas layers change
    void canvas.layers;
    syncKonvaLayers();
    applyViewport();

    // Re-hydrate a preserved inpaint mask onto the freshly-rebuilt mask layer
    // (set by the store on a base swap or an inpaint-base undo).
    const restoreUrl = canvas.pendingMaskRestoreUrl;
    if (restoreUrl) {
      canvas.pendingMaskRestoreUrl = null;
      void restoreMaskFromSnapshot(restoreUrl);
    }

    // Generate an initial thumbnail for any layer we haven't captured yet.
    for (const layer of canvas.layers) {
      if (!thumbInitialized.has(layer.id)) {
        thumbInitialized.add(layer.id);
        scheduleThumbRefresh(layer.id);
      }
    }
    // Forget removed layers so a re-added id regenerates and drop pending RAFs.
    for (const id of [...thumbInitialized]) {
      if (!canvas.layers.find((l) => l.id === id)) {
        thumbInitialized.delete(id);
        const raf = thumbRafs.get(id);
        if (raf !== undefined) {
          cancelAnimationFrame(raf);
          thumbRafs.delete(id);
        }
      }
    }
  });

  let historyDims = { w: 0, h: 0 };
  $effect(() => {
    // Keep undo/redo snapshot dimensions in sync with the canvas. When the
    // canvas is resized (e.g. a new image is loaded) existing snapshots no
    // longer match the new dimensions, so discard them rather than restoring
    // stretched or clipped pixels.
    const w = canvas.canvasWidth;
    const h = canvas.canvasHeight;
    if (w === historyDims.w && h === historyDims.h) return;
    const hadDims = historyDims.w !== 0 || historyDims.h !== 0;
    historyDims = { w, h };
    canvasHistory.setRefs(konvaLayers, w, h);
    if (hadDims) canvasHistory.clear();
  });

  $effect(() => {
    // Re-apply viewport when it changes (coalesced to one frame)
    void canvas.viewport;
    scheduleViewportApply();
  });

  $effect(() => {
    // Redraw checkerboard when toggle changes
    void canvas.showCheckerboard;
    if (bgLayer) {
      if (canvas.showCheckerboard) {
        drawCheckerboard();
      } else {
        bgLayer.destroyChildren();
        bgLayer.batchDraw();
      }
    }
  });

  $effect(() => {
    void canvas.effectiveReferenceImage;
    updateReferenceImage(canvas.effectiveReferenceImage);
  });

  $effect(() => {
    // Toggle mask-layer visibility when the pending-result hide state changes.
    // Only the Konva node's visibility is touched, never the layer model's
    // `visible`, so the panel toggle and mask export (which reads the model) are
    // unaffected.
    const hide = hideInpaintMask;
    void canvas.layers;
    if (!stage) return;
    for (const layer of canvas.layers) {
      if (layer.type !== "mask") continue;
      const kLayer = konvaLayers.get(layer.id);
      if (!kLayer) continue;
      kLayer.visible(layer.visible && !hide);
    }
    stage.batchDraw();
  });

  $effect(() => {
    void canvas.persistedMaskPreviewUrl;
    void canvas.maskOverlayVisible;
    void canvas.maskOverlayColor;
    void canvas.maskOverlayOpacity;
    void canvas.canvasWidth;
    void canvas.canvasHeight;
    updatePersistedMaskOverlay(canvas.persistedMaskPreviewUrl);
  });

  // Public API for export
  export function getRasterComposite(): HTMLCanvasElement | null {
    if (!stage) return null;

    const offscreen = document.createElement("canvas");
    offscreen.width = canvas.canvasWidth;
    offscreen.height = canvas.canvasHeight;
    const ctx = offscreen.getContext("2d")!;

    const sorted = [...canvas.layers]
      .filter((l) => l.type === "raster" && l.visible)
      .sort((a, b) => a.order - b.order);

    for (const layer of sorted) {
      const kLayer = konvaLayers.get(layer.id);
      if (!kLayer) continue;

      // Reset layer transform temporarily for export
      const origScale = kLayer.scaleX();
      const origX = kLayer.x();
      const origY = kLayer.y();
      kLayer.scaleX(1);
      kLayer.scaleY(1);
      kLayer.x(0);
      kLayer.y(0);

      const layerCanvas = kLayer.toCanvas({
        pixelRatio: 1,
        width: canvas.canvasWidth,
        height: canvas.canvasHeight,
      });

      ctx.globalAlpha = layer.opacity;
      ctx.drawImage(layerCanvas, 0, 0);
      ctx.globalAlpha = 1;

      // Restore transform
      kLayer.scaleX(origScale);
      kLayer.scaleY(origScale);
      kLayer.x(origX);
      kLayer.y(origY);
    }

    return offscreen;
  }

  export function getMaskCanvas(): HTMLCanvasElement | null {
    if (!stage) return null;

    const maskLayers = canvas.layers.filter((l) => l.type === "mask" && l.visible);
    if (maskLayers.length === 0) return null;

    const offscreen = document.createElement("canvas");
    offscreen.width = canvas.canvasWidth;
    offscreen.height = canvas.canvasHeight;
    const ctx = offscreen.getContext("2d")!;

    for (const layer of maskLayers) {
      const kLayer = konvaLayers.get(layer.id);
      if (!kLayer) continue;

      const origScale = kLayer.scaleX();
      const origX = kLayer.x();
      const origY = kLayer.y();
      // The mask node may be visually hidden while a result is previewed; Konva
      // renders nothing for an invisible node, so force it visible for the capture
      // and restore afterwards (no on-screen redraw happens in between).
      const origVisible = kLayer.visible();
      kLayer.scaleX(1);
      kLayer.scaleY(1);
      kLayer.x(0);
      kLayer.y(0);
      kLayer.visible(true);

      const layerCanvas = kLayer.toCanvas({
        pixelRatio: 1,
        width: canvas.canvasWidth,
        height: canvas.canvasHeight,
      });

      ctx.drawImage(layerCanvas, 0, 0);

      kLayer.scaleX(origScale);
      kLayer.scaleY(origScale);
      kLayer.x(origX);
      kLayer.y(origY);
      kLayer.visible(origVisible);
    }

    return offscreen;
  }

  // Get cursor style based on active tool
  function getCursorClass(): string {
    const tool = canvas.activeTool;
    if (isPanning || tool === "view") return "cursor-grab";
    if (tool === "move") return "cursor-move";
    if (tool === "eyedropper" || tool === "lasso") return "cursor-crosshair";
    if (tool === "brush" || tool === "eraser") return "cursor-none";
    return "cursor-default";
  }
</script>

<svelte:window onkeydown={handleKeyDown} onkeyup={handleKeyUp} />

<div
  class="w-full h-full relative overflow-hidden bg-neutral-950 {getCursorClass()}"
  bind:this={containerEl}
>
  {#if tooltipVisible}
    <div class="fixed" style="left: {tooltipPos.x}px; top: {tooltipPos.y}px; z-index: 100; pointer-events: none;">
      <ColorTooltip color={tooltipColor} />
    </div>
  {/if}
</div>
