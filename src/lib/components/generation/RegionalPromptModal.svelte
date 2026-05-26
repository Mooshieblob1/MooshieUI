<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import type { RegionalPromptSelection, RegionalPromptShape } from "../../types/index.js";

  interface Props {
    onclose: () => void;
  }

  interface NormalizedPoint {
    x: number;
    y: number;
  }

  let { onclose }: Props = $props();

  type CanvasTool = "select" | RegionalPromptShape;
  type ResizeHandle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";

  interface EditGesture {
    kind: "move" | "resize";
    regionId: string;
    handle?: ResizeHandle;
    startPoint: NormalizedPoint;
    startRegion: RegionalPromptSelection;
  }

  const MIN_REGION_SIZE = 0.02;
  const HANDLE_HIT_RADIUS = 0.014;
  const HANDLE_VIS_RADIUS = 0.008;

  let activeTool = $state<CanvasTool>("select");
  let draftRegions = $state<RegionalPromptSelection[]>(generation.regionalPrompts.map(cloneRegion));
  let selectedRegionId = $state<string | null>(generation.regionalPrompts[0]?.id ?? null);
  let drawStart = $state<NormalizedPoint | null>(null);
  let drawCurrent = $state<NormalizedPoint | null>(null);
  let drawingPointerId = $state<number | null>(null);
  let editGesture = $state<EditGesture | null>(null);
  let lassoPoints = $state<NormalizedPoint[]>([]);
  let lassoDrawing = $state(false);
  let canvasEl = $state<HTMLElement | null>(null);
  let snapPreview = $state(false);
  let viewportW = $state(typeof window !== "undefined" ? window.innerWidth : 1280);
  let viewportH = $state(typeof window !== "undefined" ? window.innerHeight : 800);

  const canvasWidthPx = $derived(Number.isFinite(generation.width) && generation.width > 0 ? generation.width : 1);
  const canvasHeightPx = $derived(Number.isFinite(generation.height) && generation.height > 0 ? generation.height : 1);
  const canvasAspectRatio = $derived(canvasWidthPx / canvasHeightPx);

  const canvasLayout = $derived.by(() => {
    const overlayPad = 32;
    const maxModalW = Math.min(Math.floor(viewportW * 0.96) - overlayPad, 1280);
    const maxModalH = Math.min(Math.floor(viewportH * 0.92) - overlayPad, 900);
    const sideBySide = viewportW >= 1024;

    // Measured/conservative Tailwind defaults (p-4=16, p-3=12, gap-3=12, border=1px).
    const gridPadY = 32;
    const columnGap = 12;
    const wrapPadY = 24;
    const frameBorderY = 2;
    const canvasBorderY = 2;
    const headerH = 80;
    const footerH = 57;
    const toolbarH = 52;
    const snapHintH = snapPreview ? 20 : 0;
    const bodyPadX = 32;
    const gridGap = 12;
    const sidebarW = 320;
    const stackedSidebarMinH = 240;
    const safetyMargin = 16;

    const canvasVerticalChrome = wrapPadY + frameBorderY + canvasBorderY;
    const horizontalChrome =
      bodyPadX + canvasVerticalChrome + (sideBySide ? gridGap + sidebarW : 0);
    const verticalChrome =
      headerH +
      footerH +
      gridPadY +
      toolbarH +
      columnGap +
      canvasVerticalChrome +
      snapHintH +
      safetyMargin +
      (sideBySide ? 0 : gridGap + stackedSidebarMinH);

    const maxCanvasW = Math.max(120, maxModalW - horizontalChrome);
    const maxCanvasH = Math.max(120, maxModalH - verticalChrome);
    const ratio = canvasAspectRatio;

    let canvasW: number;
    let canvasH: number;
    if (maxCanvasW / maxCanvasH >= ratio) {
      canvasH = Math.floor(maxCanvasH);
      canvasW = Math.floor(canvasH * ratio);
    } else {
      canvasW = Math.floor(maxCanvasW);
      canvasH = Math.floor(canvasW / ratio);
    }

    const minShortSide = 120;
    if (canvasW < minShortSide || canvasH < minShortSide) {
      if (ratio >= 1) {
        canvasH = Math.max(canvasH, minShortSide);
        canvasW = Math.floor(canvasH * ratio);
      } else {
        canvasW = Math.max(canvasW, minShortSide);
        canvasH = Math.floor(canvasW / ratio);
      }
    }

    if (canvasW > maxCanvasW) {
      canvasW = Math.floor(maxCanvasW);
      canvasH = Math.floor(canvasW / ratio);
    }
    if (canvasH > maxCanvasH) {
      canvasH = Math.floor(maxCanvasH);
      canvasW = Math.floor(canvasH * ratio);
    }

    const modalW = sideBySide
      ? canvasW + horizontalChrome
      : canvasW + bodyPadX + canvasVerticalChrome;
    const modalH =
      headerH +
      footerH +
      gridPadY +
      toolbarH +
      columnGap +
      canvasVerticalChrome +
      canvasH +
      snapHintH +
      safetyMargin +
      (sideBySide ? 0 : gridGap + stackedSidebarMinH);

    return {
      canvasW,
      canvasH,
      modalW: Math.min(maxModalW, modalW),
      modalH: Math.min(maxModalH, modalH),
      sideBySide,
    };
  });

  function handleViewportResize(): void {
    viewportW = window.innerWidth;
    viewportH = window.innerHeight;
  }

  const selectedRegion = $derived(
    selectedRegionId ? draftRegions.find((region) => region.id === selectedRegionId) ?? null : null,
  );

  const draftShape = $derived.by(() => {
    if (!drawStart || !drawCurrent || activeTool === "lasso") return null;
    return boundsFromPoints(drawStart, drawCurrent);
  });

  function clamp01(value: number): number {
    return Math.max(0, Math.min(1, value));
  }

  function clampStrength(value: number): number {
    if (!Number.isFinite(value)) return 1;
    return Math.max(0, Math.min(2, value));
  }

  function cloneRegion(region: RegionalPromptSelection): RegionalPromptSelection {
    return {
      ...region,
      strength: clampStrength(region.strength),
      x: clamp01(region.x),
      y: clamp01(region.y),
      width: clamp01(region.width),
      height: clamp01(region.height),
      points: region.points?.map((point) => ({ x: clamp01(point.x), y: clamp01(point.y) })),
    };
  }

  function toCanvasPoint(event: PointerEvent | MouseEvent): NormalizedPoint | null {
    if (!canvasEl) return null;
    const rect = canvasEl.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return null;
    const x = clamp01((event.clientX - rect.left) / rect.width);
    const y = clamp01((event.clientY - rect.top) / rect.height);
    return { x, y };
  }

  function boundsFromPoints(a: NormalizedPoint, b: NormalizedPoint): Pick<RegionalPromptSelection, "x" | "y" | "width" | "height"> {
    const x = Math.min(a.x, b.x);
    const y = Math.min(a.y, b.y);
    const width = Math.abs(a.x - b.x);
    const height = Math.abs(a.y - b.y);
    return { x, y, width, height };
  }

  function generateRegionId(): string {
    return typeof crypto !== "undefined" && "randomUUID" in crypto
      ? crypto.randomUUID()
      : `${Date.now()}-${Math.round(Math.random() * 1_000_000)}`;
  }

  function createRegion(shape: RegionalPromptShape, bounds: Pick<RegionalPromptSelection, "x" | "y" | "width" | "height">, points?: NormalizedPoint[]): RegionalPromptSelection {
    const id = generateRegionId();

    return {
      id,
      shape,
      text: "",
      strength: 0.75,
      x: clamp01(bounds.x),
      y: clamp01(bounds.y),
      width: clamp01(bounds.width),
      height: clamp01(bounds.height),
      points: points && points.length >= 3 ? points.map((point) => ({ x: clamp01(point.x), y: clamp01(point.y) })) : undefined,
    };
  }

  function addRegion(region: RegionalPromptSelection): void {
    draftRegions = [...draftRegions, region];
    selectedRegionId = region.id;
  }

  function distance(a: NormalizedPoint, b: NormalizedPoint): number {
    const dx = a.x - b.x;
    const dy = a.y - b.y;
    return Math.sqrt(dx * dx + dy * dy);
  }

  function pointInPolygon(point: NormalizedPoint, polygon: NormalizedPoint[]): boolean {
    let inside = false;
    for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
      const xi = polygon[i].x;
      const yi = polygon[i].y;
      const xj = polygon[j].x;
      const yj = polygon[j].y;
      const intersect =
        yi > point.y !== yj > point.y &&
        point.x < ((xj - xi) * (point.y - yi)) / (yj - yi + 1e-12) + xi;
      if (intersect) inside = !inside;
    }
    return inside;
  }

  function pointInRegion(point: NormalizedPoint, region: RegionalPromptSelection): boolean {
    const { x, y, width: w, height: h } = region;
    if (w <= 0 || h <= 0) return false;
    if (region.shape === "circle") {
      const cx = x + w / 2;
      const cy = y + h / 2;
      const rx = w / 2;
      const ry = h / 2;
      if (rx <= 0 || ry <= 0) return false;
      const dx = (point.x - cx) / rx;
      const dy = (point.y - cy) / ry;
      return dx * dx + dy * dy <= 1;
    }
    if (region.shape === "lasso" && region.points && region.points.length >= 3) {
      return pointInPolygon(point, region.points);
    }
    return point.x >= x && point.x <= x + w && point.y >= y && point.y <= y + h;
  }

  function getResizeHandles(region: RegionalPromptSelection): { id: ResizeHandle; x: number; y: number }[] {
    const { x, y, width: w, height: h } = region;
    const cx = x + w / 2;
    const cy = y + h / 2;
    return [
      { id: "nw", x, y },
      { id: "n", x: cx, y },
      { id: "ne", x: x + w, y },
      { id: "e", x: x + w, y: cy },
      { id: "se", x: x + w, y: y + h },
      { id: "s", x: cx, y: y + h },
      { id: "sw", x, y: y + h },
      { id: "w", x, y: cy },
    ];
  }

  function hitResizeHandle(point: NormalizedPoint, region: RegionalPromptSelection): ResizeHandle | null {
    for (const handle of getResizeHandles(region)) {
      if (distance(point, { x: handle.x, y: handle.y }) <= HANDLE_HIT_RADIUS) {
        return handle.id;
      }
    }
    return null;
  }

  function hitRegionAtPoint(point: NormalizedPoint): string | null {
    for (let i = draftRegions.length - 1; i >= 0; i--) {
      if (pointInRegion(point, draftRegions[i])) return draftRegions[i].id;
    }
    return null;
  }

  function resizeAnchor(handle: ResizeHandle, bounds: Pick<RegionalPromptSelection, "x" | "y" | "width" | "height">): NormalizedPoint {
    const right = bounds.x + bounds.width;
    const bottom = bounds.y + bounds.height;
    const midX = bounds.x + bounds.width / 2;
    const midY = bounds.y + bounds.height / 2;
    switch (handle) {
      case "nw":
        return { x: right, y: bottom };
      case "n":
        return { x: midX, y: bottom };
      case "ne":
        return { x: bounds.x, y: bottom };
      case "e":
        return { x: bounds.x, y: midY };
      case "se":
        return { x: bounds.x, y: bounds.y };
      case "s":
        return { x: midX, y: bounds.y };
      case "sw":
        return { x: right, y: bounds.y };
      case "w":
        return { x: right, y: midY };
    }
  }

  function resizeBoundsFromHandle(
    handle: ResizeHandle,
    start: RegionalPromptSelection,
    point: NormalizedPoint,
  ): Pick<RegionalPromptSelection, "x" | "y" | "width" | "height"> {
    const sx = start.x;
    const sy = start.y;
    const right = sx + start.width;
    const bottom = sy + start.height;
    let x = sx;
    let y = sy;
    let w = start.width;
    let h = start.height;

    if (handle.includes("e") && !handle.includes("w")) {
      w = Math.max(MIN_REGION_SIZE, point.x - sx);
    }
    if (handle.includes("w") && !handle.includes("e")) {
      x = Math.min(point.x, right - MIN_REGION_SIZE);
      w = right - x;
    }
    if (handle.includes("s") && !handle.includes("n")) {
      h = Math.max(MIN_REGION_SIZE, point.y - sy);
    }
    if (handle.includes("n") && !handle.includes("s")) {
      y = Math.min(point.y, bottom - MIN_REGION_SIZE);
      h = bottom - y;
    }
    if (handle === "nw") {
      x = Math.min(point.x, right - MIN_REGION_SIZE);
      y = Math.min(point.y, bottom - MIN_REGION_SIZE);
      w = right - x;
      h = bottom - y;
    }
    if (handle === "ne") {
      y = Math.min(point.y, bottom - MIN_REGION_SIZE);
      w = Math.max(MIN_REGION_SIZE, point.x - sx);
      h = bottom - y;
    }
    if (handle === "sw") {
      x = Math.min(point.x, right - MIN_REGION_SIZE);
      w = right - x;
      h = Math.max(MIN_REGION_SIZE, point.y - sy);
    }
    if (handle === "se") {
      w = Math.max(MIN_REGION_SIZE, point.x - sx);
      h = Math.max(MIN_REGION_SIZE, point.y - sy);
    }

    x = clamp01(x);
    y = clamp01(y);
    w = Math.min(1 - x, Math.max(MIN_REGION_SIZE, w));
    h = Math.min(1 - y, Math.max(MIN_REGION_SIZE, h));
    return { x, y, width: w, height: h };
  }

  function scaledPoints(
    start: RegionalPromptSelection,
    anchor: NormalizedPoint,
    nextBounds: Pick<RegionalPromptSelection, "x" | "y" | "width" | "height">,
  ): NormalizedPoint[] | undefined {
    if (!start.points?.length) return undefined;
    const sx = start.width || MIN_REGION_SIZE;
    const sy = start.height || MIN_REGION_SIZE;
    const scaleX = nextBounds.width / sx;
    const scaleY = nextBounds.height / sy;
    return start.points.map((point) => ({
      x: clamp01(anchor.x + (point.x - anchor.x) * scaleX),
      y: clamp01(anchor.y + (point.y - anchor.y) * scaleY),
    }));
  }

  function movedPoints(start: RegionalPromptSelection, dx: number, dy: number): NormalizedPoint[] | undefined {
    if (!start.points?.length) return undefined;
    return start.points.map((point) => ({
      x: clamp01(point.x + dx),
      y: clamp01(point.y + dy),
    }));
  }

  function updateRegionGeometry(
    regionId: string,
    bounds: Pick<RegionalPromptSelection, "x" | "y" | "width" | "height">,
    points?: NormalizedPoint[],
  ): void {
    draftRegions = draftRegions.map((region) => {
      if (region.id !== regionId) return region;
      return {
        ...region,
        x: clamp01(bounds.x),
        y: clamp01(bounds.y),
        width: clamp01(bounds.width),
        height: clamp01(bounds.height),
        points: points ?? region.points,
      };
    });
  }

  function beginEditGesture(event: PointerEvent, gesture: EditGesture): void {
    event.stopPropagation();
    editGesture = gesture;
    drawingPointerId = event.pointerId;
    canvasEl?.setPointerCapture(event.pointerId);
  }

  function onRegionPointerDown(event: PointerEvent, regionId: string): void {
    if (event.button !== 0) return;
    event.stopPropagation();
    const point = toCanvasPoint(event);
    if (!point) return;

    selectRegion(regionId);
    const region = draftRegions.find((entry) => entry.id === regionId);
    if (!region) return;

    const handle = hitResizeHandle(point, region);
    if (handle) {
      beginEditGesture(event, {
        kind: "resize",
        regionId,
        handle,
        startPoint: point,
        startRegion: cloneRegion(region),
      });
      return;
    }

    if (pointInRegion(point, region)) {
      beginEditGesture(event, {
        kind: "move",
        regionId,
        startPoint: point,
        startRegion: cloneRegion(region),
      });
    }
  }

  function onHandlePointerDown(event: PointerEvent, regionId: string, handle: ResizeHandle): void {
    if (event.button !== 0) return;
    event.stopPropagation();
    const point = toCanvasPoint(event);
    if (!point) return;
    selectRegion(regionId);
    const region = draftRegions.find((entry) => entry.id === regionId);
    if (!region) return;
    beginEditGesture(event, {
      kind: "resize",
      regionId,
      handle,
      startPoint: point,
      startRegion: cloneRegion(region),
    });
  }

  function applyEditGesture(point: NormalizedPoint): void {
    if (!editGesture) return;
    const { kind, regionId, startRegion, handle } = editGesture;

    if (kind === "move") {
      const dx = point.x - editGesture.startPoint.x;
      const dy = point.y - editGesture.startPoint.y;
      const nx = clamp01(Math.min(1 - startRegion.width, startRegion.x + dx));
      const ny = clamp01(Math.min(1 - startRegion.height, startRegion.y + dy));
      updateRegionGeometry(
        regionId,
        { x: nx, y: ny, width: startRegion.width, height: startRegion.height },
        movedPoints(startRegion, nx - startRegion.x, ny - startRegion.y),
      );
      return;
    }

    if (kind === "resize" && handle) {
      const bounds = resizeBoundsFromHandle(handle, startRegion, point);
      const anchor = resizeAnchor(handle, startRegion);
      updateRegionGeometry(regionId, bounds, scaledPoints(startRegion, anchor, bounds));
    }
  }

  function finishEditGesture(event: PointerEvent): boolean {
    if (editGesture === null || drawingPointerId !== event.pointerId) return false;
    editGesture = null;
    drawingPointerId = null;
    try {
      canvasEl?.releasePointerCapture(event.pointerId);
    } catch {
      // pointer may not be captured
    }
    return true;
  }

  function startDrag(event: PointerEvent): void {
    if (event.button !== 0) return;
    const point = toCanvasPoint(event);
    if (!point) return;

    const hitId = hitRegionAtPoint(point);
    if (hitId) {
      onRegionPointerDown(event, hitId);
      return;
    }

    if (activeTool === "select") {
      selectedRegionId = null;
      return;
    }

    if (activeTool === "lasso") {
      drawingPointerId = event.pointerId;
      lassoDrawing = true;
      lassoPoints = [point];
      return;
    }

    drawStart = point;
    drawCurrent = point;
    drawingPointerId = event.pointerId;
  }

  function trackDrag(event: PointerEvent): void {
    if (drawingPointerId === null || drawingPointerId !== event.pointerId) return;
    const point = toCanvasPoint(event);
    if (!point) return;
    if (editGesture) {
      applyEditGesture(point);
      return;
    }
    if (activeTool === "lasso" && lassoDrawing) {
      const last = lassoPoints[lassoPoints.length - 1];
      if (!last || distance(last, point) >= 0.003) {
        lassoPoints = [...lassoPoints, point];
      }
      return;
    }
    drawCurrent = point;
  }

  function finishDrag(event: PointerEvent): void {
    if (drawingPointerId === null || drawingPointerId !== event.pointerId) return;
    if (finishEditGesture(event)) return;
    const point = toCanvasPoint(event);
    if (activeTool === "lasso" && lassoDrawing) {
      drawingPointerId = null;
      lassoDrawing = false;
      if (point) {
        const last = lassoPoints[lassoPoints.length - 1];
        if (!last || distance(last, point) >= 0.003) {
          lassoPoints = [...lassoPoints, point];
        }
      }
      finishLasso();
      return;
    }

    const start = drawStart;
    drawingPointerId = null;
    drawStart = null;
    drawCurrent = null;

    if (!point || !start) return;
    const bounds = boundsFromPoints(start, point);
    if (bounds.width <= 0.01 || bounds.height <= 0.01) return;

    addRegion(createRegion(activeTool as RegionalPromptShape, bounds));
  }

  function finishLasso(): void {
    if (lassoPoints.length < 3) {
      lassoPoints = [];
      return;
    }
    const xs = lassoPoints.map((point) => point.x);
    const ys = lassoPoints.map((point) => point.y);
    const minX = Math.min(...xs);
    const maxX = Math.max(...xs);
    const minY = Math.min(...ys);
    const maxY = Math.max(...ys);
    const width = maxX - minX;
    const height = maxY - minY;
    if (width <= 0.01 || height <= 0.01) {
      lassoPoints = [];
      return;
    }

    addRegion(
      createRegion(
        "lasso",
        { x: minX, y: minY, width, height },
        lassoPoints,
      ),
    );
    lassoPoints = [];
  }

  function cancelLasso(): void {
    lassoDrawing = false;
    drawingPointerId = null;
    lassoPoints = [];
  }

  function selectRegion(regionId: string): void {
    selectedRegionId = regionId;
  }

  function updateSelectedText(text: string): void {
    if (!selectedRegionId) return;
    draftRegions = draftRegions.map((region) => (
      region.id === selectedRegionId ? { ...region, text } : region
    ));
  }

  function updateSelectedStrength(value: string): void {
    if (!selectedRegionId) return;
    const parsed = Number.parseFloat(value);
    const strength = clampStrength(parsed);
    draftRegions = draftRegions.map((region) => (
      region.id === selectedRegionId ? { ...region, strength } : region
    ));
  }

  function removeRegion(regionId: string): void {
    const next = draftRegions.filter((region) => region.id !== regionId);
    draftRegions = next;
    if (selectedRegionId !== regionId) return;
    selectedRegionId = next[0]?.id ?? null;
  }

  function moveRegion(regionId: string, direction: -1 | 1): void {
    const currentIndex = draftRegions.findIndex((region) => region.id === regionId);
    if (currentIndex < 0) return;
    const targetIndex = currentIndex + direction;
    if (targetIndex < 0 || targetIndex >= draftRegions.length) return;
    const next = [...draftRegions];
    const [entry] = next.splice(currentIndex, 1);
    next.splice(targetIndex, 0, entry);
    draftRegions = next;
  }

  function duplicateRegion(regionId: string): void {
    const currentIndex = draftRegions.findIndex((region) => region.id === regionId);
    if (currentIndex < 0) return;
    const source = draftRegions[currentIndex];
    const duplicate: RegionalPromptSelection = {
      ...source,
      id: generateRegionId(),
      x: Math.min(1, source.x + 0.02),
      y: Math.min(1, source.y + 0.02),
      points: source.points?.map((point) => ({
        x: clamp01(point.x + 0.02),
        y: clamp01(point.y + 0.02),
      })),
    };
    const next = [...draftRegions];
    next.splice(currentIndex + 1, 0, duplicate);
    draftRegions = next;
    selectedRegionId = duplicate.id;
  }

  function commitDraftToStore(): void {
    generation.regionalPrompts = draftRegions.map((region) => ({
      ...region,
      x: clamp01(region.x),
      y: clamp01(region.y),
      width: clamp01(region.width),
      height: clamp01(region.height),
      strength: clampStrength(region.strength),
      points: region.points?.map((point) => ({ x: clamp01(point.x), y: clamp01(point.y) })),
    }));
    generation.saveSettings();
  }

  function closeWithDraftSaved(): void {
    commitDraftToStore();
    onclose();
  }

  function handleEscape(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    event.preventDefault();
    closeWithDraftSaved();
  }

  function saveRegions(): void {
    commitDraftToStore();
    onclose();
  }

  function toolLabel(tool: CanvasTool): string {
    if (tool === "select") return locale.t("generation.regional.tool_select");
    if (tool === "box") return locale.t("generation.regional.shape_box");
    if (tool === "circle") return locale.t("generation.regional.shape_circle");
    return locale.t("generation.regional.shape_lasso");
  }

  function shapeLabel(shape: RegionalPromptShape): string {
    if (shape === "box") return locale.t("generation.regional.shape_box");
    if (shape === "circle") return locale.t("generation.regional.shape_circle");
    return locale.t("generation.regional.shape_lasso");
  }

  function snapCoord01(value: number, dimensionPx: number): number {
    const snappedPx = Math.max(0, Math.round((clamp01(value) * dimensionPx) / 8) * 8);
    return clamp01(snappedPx / dimensionPx);
  }

  function snapSize01(value: number, dimensionPx: number): number {
    const snappedPx = Math.max(8, Math.round((Math.max(0, value) * dimensionPx) / 8) * 8);
    return Math.min(1, snappedPx / dimensionPx);
  }

  function snappedBounds(bounds: Pick<RegionalPromptSelection, "x" | "y" | "width" | "height">): {
    x: number;
    y: number;
    width: number;
    height: number;
  } {
    const x = snapCoord01(bounds.x, canvasWidthPx);
    const y = snapCoord01(bounds.y, canvasHeightPx);
    const maxWidth = Math.max(0, 1 - x);
    const maxHeight = Math.max(0, 1 - y);
    const width = Math.min(maxWidth, snapSize01(bounds.width, canvasWidthPx));
    const height = Math.min(maxHeight, snapSize01(bounds.height, canvasHeightPx));
    return { x, y, width, height };
  }

  function pointsToSvg(points: NormalizedPoint[] | undefined): string {
    if (!points || points.length === 0) return "";
    return points.map((point) => `${point.x},${point.y}`).join(" ");
  }
</script>

<svelte:window onkeydown={handleEscape} onpointerup={finishDrag} onpointermove={trackDrag} onresize={handleViewportResize} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="fixed inset-0 z-80 flex items-center justify-center bg-black/70 p-4"
  onclick={(event) => { if (event.target === event.currentTarget) closeWithDraftSaved(); }}
  role="dialog"
  aria-modal="true"
  tabindex="-1"
>
  <div
    class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-2xl border border-neutral-700 bg-neutral-900 shadow-2xl"
    style:width="{canvasLayout.modalW}px"
    style:max-width="96vw"
    style:max-height="92vh"
  >
    <div class="flex shrink-0 items-center justify-between gap-3 border-b border-neutral-700 px-5 py-4">
      <div>
        <h2 class="text-base font-semibold text-neutral-100">{locale.t("generation.regional.title")}</h2>
        <p class="text-xs text-neutral-400">
          {locale.t("generation.regional.subtitle")}
          <span class="text-neutral-500"> · {generation.width}×{generation.height}</span>
        </p>
        <p class="mt-1 text-[11px] text-neutral-500">{locale.t("generation.regional.cohesion_tip")}</p>
        {#if generation.canChooseRegionalStrategy}
          <div class="mt-2 flex flex-wrap items-center gap-2">
            <span class="text-[11px] text-neutral-400">{locale.t("generation.regional.strategy_label")}</span>
            <button
              type="button"
              onclick={() => { generation.regionalPromptStrategy = "conditioning"; }}
              class="rounded-lg border px-2 py-0.5 text-[11px] transition-colors {generation.regionalPromptStrategy === 'conditioning' ? 'border-indigo-400 bg-indigo-500/20 text-indigo-200' : 'border-neutral-600 bg-neutral-900 text-neutral-300 hover:border-neutral-500'}"
            >
              {locale.t("generation.regional.strategy_conditioning")}
            </button>
            <button
              type="button"
              onclick={() => { generation.regionalPromptStrategy = "inpaint_chain"; }}
              class="rounded-lg border px-2 py-0.5 text-[11px] transition-colors {generation.regionalPromptStrategy === 'inpaint_chain' ? 'border-indigo-400 bg-indigo-500/20 text-indigo-200' : 'border-neutral-600 bg-neutral-900 text-neutral-300 hover:border-neutral-500'}"
            >
              {locale.t("generation.regional.strategy_inpaint")}
            </button>
          </div>
        {:else if generation.isAnima}
          <p class="mt-1 text-[11px] text-cyan-300/90">{locale.t("generation.regional.anima_inpaint_note")}</p>
        {/if}
        {#if generation.effectiveRegionalStrategy === "conditioning"}
          <p class="text-[11px] text-amber-400/80">{locale.t("generation.regional.shape_backend_note")}</p>
        {:else}
          <p class="text-[11px] text-cyan-300/90">{locale.t("generation.regional.inpaint_shape_note")}</p>
        {/if}
      </div>
      <button
        type="button"
        onclick={closeWithDraftSaved}
        class="flex h-8 w-8 items-center justify-center rounded-lg text-neutral-400 transition-colors hover:bg-neutral-700 hover:text-neutral-200"
        aria-label={locale.t("common.close")}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M18 6L6 18"></path>
          <path d="M6 6l12 12"></path>
        </svg>
      </button>
    </div>

    <div class="grid min-h-0 flex-1 gap-3 overflow-y-auto overflow-x-hidden p-4 {canvasLayout.sideBySide ? 'lg:grid-cols-[auto_minmax(280px,320px)]' : 'grid-cols-1'}">
      <div class="flex min-h-0 min-w-0 flex-col gap-3">
        <div class="flex shrink-0 flex-wrap items-center gap-2 rounded-xl border border-neutral-700 bg-neutral-800 px-3 py-2">
          <span class="text-xs text-neutral-400">{locale.t("generation.regional.tool")}</span>
          {#each (["select", "box", "circle", "lasso"] as CanvasTool[]) as tool (tool)}
            <button
              type="button"
              onclick={() => {
                activeTool = tool;
                if (tool !== "lasso") cancelLasso();
              }}
              class="rounded-lg border px-2.5 py-1 text-xs transition-colors {activeTool === tool ? 'border-indigo-400 bg-indigo-500/20 text-indigo-200' : 'border-neutral-600 bg-neutral-900 text-neutral-300 hover:border-neutral-500'}"
            >
              {toolLabel(tool)}
            </button>
          {/each}
          {#if activeTool === "select"}
            <p class="ml-auto text-[11px] text-neutral-400">{locale.t("generation.regional.select_hint")}</p>
          {/if}
          {#if activeTool === "lasso" && lassoDrawing}
            <div class="ml-auto flex items-center gap-2">
              <button
                type="button"
                onclick={cancelLasso}
                class="rounded-lg border border-neutral-600 bg-neutral-900 px-2.5 py-1 text-xs text-neutral-200 transition-colors hover:border-neutral-500"
              >
                {locale.t("generation.regional.cancel_lasso")}
              </button>
            </div>
          {:else if activeTool === "lasso"}
            <p class="ml-auto text-[11px] text-neutral-400">{locale.t("generation.regional.lasso_hint")}</p>
          {/if}
          <label class="ml-auto inline-flex items-center gap-1.5 text-[11px] text-neutral-300">
            <input
              type="checkbox"
              bind:checked={snapPreview}
              class="h-3.5 w-3.5 rounded border-neutral-600 bg-neutral-900 accent-indigo-500"
            />
            {locale.t("generation.regional.snap_toggle")}
          </label>
        </div>
        {#if snapPreview}
          <p class="shrink-0 text-[10px] text-cyan-300/80">{locale.t("generation.regional.snap_hint")}</p>
        {/if}

        <div class="relative shrink-0 rounded-xl border border-neutral-700 bg-neutral-950 p-3">
          <div
            bind:this={canvasEl}
            class="relative rounded-lg border border-neutral-700 bg-neutral-900 {activeTool === 'select' ? 'cursor-default' : 'cursor-crosshair'}"
            style:width="{canvasLayout.canvasW}px"
            style:height="{canvasLayout.canvasH}px"
            onpointerdown={startDrag}
            onpointercancel={finishDrag}
            role="application"
            aria-label={locale.t("generation.regional.canvas_aria")}
          >
            <svg class="absolute inset-0 h-full w-full select-none touch-none" viewBox="0 0 1 1" preserveAspectRatio="none">
              {#each draftRegions as region (region.id)}
                {@const selected = region.id === selectedRegionId}
                {@const snapped = snappedBounds(region)}
                {#if region.shape === "box"}
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <rect
                    x={region.x}
                    y={region.y}
                    width={region.width}
                    height={region.height}
                    fill={selected ? "rgba(99,102,241,0.18)" : "rgba(163,163,163,0.12)"}
                    stroke={selected ? "#818CF8" : "#A3A3A3"}
                    stroke-width={selected ? 0.007 : 0.004}
                    onpointerdown={(event) => onRegionPointerDown(event, region.id)}
                  />
                {:else if region.shape === "circle"}
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <ellipse
                    cx={region.x + region.width / 2}
                    cy={region.y + region.height / 2}
                    rx={region.width / 2}
                    ry={region.height / 2}
                    fill={selected ? "rgba(99,102,241,0.18)" : "rgba(163,163,163,0.12)"}
                    stroke={selected ? "#818CF8" : "#A3A3A3"}
                    stroke-width={selected ? 0.007 : 0.004}
                    onpointerdown={(event) => onRegionPointerDown(event, region.id)}
                  />
                {:else if region.points && region.points.length > 2}
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <polygon
                    points={pointsToSvg(region.points)}
                    fill={selected ? "rgba(99,102,241,0.18)" : "rgba(163,163,163,0.12)"}
                    stroke={selected ? "#818CF8" : "#A3A3A3"}
                    stroke-width={selected ? 0.007 : 0.004}
                    onpointerdown={(event) => onRegionPointerDown(event, region.id)}
                  />
                {/if}
                {#if selected}
                  {#each getResizeHandles(region) as handle (handle.id)}
                    <!-- svelte-ignore a11y_no_static_element_interactions -->
                    <circle
                      cx={handle.x}
                      cy={handle.y}
                      r={HANDLE_VIS_RADIUS}
                      fill="#E0E7FF"
                      stroke="#818CF8"
                      stroke-width="0.003"
                      onpointerdown={(event) => onHandlePointerDown(event, region.id, handle.id)}
                    />
                  {/each}
                {/if}
                {#if snapPreview}
                  <rect
                    x={snapped.x}
                    y={snapped.y}
                    width={snapped.width}
                    height={snapped.height}
                    fill="none"
                    stroke={selected ? "#22D3EE" : "rgba(34,211,238,0.55)"}
                    stroke-width={selected ? 0.006 : 0.0035}
                    stroke-dasharray="0.012 0.007"
                  />
                {/if}
              {/each}

              {#if draftShape}
                {@const snappedDraft = snappedBounds(draftShape)}
                {#if activeTool === "box"}
                  <rect
                    x={draftShape.x}
                    y={draftShape.y}
                    width={draftShape.width}
                    height={draftShape.height}
                    fill="rgba(79,70,229,0.18)"
                    stroke="#A5B4FC"
                    stroke-width="0.004"
                    stroke-dasharray="0.012 0.008"
                  />
                {:else if activeTool === "circle"}
                  <ellipse
                    cx={draftShape.x + draftShape.width / 2}
                    cy={draftShape.y + draftShape.height / 2}
                    rx={draftShape.width / 2}
                    ry={draftShape.height / 2}
                    fill="rgba(79,70,229,0.18)"
                    stroke="#A5B4FC"
                    stroke-width="0.004"
                    stroke-dasharray="0.012 0.008"
                  />
                {/if}
                {#if snapPreview}
                  <rect
                    x={snappedDraft.x}
                    y={snappedDraft.y}
                    width={snappedDraft.width}
                    height={snappedDraft.height}
                    fill="none"
                    stroke="#22D3EE"
                    stroke-width="0.0045"
                    stroke-dasharray="0.012 0.007"
                  />
                {/if}
              {/if}

              {#if lassoPoints.length > 0}
                <polyline
                  points={pointsToSvg(lassoPoints)}
                  fill="none"
                  stroke="#A5B4FC"
                  stroke-width="0.005"
                  stroke-dasharray="0.01 0.006"
                />
                {#each lassoPoints as point, pointIndex (`${point.x}-${point.y}-${pointIndex}`)}
                  <circle cx={point.x} cy={point.y} r="0.006" fill="#A5B4FC" />
                {/each}
              {/if}
            </svg>
          </div>
        </div>
      </div>

      <div class="flex min-h-0 min-w-0 flex-col gap-3 overflow-hidden rounded-xl border border-neutral-700 bg-neutral-900 p-3 {canvasLayout.sideBySide ? 'max-h-full' : 'max-h-[min(40vh,320px)]'}">
        <div class="flex shrink-0 items-center justify-between gap-2">
          <h3 class="text-sm font-semibold text-neutral-200">{locale.t("generation.regional.regions_title", { count: String(draftRegions.length) })}</h3>
        </div>

        <div class="min-h-0 flex-1 space-y-2 overflow-y-auto overscroll-contain pr-1">
          {#if draftRegions.length === 0}
            <p class="rounded-lg border border-dashed border-neutral-700 px-3 py-2 text-xs text-neutral-500">
              {locale.t("generation.regional.empty")}
            </p>
          {/if}

          {#each draftRegions as region, index (region.id)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              onclick={() => selectRegion(region.id)}
              onkeydown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  selectRegion(region.id);
                }
              }}
              role="button"
              tabindex="0"
              class="w-full rounded-lg border px-2.5 py-2 text-left transition-colors {region.id === selectedRegionId ? 'border-indigo-500 bg-indigo-500/15' : 'border-neutral-700 bg-neutral-900 hover:border-neutral-600'}"
            >
              <div class="flex items-center justify-between gap-2">
                <span class="text-xs font-medium text-neutral-200">{locale.t("generation.regional.region_label", { index: String(index + 1), shape: shapeLabel(region.shape) })}</span>
                <span class="text-[11px] text-neutral-400">x{region.strength.toFixed(2)}</span>
              </div>
              <p class="mt-1 line-clamp-2 text-[11px] text-neutral-400">{region.text || locale.t("generation.regional.no_prompt")}</p>
              <div class="mt-1.5 flex items-center gap-1">
                <button
                  type="button"
                  class="rounded border border-neutral-700 px-1.5 py-0.5 text-[10px] text-neutral-300 hover:border-neutral-500"
                  onclick={(event) => { event.stopPropagation(); duplicateRegion(region.id); }}
                >
                  {locale.t("generation.regional.duplicate")}
                </button>
                <button
                  type="button"
                  class="rounded border border-neutral-700 px-1.5 py-0.5 text-[10px] text-neutral-300 hover:border-neutral-500 disabled:opacity-40"
                  disabled={index === 0}
                  onclick={(event) => { event.stopPropagation(); moveRegion(region.id, -1); }}
                >
                  {locale.t("generation.regional.move_up")}
                </button>
                <button
                  type="button"
                  class="rounded border border-neutral-700 px-1.5 py-0.5 text-[10px] text-neutral-300 hover:border-neutral-500 disabled:opacity-40"
                  disabled={index === draftRegions.length - 1}
                  onclick={(event) => { event.stopPropagation(); moveRegion(region.id, 1); }}
                >
                  {locale.t("generation.regional.move_down")}
                </button>
              </div>
            </div>
          {/each}
        </div>

        {#if selectedRegion}
          <div class="shrink-0 space-y-2 rounded-lg border border-neutral-700 bg-neutral-900 p-3">
            <label class="block text-xs text-neutral-400">
              {locale.t("generation.regional.prompt_text")}
              <textarea
                value={selectedRegion.text}
                oninput={(event) => updateSelectedText((event.currentTarget as HTMLTextAreaElement).value)}
                class="mt-1 h-20 w-full resize-none rounded-lg border border-neutral-700 bg-neutral-950 px-2 py-1.5 text-xs text-neutral-100 outline-none transition-colors focus:border-indigo-500"
                placeholder={locale.t("generation.regional.prompt_placeholder")}
              ></textarea>
            </label>

            <label class="block text-xs text-neutral-400">
              {locale.t("generation.regional.strength", { value: selectedRegion.strength.toFixed(2) })}
              <input
                type="range"
                min="0"
                max="2"
                step="0.05"
                value={selectedRegion.strength}
                oninput={(event) => updateSelectedStrength((event.currentTarget as HTMLInputElement).value)}
                class="mt-1 w-full accent-indigo-500"
              />
              <input
                type="number"
                min="0"
                max="2"
                step="0.05"
                value={selectedRegion.strength}
                oninput={(event) => updateSelectedStrength((event.currentTarget as HTMLInputElement).value)}
                class="mt-1 w-full rounded-lg border border-neutral-700 bg-neutral-950 px-2 py-1 text-xs text-neutral-100 outline-none transition-colors focus:border-indigo-500"
              />
            </label>

            <button
              type="button"
              onclick={() => removeRegion(selectedRegion.id)}
              class="w-full rounded-lg border border-red-500/40 bg-red-500/10 px-2.5 py-1.5 text-xs text-red-200 transition-colors hover:border-red-500/60 hover:bg-red-500/20"
            >
              {locale.t("generation.regional.delete_selected")}
            </button>
          </div>
        {/if}
      </div>
    </div>

    <div class="flex shrink-0 items-center justify-end gap-2 border-t border-neutral-700 px-4 py-3">
      <button
        type="button"
        onclick={onclose}
        class="rounded-lg border border-neutral-600 bg-neutral-900 px-3 py-1.5 text-sm text-neutral-200 transition-colors hover:border-neutral-500"
      >
        {locale.t("common.cancel")}
      </button>
      <button
        type="button"
        onclick={saveRegions}
        class="rounded-lg bg-indigo-600 px-3 py-1.5 text-sm text-white transition-colors hover:bg-indigo-500"
      >
        {locale.t("generation.regional.save_regions")}
      </button>
    </div>
  </div>
</div>

