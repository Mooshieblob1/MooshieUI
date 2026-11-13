<script lang="ts">
  import Hand from "@lucide/svelte/icons/hand";
  import Search from "@lucide/svelte/icons/search";
  import Crop from "@lucide/svelte/icons/crop";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import Redo2 from "@lucide/svelte/icons/redo-2";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw-square";
  import RotateCw from "@lucide/svelte/icons/rotate-cw-square";
  import FlipH from "@lucide/svelte/icons/square-centerline-dashed-horizontal";
  import FlipV from "@lucide/svelte/icons/square-centerline-dashed-vertical";
  import Scaling from "@lucide/svelte/icons/scaling";
  import Download from "@lucide/svelte/icons/download";
  import Save from "@lucide/svelte/icons/save";
  import X from "@lucide/svelte/icons/x";
  import LinkIcon from "@lucide/svelte/icons/link";
  import Unlink from "@lucide/svelte/icons/unlink";
  import { portal } from "../../utils/portal.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { imageEditor } from "../../stores/imageEditor.svelte.js";
  import type { ImageEditorSource } from "../../stores/imageEditor.svelte.js";
  import {
    embedImageMetadataBytes,
    loadGalleryImagePng,
    readImageMetadata,
    readImageMetadataBytes,
    saveToGalleryBytes,
  } from "../../utils/api.js";
  import {
    ADJUSTMENT_KEYS,
    NEUTRAL_ADJUSTMENTS,
    applyAdjustments,
    curveLut,
    isIdentityCurve,
    normalizeAdjustments,
    dragCropRect,
    editedMetadata,
    fitAspectRect,
    isNeutral,
    roundRect,
    sameAdjustments,
  } from "../../utils/imageEditorOps.js";
  import type { AdjustmentKey, Adjustments, CropHandle, CurvePoint, EditOp, Rect } from "../../utils/imageEditorOps.js";
  import type { GenerationMode } from "../../types/index.js";

  type Tool = "hand" | "zoom" | "crop";

  interface HistoryEntry {
    canvas: HTMLCanvasElement;
    adjustments: Adjustments;
    ops: EditOp[];
  }

  /** Longest side of the buffer the sliders are previewed on. */
  const PREVIEW_MAX = 2048;
  const MAX_DIMENSION = 16384;
  const MAX_HISTORY = 30;
  const MIN_ZOOM = 0.02;
  const MAX_ZOOM = 32;
  /** Hit radius around a crop handle, in screen pixels. */
  const HANDLE_HIT = 10;

  const CROP_PRESETS: { key: string; label: string; ratio: number | "original" | null }[] = [
    { key: "free", label: "", ratio: null },
    { key: "original", label: "", ratio: "original" },
    { key: "1:1", label: "1:1", ratio: 1 },
    { key: "4:5", label: "4:5", ratio: 4 / 5 },
    { key: "2:3", label: "2:3", ratio: 2 / 3 },
    { key: "3:2", label: "3:2", ratio: 3 / 2 },
    { key: "16:9", label: "16:9", ratio: 16 / 9 },
    { key: "9:16", label: "9:16", ratio: 9 / 16 },
  ];

  type Slider = { key: AdjustmentKey; min: number; max: number };
  const range = (key: AdjustmentKey, min = -100, max = 100): Slider => ({ key, min, max });
  /** Panel sections, top to bottom. The tone curve sits after "light". */
  const GROUPS: { key: string; sliders: Slider[] }[] = [
    { key: "light", sliders: [range("brightness"), range("contrast"), range("highlights"), range("shadows")] },
    {
      key: "color",
      sliders: [range("temperature"), range("tint"), range("saturation"), range("vibrance"), range("hue", -180, 180)],
    },
    { key: "detail", sliders: [range("sharpen", 0), range("blur", 0)] },
    { key: "effects", sliders: [range("vignette")] },
  ];

  const PRESETS_KEY = "mooshieui.editor.presets.v1";
  const MAX_PRESETS = 24;
  /** How close (in curve units, 0..255) a press must be to grab a point. */
  const CURVE_GRAB = 14;

  const source = $derived(imageEditor.source);

  let stageEl = $state<HTMLDivElement | null>(null);
  let viewEl = $state<HTMLCanvasElement | null>(null);

  let phase = $state<"loading" | "ready" | "error">("loading");
  let saving = $state(false);
  let error = $state("");
  let confirmClose = $state(false);

  // The document. Canvases are plain objects, not state: `docVersion` is what
  // the UI reacts to.
  let doc: HTMLCanvasElement | null = null;
  let docW = $state(0);
  let docH = $state(0);
  let docVersion = $state(0);
  let adjustments = $state<Adjustments>({ ...NEUTRAL_ADJUSTMENTS });

  let history: HistoryEntry[] = [];
  let historyIndex = $state(-1);
  let historyLength = $state(0);
  let savedIndex = $state(0);

  let sourceMetadata = $state<Record<string, string> | null>(null);
  let editedFrom: string | null = null;
  let generationMode: GenerationMode | undefined;

  let tool = $state<Tool>("hand");
  let spaceDown = $state(false);
  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let stageW = $state(0);
  let stageH = $state(0);
  let refitOnResize = false;

  let crop = $state<Rect | null>(null);
  let cropPreset = $state("free");
  let cropRatio = $state<number | null>(null);

  let sizeOpen = $state(false);
  let sizeW = $state(0);
  let sizeH = $state(0);
  let sizeLocked = $state(true);

  // Preview buffers for the adjustment sliders.
  let previewBase: HTMLCanvasElement | null = null;
  let previewScale = 1;
  let adjusted: HTMLCanvasElement | null = null;
  let adjustedFor: { version: number; adjustments: Adjustments } | null = null;

  const canUndo = $derived(historyIndex > 0);
  const canRedo = $derived(historyIndex < historyLength - 1);
  const dirty = $derived(historyIndex !== savedIndex);
  const busy = $derived(saving || phase !== "ready");
  const metadataTitle = $derived.by(() => {
    const m = sourceMetadata;
    if (!m) return "";
    return [m.model, m.seed, m.size].filter(Boolean).join(" · ");
  });

  function makeCanvas(width: number, height: number): HTMLCanvasElement {
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(width));
    canvas.height = Math.max(1, Math.round(height));
    return canvas;
  }

  // ------------------------------------------------------------------ load

  async function loadSource(src: ImageEditorSource): Promise<{
    blob: Blob;
    metadata: Record<string, string> | null;
    from: string | null;
  }> {
    const image = src.image;
    const galleryFilename = image ? await gallery.resolveGalleryFilename(image) : null;
    if (galleryFilename) {
      // PNG bytes whatever the gallery format: the backend transcodes JXL.
      const bytes = await loadGalleryImagePng(galleryFilename);
      let metadata: Record<string, string> | null = null;
      try {
        metadata = await readImageMetadata(galleryFilename);
      } catch (e) {
        console.warn("Image editor: could not read metadata:", e);
      }
      return {
        blob: new Blob([new Uint8Array(bytes)], { type: "image/png" }),
        metadata: metadata ?? image?.metadata ?? null,
        from: galleryFilename,
      };
    }

    let blob = image?.sessionBlob ?? null;
    const url = src.url ?? image?.fullImageUrl ?? image?.url ?? null;
    if (!blob) {
      if (!url) throw new Error("no image source");
      const response = await fetch(url);
      if (!response.ok) throw new Error(`fetch failed: ${response.status}`);
      blob = await response.blob();
    }
    let metadata: Record<string, string> | null = image?.metadata ?? null;
    if (!metadata) {
      try {
        metadata = await readImageMetadataBytes(Array.from(new Uint8Array(await blob.arrayBuffer())));
      } catch (e) {
        console.warn("Image editor: could not read metadata:", e);
      }
    }
    return { blob, metadata, from: null };
  }

  $effect(() => {
    const src = source;
    if (!src) return;
    let cancelled = false;

    phase = "loading";
    error = "";
    saving = false;
    confirmClose = false;
    tool = "hand";
    crop = null;
    sizeOpen = false;
    adjustments = { ...NEUTRAL_ADJUSTMENTS };
    doc = null;
    previewBase = null;
    adjusted = null;
    adjustedFor = null;
    history = [];
    historyIndex = -1;
    historyLength = 0;
    sourceMetadata = null;
    editedFrom = null;
    generationMode = src.image?.generation_mode;

    void (async () => {
      try {
        const loaded = await loadSource(src);
        const bitmap = await createImageBitmap(loaded.blob);
        if (cancelled) {
          bitmap.close();
          return;
        }
        const canvas = makeCanvas(bitmap.width, bitmap.height);
        canvas.getContext("2d")!.drawImage(bitmap, 0, 0);
        bitmap.close();
        sourceMetadata = loaded.metadata;
        editedFrom = loaded.from;
        history = [{ canvas, adjustments: { ...NEUTRAL_ADJUSTMENTS }, ops: [] }];
        historyLength = 1;
        historyIndex = 0;
        savedIndex = 0;
        setDoc(canvas);
        phase = "ready";
        fitView(true);
      } catch (e) {
        if (cancelled) return;
        console.error("Image editor: failed to open image:", e);
        phase = "error";
        error = locale.t("editor.load_failed");
      }
    })();

    return () => {
      cancelled = true;
    };
  });

  // --------------------------------------------------------------- history

  function setDoc(canvas: HTMLCanvasElement) {
    doc = canvas;
    docW = canvas.width;
    docH = canvas.height;
    const scale = Math.min(1, PREVIEW_MAX / Math.max(canvas.width, canvas.height));
    previewScale = scale;
    if (scale === 1) {
      previewBase = canvas;
    } else {
      previewBase = makeCanvas(canvas.width * scale, canvas.height * scale);
      const ctx = previewBase.getContext("2d", { willReadFrequently: true })!;
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(canvas, 0, 0, previewBase.width, previewBase.height);
    }
    docVersion++;
  }

  function pushHistory(entry: HistoryEntry) {
    history = [...history.slice(0, historyIndex + 1), entry];
    if (history.length > MAX_HISTORY) {
      const dropped = history.length - MAX_HISTORY;
      history = history.slice(dropped);
      savedIndex -= dropped;
    }
    historyLength = history.length;
    historyIndex = history.length - 1;
  }

  function commitPixels(canvas: HTMLCanvasElement, op: EditOp) {
    const current = history[historyIndex];
    pushHistory({ canvas, adjustments: { ...adjustments }, ops: [...current.ops, op] });
    setDoc(canvas);
  }

  /** Record the sliders' current values as one undo step. */
  function commitAdjustments() {
    const current = history[historyIndex];
    if (!current || sameAdjustments(current.adjustments, adjustments)) return;
    pushHistory({ canvas: current.canvas, adjustments: { ...adjustments }, ops: current.ops });
  }

  function restore(index: number) {
    const entry = history[index];
    if (!entry) return;
    historyIndex = index;
    adjustments = { ...entry.adjustments };
    crop = null;
    if (tool === "crop") tool = "hand";
    if (entry.canvas !== doc) setDoc(entry.canvas);
  }

  function undo() {
    if (canUndo && !busy) restore(historyIndex - 1);
  }

  function redo() {
    if (canRedo && !busy) restore(historyIndex + 1);
  }

  // ------------------------------------------------------------ tone curve

  let curveEl = $state<SVGSVGElement | null>(null);
  let curveDrag: number | null = null;

  const curvePath = $derived.by(() => {
    const lut = curveLut(adjustments.curve);
    let d = "";
    for (let x = 0; x < 256; x++) d += `${x === 0 ? "M" : "L"}${x} ${255 - lut[x]}`;
    return d;
  });

  /** Pointer position in curve units. The SVG's viewBox is -6..261 on both axes. */
  function curvePoint(e: MouseEvent): CurvePoint {
    const rect = curveEl!.getBoundingClientRect();
    const x = ((e.clientX - rect.left) / Math.max(1, rect.width)) * 267 - 6;
    const y = 255 - (((e.clientY - rect.top) / Math.max(1, rect.height)) * 267 - 6);
    return [Math.round(Math.min(255, Math.max(0, x))), Math.round(Math.min(255, Math.max(0, y)))];
  }

  function moveCurvePoint(index: number, [x, y]: CurvePoint) {
    const points = adjustments.curve;
    const last = points.length - 1;
    // The end points slide up and down only; inner points stay between
    // their neighbours so the curve remains a function of x.
    const nx = index === 0 ? 0 : index === last ? 255 : Math.min(points[index + 1][0] - 1, Math.max(points[index - 1][0] + 1, x));
    adjustments.curve = points.map((p, i) => (i === index ? [nx, y] : p) as CurvePoint);
  }

  function onCurveDown(e: PointerEvent) {
    if (busy || !curveEl || e.button !== 0) return;
    const [x, y] = curvePoint(e);
    const points = adjustments.curve;
    let index = points.findIndex((p) => Math.abs(p[0] - x) <= CURVE_GRAB && Math.abs(p[1] - y) <= CURVE_GRAB);
    if (index < 0) {
      if (x <= 0 || x >= 255 || points.some((p) => p[0] === x)) return;
      index = points.findIndex((p) => p[0] > x);
      adjustments.curve = [...points.slice(0, index), [x, y] as CurvePoint, ...points.slice(index)];
    }
    curveDrag = index;
    moveCurvePoint(index, [x, y]);
    curveEl.setPointerCapture(e.pointerId);
    e.preventDefault();
  }

  function onCurveMove(e: PointerEvent) {
    if (curveDrag === null) return;
    moveCurvePoint(curveDrag, curvePoint(e));
  }

  function onCurveUp(e: PointerEvent) {
    if (curveDrag === null) return;
    curveDrag = null;
    if (curveEl?.hasPointerCapture(e.pointerId)) curveEl.releasePointerCapture(e.pointerId);
    commitAdjustments();
  }

  /** Double-click removes an inner point. */
  function onCurveDblClick(e: MouseEvent) {
    if (busy || !curveEl) return;
    const [x, y] = curvePoint(e);
    const points = adjustments.curve;
    const index = points.findIndex((p) => Math.abs(p[0] - x) <= CURVE_GRAB && Math.abs(p[1] - y) <= CURVE_GRAB);
    if (index <= 0 || index >= points.length - 1) return;
    adjustments.curve = points.filter((_, i) => i !== index);
    commitAdjustments();
  }

  function resetCurve() {
    adjustments.curve = NEUTRAL_ADJUSTMENTS.curve;
    commitAdjustments();
  }

  // --------------------------------------------------------------- presets

  interface Preset {
    name: string;
    adjustments: Adjustments;
  }

  function loadPresets(): Preset[] {
    try {
      const parsed: unknown = JSON.parse(localStorage.getItem(PRESETS_KEY) ?? "[]");
      if (!Array.isArray(parsed)) return [];
      return parsed
        .filter((p): p is { name: string; adjustments: unknown } => !!p && typeof p.name === "string" && p.name.trim() !== "")
        .slice(0, MAX_PRESETS)
        .map((p) => ({ name: p.name.trim().slice(0, 40), adjustments: normalizeAdjustments(p.adjustments) }));
    } catch {
      return [];
    }
  }

  let presets = $state<Preset[]>(loadPresets());
  let presetNaming = $state(false);
  let presetName = $state("");

  function storePresets(next: Preset[]) {
    presets = next;
    try {
      localStorage.setItem(PRESETS_KEY, JSON.stringify(next));
    } catch (e) {
      console.warn("Image editor: could not store presets:", e);
    }
  }

  function savePreset() {
    const name = presetName.trim().slice(0, 40);
    if (!name) return;
    const entry: Preset = { name, adjustments: normalizeAdjustments($state.snapshot(adjustments)) };
    // Saving under an existing name replaces it.
    storePresets([entry, ...presets.filter((p) => p.name !== name)].slice(0, MAX_PRESETS));
    presetNaming = false;
    presetName = "";
  }

  function applyPreset(preset: Preset) {
    if (busy) return;
    adjustments = normalizeAdjustments(preset.adjustments);
    commitAdjustments();
  }

  function focusOnMount(node: HTMLInputElement) {
    node.focus();
  }

  // ------------------------------------------------------------- pixel ops

  function rotate(deg: 90 | -90) {
    if (!doc || busy) return;
    const out = makeCanvas(doc.height, doc.width);
    const ctx = out.getContext("2d")!;
    ctx.translate(out.width / 2, out.height / 2);
    ctx.rotate((deg * Math.PI) / 180);
    ctx.drawImage(doc, -doc.width / 2, -doc.height / 2);
    cancelCrop();
    commitPixels(out, { op: "rotate", deg });
    fitView(true);
  }

  function flip(axis: "h" | "v") {
    if (!doc || busy) return;
    const out = makeCanvas(doc.width, doc.height);
    const ctx = out.getContext("2d")!;
    if (axis === "h") {
      ctx.translate(out.width, 0);
      ctx.scale(-1, 1);
    } else {
      ctx.translate(0, out.height);
      ctx.scale(1, -1);
    }
    ctx.drawImage(doc, 0, 0);
    cancelCrop();
    commitPixels(out, { op: "flip", axis });
  }

  function startCrop() {
    if (!doc || busy) return;
    tool = "crop";
    sizeOpen = false;
    // The options bar takes height from the stage; keep every handle on screen.
    refitOnResize = true;
    setCropPreset(cropPreset);
  }

  function setCropPreset(key: string) {
    const preset = CROP_PRESETS.find((p) => p.key === key) ?? CROP_PRESETS[0];
    cropPreset = preset.key;
    cropRatio = preset.ratio === "original" ? docW / docH : preset.ratio;
    crop = fitAspectRect(docW, docH, cropRatio);
  }

  function cancelCrop() {
    crop = null;
    if (tool === "crop") tool = "hand";
  }

  function applyCrop() {
    if (!doc || !crop || busy) return;
    const rect = roundRect(crop, docW, docH);
    if (rect.w === docW && rect.h === docH) {
      cancelCrop();
      return;
    }
    const out = makeCanvas(rect.w, rect.h);
    out.getContext("2d")!.drawImage(doc, rect.x, rect.y, rect.w, rect.h, 0, 0, rect.w, rect.h);
    crop = null;
    tool = "hand";
    commitPixels(out, { op: "crop", w: rect.w, h: rect.h });
    fitView(true);
  }

  function openSize() {
    if (!doc || busy) return;
    cancelCrop();
    sizeW = docW;
    sizeH = docH;
    sizeLocked = true;
    sizeOpen = true;
  }

  function onSizeInput(which: "w" | "h") {
    if (!sizeLocked || !docW || !docH) return;
    if (which === "w") sizeH = Math.max(1, Math.round((sizeW * docH) / docW));
    else sizeW = Math.max(1, Math.round((sizeH * docW) / docH));
  }

  function applySize() {
    if (!doc || busy) return;
    const w = Math.min(MAX_DIMENSION, Math.max(1, Math.round(Number(sizeW) || 0)));
    const h = Math.min(MAX_DIMENSION, Math.max(1, Math.round(Number(sizeH) || 0)));
    sizeOpen = false;
    if (w === docW && h === docH) return;
    // Halve repeatedly on big reductions: one drawImage step samples too few
    // source pixels and aliases.
    let current: HTMLCanvasElement = doc;
    while (current.width / 2 >= w && current.height / 2 >= h) {
      const half = makeCanvas(current.width / 2, current.height / 2);
      const ctx = half.getContext("2d")!;
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(current, 0, 0, half.width, half.height);
      current = half;
    }
    const out = makeCanvas(w, h);
    const ctx = out.getContext("2d")!;
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(current, 0, 0, w, h);
    commitPixels(out, { op: "resize", w, h });
    fitView(true);
  }

  // ------------------------------------------------------------------ view

  function fitZoom(): number {
    if (!docW || !docH || !stageW || !stageH) return 1;
    return Math.min((stageW - 48) / docW, (stageH - 48) / docH);
  }

  /** Centre the image. `noUpscale` keeps a small image at 100%. */
  function fitView(noUpscale = false) {
    const fit = fitZoom();
    zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, noUpscale ? Math.min(1, fit) : fit));
    panX = (stageW - docW * zoom) / 2;
    panY = (stageH - docH * zoom) / 2;
  }

  function zoomTo(next: number, cx = stageW / 2, cy = stageH / 2) {
    const clamped = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, next));
    const ix = (cx - panX) / zoom;
    const iy = (cy - panY) / zoom;
    zoom = clamped;
    panX = cx - ix * clamped;
    panY = cy - iy * clamped;
  }

  function trackStage(node: HTMLDivElement) {
    const measure = () => {
      const first = stageW === 0;
      stageW = node.clientWidth;
      stageH = node.clientHeight;
      if (phase !== "ready") return;
      if (first || refitOnResize) fitView(true);
      refitOnResize = false;
    };
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    measure();
    // Not passive, so the page behind never scrolls or pinch-zooms.
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      if (phase !== "ready") return;
      const rect = node.getBoundingClientRect();
      zoomTo(zoom * Math.exp(-e.deltaY * 0.0015), e.clientX - rect.left, e.clientY - rect.top);
    };
    node.addEventListener("wheel", onWheel, { passive: false });
    return {
      destroy() {
        observer.disconnect();
        node.removeEventListener("wheel", onWheel);
      },
    };
  }

  /** The canvas shown on screen: the document, or its adjusted preview. */
  function viewSource(): HTMLCanvasElement | null {
    if (!doc) return null;
    if (isNeutral(adjustments) || !previewBase) return doc;
    if (
      adjusted &&
      adjustedFor &&
      adjustedFor.version === docVersion &&
      sameAdjustments(adjustedFor.adjustments, adjustments)
    ) {
      return adjusted;
    }
    const ctx = previewBase.getContext("2d", { willReadFrequently: true })!;
    const data = ctx.getImageData(0, 0, previewBase.width, previewBase.height);
    applyAdjustments(data.data, data.width, data.height, adjustments, previewScale);
    if (!adjusted || adjusted.width !== data.width || adjusted.height !== data.height) {
      adjusted = makeCanvas(data.width, data.height);
    }
    adjusted.getContext("2d")!.putImageData(data, 0, 0);
    adjustedFor = { version: docVersion, adjustments: { ...adjustments } };
    return adjusted;
  }

  function draw() {
    const canvas = viewEl;
    if (!canvas || !stageW || !stageH) return;
    const dpr = window.devicePixelRatio || 1;
    const width = Math.round(stageW * dpr);
    const height = Math.round(stageH * dpr);
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, stageW, stageH);
    const src = viewSource();
    if (!src) return;

    const w = docW * zoom;
    const h = docH * zoom;
    // Transparency checkerboard under the image.
    ctx.save();
    ctx.beginPath();
    ctx.rect(panX, panY, w, h);
    ctx.clip();
    ctx.fillStyle = "#3f3f46";
    ctx.fillRect(panX, panY, w, h);
    ctx.fillStyle = "#52525b";
    const cell = 12;
    const x0 = Math.max(panX, 0);
    const y0 = Math.max(panY, 0);
    const x1 = Math.min(panX + w, stageW);
    const y1 = Math.min(panY + h, stageH);
    for (let y = y0 - ((y0 - panY) % cell); y < y1; y += cell) {
      for (let x = x0 - ((x0 - panX) % cell); x < x1; x += cell) {
        if ((Math.round((x - panX) / cell) + Math.round((y - panY) / cell)) % 2 === 0) {
          ctx.fillRect(x, y, cell, cell);
        }
      }
    }
    ctx.restore();

    ctx.imageSmoothingEnabled = zoom < 4;
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(src, panX, panY, w, h);

    if (crop) {
      const cx = panX + crop.x * zoom;
      const cy = panY + crop.y * zoom;
      const cw = crop.w * zoom;
      const ch = crop.h * zoom;
      ctx.fillStyle = "rgba(0, 0, 0, 0.6)";
      ctx.beginPath();
      ctx.rect(0, 0, stageW, stageH);
      ctx.rect(cx, cy, cw, ch);
      ctx.fill("evenodd");
      ctx.strokeStyle = "rgba(255, 255, 255, 0.35)";
      ctx.lineWidth = 1;
      ctx.beginPath();
      for (let i = 1; i <= 2; i++) {
        ctx.moveTo(Math.round(cx + (cw * i) / 3) + 0.5, cy);
        ctx.lineTo(Math.round(cx + (cw * i) / 3) + 0.5, cy + ch);
        ctx.moveTo(cx, Math.round(cy + (ch * i) / 3) + 0.5);
        ctx.lineTo(cx + cw, Math.round(cy + (ch * i) / 3) + 0.5);
      }
      ctx.stroke();
      ctx.strokeStyle = "#ffffff";
      ctx.strokeRect(Math.round(cx) + 0.5, Math.round(cy) + 0.5, Math.round(cw), Math.round(ch));
      ctx.fillStyle = "#ffffff";
      for (const [hx, hy] of handlePoints(crop)) {
        ctx.fillRect(Math.round(panX + hx * zoom) - 4, Math.round(panY + hy * zoom) - 4, 8, 8);
      }
    }
  }

  $effect(() => {
    // Everything the picture depends on.
    void docVersion;
    void zoom;
    void panX;
    void panY;
    void stageW;
    void stageH;
    void crop;
    for (const key of ADJUSTMENT_KEYS) void adjustments[key];
    void adjustments.curve;
    if (!viewEl) return;
    const frame = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(frame);
  });

  // --------------------------------------------------------------- pointer

  function handlePoints(rect: Rect): [number, number, CropHandle][] {
    const { x, y, w, h } = rect;
    return [
      [x, y, "nw"],
      [x + w / 2, y, "n"],
      [x + w, y, "ne"],
      [x + w, y + h / 2, "e"],
      [x + w, y + h, "se"],
      [x + w / 2, y + h, "s"],
      [x, y + h, "sw"],
      [x, y + h / 2, "w"],
    ];
  }

  function hitCrop(sx: number, sy: number): CropHandle {
    if (!crop) return "new";
    for (const [hx, hy, handle] of handlePoints(crop)) {
      if (Math.abs(panX + hx * zoom - sx) <= HANDLE_HIT && Math.abs(panY + hy * zoom - sy) <= HANDLE_HIT) {
        return handle;
      }
    }
    const ix = (sx - panX) / zoom;
    const iy = (sy - panY) / zoom;
    if (ix >= crop.x && ix <= crop.x + crop.w && iy >= crop.y && iy <= crop.y + crop.h) return "move";
    return "new";
  }

  const HANDLE_CURSORS: Record<CropHandle, string> = {
    n: "ns-resize",
    s: "ns-resize",
    e: "ew-resize",
    w: "ew-resize",
    ne: "nesw-resize",
    sw: "nesw-resize",
    nw: "nwse-resize",
    se: "nwse-resize",
    move: "move",
    new: "crosshair",
  };

  let drag:
    | { kind: "pan"; x: number; y: number; panX: number; panY: number }
    | { kind: "crop"; handle: CropHandle; x: number; y: number; start: Rect; previous: Rect | null }
    | null = null;
  let dragging = $state(false);
  let hoverCursor = $state("default");

  function stagePoint(e: PointerEvent): [number, number] {
    const rect = stageEl!.getBoundingClientRect();
    return [e.clientX - rect.left, e.clientY - rect.top];
  }

  function onPointerDown(e: PointerEvent) {
    if (phase !== "ready" || !stageEl) return;
    const [sx, sy] = stagePoint(e);
    const panning = spaceDown || e.button === 1 || (tool === "hand" && e.button === 0);
    if (panning) {
      drag = { kind: "pan", x: e.clientX, y: e.clientY, panX, panY };
    } else if (e.button !== 0) {
      return;
    } else if (tool === "zoom") {
      zoomTo(zoom * (e.altKey ? 0.5 : 2), sx, sy);
      return;
    } else if (tool === "crop") {
      const handle = hitCrop(sx, sy);
      const ix = Math.min(docW, Math.max(0, (sx - panX) / zoom));
      const iy = Math.min(docH, Math.max(0, (sy - panY) / zoom));
      drag = {
        kind: "crop",
        handle,
        x: e.clientX,
        y: e.clientY,
        start: handle === "new" || !crop ? { x: ix, y: iy, w: 0, h: 0 } : { ...crop },
        previous: crop ? { ...crop } : null,
      };
    } else {
      return;
    }
    e.preventDefault();
    dragging = true;
    stageEl.setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!drag) {
      if (tool === "crop" && !spaceDown && stageEl) {
        const [sx, sy] = stagePoint(e);
        hoverCursor = HANDLE_CURSORS[hitCrop(sx, sy)];
      }
      return;
    }
    if (drag.kind === "pan") {
      panX = drag.panX + (e.clientX - drag.x);
      panY = drag.panY + (e.clientY - drag.y);
      return;
    }
    const dx = (e.clientX - drag.x) / zoom;
    const dy = (e.clientY - drag.y) / zoom;
    crop = dragCropRect(drag.start, drag.handle, dx, dy, docW, docH, cropRatio);
  }

  function onPointerUp(e: PointerEvent) {
    if (!drag) return;
    if (drag.kind === "crop") {
      // A click or a sliver is not a new crop: keep the one that was there.
      if (crop && (crop.w < 2 || crop.h < 2)) crop = drag.previous ?? fitAspectRect(docW, docH, cropRatio);
    }
    drag = null;
    dragging = false;
    if (stageEl?.hasPointerCapture(e.pointerId)) stageEl.releasePointerCapture(e.pointerId);
  }

  const cursor = $derived.by(() => {
    if (phase !== "ready") return "default";
    if (spaceDown || tool === "hand") return dragging ? "grabbing" : "grab";
    if (tool === "zoom") return "zoom-in";
    return hoverCursor;
  });

  // ------------------------------------------------------------------ save

  async function renderPng(): Promise<number[]> {
    if (!doc) throw new Error("no document");
    let out = doc;
    if (!isNeutral(adjustments)) {
      out = makeCanvas(doc.width, doc.height);
      const ctx = out.getContext("2d", { willReadFrequently: true })!;
      ctx.drawImage(doc, 0, 0);
      const data = ctx.getImageData(0, 0, out.width, out.height);
      applyAdjustments(data.data, data.width, data.height, adjustments, 1);
      ctx.putImageData(data, 0, 0);
    }
    const blob = await new Promise<Blob | null>((resolve) => out.toBlob(resolve, "image/png"));
    if (!blob) throw new Error("PNG encode failed");
    return Array.from(new Uint8Array(await blob.arrayBuffer()));
  }

  function outputMetadata(): Record<string, string> | null {
    const entry = history[historyIndex];
    return editedMetadata(
      sourceMetadata,
      entry?.ops ?? [],
      adjustments,
      { width: docW, height: docH },
      editedFrom,
      new Date().toISOString(),
    );
  }

  function outputName(): string {
    const base = (source?.name ?? "image.png").replace(/\.[a-z0-9]+$/i, "");
    return `${base.startsWith("edit_") ? base : `edit_${base}`}.png`;
  }

  async function saveToGallery() {
    if (busy || !doc) return;
    saving = true;
    error = "";
    // Let the "Saving..." label paint before the pixels are crunched.
    await new Promise((resolve) => setTimeout(resolve, 0));
    try {
      const bytes = await renderPng();
      const saved = await saveToGalleryBytes(
        bytes,
        outputName(),
        `edit_${Date.now()}`, // unique prompt id, so every save is its own file
        generationMode,
        outputMetadata() ?? undefined,
        generation.metadataMode,
        generation.invisibleWatermark,
      );
      savedIndex = historyIndex;
      void gallery.addPersistedImage(saved);
      gallery.showToast(locale.t("editor.saved"), "success");
    } catch (e) {
      console.error("Image editor: failed to save:", e);
      error = locale.t("editor.save_failed");
    } finally {
      saving = false;
    }
  }

  async function saveAsFile() {
    if (busy || !doc) return;
    saving = true;
    error = "";
    await new Promise((resolve) => setTimeout(resolve, 0));
    let url: string | null = null;
    try {
      let bytes = await renderPng();
      const metadata = outputMetadata();
      if (metadata) {
        try {
          bytes = await embedImageMetadataBytes(bytes, metadata, generation.metadataMode);
        } catch (e) {
          console.warn("Image editor: could not embed metadata:", e);
        }
      }
      url = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "image/png" }));
      // PNG blobs pass through saveBlobAs byte for byte, metadata included.
      await gallery.saveBlobAs(url, outputName());
    } catch (e) {
      console.error("Image editor: failed to save file:", e);
      error = locale.t("editor.save_failed");
    } finally {
      if (url) URL.revokeObjectURL(url);
      saving = false;
    }
  }

  function requestClose() {
    if (saving) return;
    if (dirty && !confirmClose) {
      confirmClose = true;
      return;
    }
    imageEditor.close();
  }

  // -------------------------------------------------------------- keyboard

  function isTyping(target: EventTarget | null): boolean {
    return (
      target instanceof HTMLInputElement &&
      target.type !== "range" &&
      target.type !== "checkbox" &&
      target.type !== "button"
    );
  }

  function onKeyDown(e: KeyboardEvent) {
    // Capture phase on window: the app's own shortcuts (inpaint canvas tools,
    // lightbox arrows) must not fire underneath the editor.
    e.stopPropagation();
    const mod = e.ctrlKey || e.metaKey;
    const key = e.key.toLowerCase();

    if (key === "escape") {
      e.preventDefault();
      if (presetNaming) presetNaming = false;
      else if (sizeOpen) sizeOpen = false;
      else if (tool === "crop") cancelCrop();
      else if (confirmClose) confirmClose = false;
      else requestClose();
      return;
    }
    if (isTyping(e.target)) {
      if (key === "enter" && presetNaming) {
        e.preventDefault();
        savePreset();
      } else if (key === "enter" && sizeOpen) {
        e.preventDefault();
        applySize();
      }
      return;
    }
    if (mod) {
      if (key === "z") {
        e.preventDefault();
        if (e.shiftKey) redo();
        else undo();
      } else if (key === "y") {
        e.preventDefault();
        redo();
      } else if (key === "s") {
        e.preventDefault();
        if (e.shiftKey) void saveAsFile();
        else void saveToGallery();
      } else if (key === "0") {
        e.preventDefault();
        fitView();
      } else if (key === "1") {
        e.preventDefault();
        zoomTo(1);
      } else if (key === "=" || key === "+") {
        e.preventDefault();
        zoomTo(zoom * 1.25);
      } else if (key === "-") {
        e.preventDefault();
        zoomTo(zoom / 1.25);
      } else if (key === "i" && e.altKey) {
        e.preventDefault();
        openSize();
      }
      return;
    }
    if (e.code === "Space") {
      e.preventDefault();
      spaceDown = true;
      return;
    }
    if (key === "enter") {
      if (tool === "crop") {
        e.preventDefault();
        applyCrop();
      } else if (sizeOpen) {
        e.preventDefault();
        applySize();
      }
      return;
    }
    if (e.altKey) return;
    if (key === "h" || key === "v") {
      cancelCrop();
      tool = "hand";
    } else if (key === "z") {
      cancelCrop();
      tool = "zoom";
    } else if (key === "c") {
      startCrop();
    }
  }

  function onKeyUp(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.code === "Space") spaceDown = false;
  }

  $effect(() => {
    if (!source) return;
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      spaceDown = false;
    };
  });

  const barButton =
    "flex items-center justify-center w-8 h-8 rounded-md text-neutral-300 hover:bg-neutral-800 hover:text-neutral-100 transition-colors disabled:opacity-35 disabled:hover:bg-transparent disabled:cursor-default";
  const railButton = (active: boolean) =>
    `flex items-center justify-center w-9 h-9 rounded-md transition-colors ${
      active ? "bg-indigo-600 text-white" : "text-neutral-300 hover:bg-neutral-800 hover:text-neutral-100"
    }`;
</script>

{#if source}
  <div
    use:portal
    class="fixed inset-0 z-[230] flex flex-col bg-neutral-950 text-neutral-200 select-none"
    role="dialog"
    aria-modal="true"
    aria-label={locale.t("editor.title")}
  >
    <!-- Top bar -->
    <div class="relative flex items-center gap-1 h-11 px-2 shrink-0 bg-neutral-900 border-b border-neutral-800">
      <span class="px-2 text-sm font-semibold text-neutral-100">{locale.t("editor.title")}</span>
      <span class="w-px h-5 mx-1 bg-neutral-700"></span>
      <button type="button" class={barButton} title={locale.t("editor.undo")} aria-label={locale.t("editor.undo")} disabled={!canUndo || busy} onclick={undo}><Undo2 size={16} /></button>
      <button type="button" class={barButton} title={locale.t("editor.redo")} aria-label={locale.t("editor.redo")} disabled={!canRedo || busy} onclick={redo}><Redo2 size={16} /></button>
      <span class="w-px h-5 mx-1 bg-neutral-700"></span>
      <button type="button" class={barButton} title={locale.t("editor.rotate_left")} aria-label={locale.t("editor.rotate_left")} disabled={busy} onclick={() => rotate(-90)}><RotateCcw size={16} /></button>
      <button type="button" class={barButton} title={locale.t("editor.rotate_right")} aria-label={locale.t("editor.rotate_right")} disabled={busy} onclick={() => rotate(90)}><RotateCw size={16} /></button>
      <button type="button" class={barButton} title={locale.t("editor.flip_h")} aria-label={locale.t("editor.flip_h")} disabled={busy} onclick={() => flip("h")}><FlipH size={16} /></button>
      <button type="button" class={barButton} title={locale.t("editor.flip_v")} aria-label={locale.t("editor.flip_v")} disabled={busy} onclick={() => flip("v")}><FlipV size={16} /></button>
      <button type="button" class="{barButton} {sizeOpen ? 'bg-neutral-800 text-neutral-100' : ''}" title={locale.t("editor.image_size")} aria-label={locale.t("editor.image_size")} aria-expanded={sizeOpen} disabled={busy} onclick={() => (sizeOpen ? (sizeOpen = false) : openSize())}><Scaling size={16} /></button>

      {#if sizeOpen}
        <div class="absolute top-full left-56 z-10 mt-1 flex items-end gap-2 p-3 rounded-lg bg-neutral-900 border border-neutral-700 shadow-xl">
          <label class="flex flex-col gap-1 text-[11px] text-neutral-400">
            {locale.t("editor.width")}
            <input type="number" min="1" max={MAX_DIMENSION} class="w-20 px-2 py-1 rounded bg-neutral-800 border border-neutral-700 text-sm text-neutral-100 select-text" bind:value={sizeW} oninput={() => onSizeInput("w")} />
          </label>
          <button type="button" class="{barButton} {sizeLocked ? 'text-indigo-300' : ''}" title={locale.t("editor.lock_ratio")} aria-label={locale.t("editor.lock_ratio")} aria-pressed={sizeLocked} onclick={() => { sizeLocked = !sizeLocked; if (sizeLocked) onSizeInput("w"); }}>
            {#if sizeLocked}<LinkIcon size={14} />{:else}<Unlink size={14} />{/if}
          </button>
          <label class="flex flex-col gap-1 text-[11px] text-neutral-400">
            {locale.t("editor.height")}
            <input type="number" min="1" max={MAX_DIMENSION} class="w-20 px-2 py-1 rounded bg-neutral-800 border border-neutral-700 text-sm text-neutral-100 select-text" bind:value={sizeH} oninput={() => onSizeInput("h")} />
          </label>
          <button type="button" class="h-8 px-3 rounded-md bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-medium" onclick={applySize}>{locale.t("common.apply")}</button>
        </div>
      {/if}

      <span class="flex-1"></span>

      {#if confirmClose}
        <span class="text-xs text-neutral-300">{locale.t("editor.discard")}</span>
        <button type="button" class="h-8 px-3 rounded-md bg-red-700 hover:bg-red-600 text-white text-xs font-medium" onclick={() => imageEditor.close()}>{locale.t("editor.discard_yes")}</button>
        <button type="button" class="h-8 px-3 rounded-md bg-neutral-800 hover:bg-neutral-700 text-neutral-100 text-xs font-medium" onclick={() => (confirmClose = false)}>{locale.t("editor.keep")}</button>
      {:else}
        {#if saving}
          <span class="px-2 text-xs text-indigo-300">{locale.t("common.saving")}</span>
        {:else if error && phase === "ready"}
          <span class="px-2 text-xs text-red-400">{error}</span>
        {/if}
        <button type="button" class={barButton} title={locale.t("editor.save_file")} aria-label={locale.t("editor.save_file")} disabled={busy} onclick={saveAsFile}><Download size={16} /></button>
        <button type="button" class="flex items-center gap-1.5 h-8 px-3 rounded-md bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-medium transition-colors disabled:opacity-40" title={locale.t("editor.save")} disabled={busy} onclick={saveToGallery}><Save size={14} />{locale.t("common.save")}</button>
      {/if}
      <button type="button" class={barButton} title={locale.t("common.close")} aria-label={locale.t("common.close")} onclick={requestClose}><X size={16} /></button>
    </div>

    <!-- Crop options -->
    {#if tool === "crop" && crop}
      <div class="flex items-center gap-1 h-10 px-3 shrink-0 bg-neutral-900/70 border-b border-neutral-800">
        {#each CROP_PRESETS as preset (preset.key)}
          <button
            type="button"
            class="h-7 px-2.5 rounded-md text-xs transition-colors {cropPreset === preset.key ? 'bg-indigo-600 text-white' : 'text-neutral-300 hover:bg-neutral-800'}"
            aria-pressed={cropPreset === preset.key}
            onclick={() => setCropPreset(preset.key)}
          >{preset.label || locale.t(`editor.crop.${preset.key}`)}</button>
        {/each}
        <span class="px-3 text-xs tabular-nums text-neutral-400">{Math.round(crop.w)} × {Math.round(crop.h)}</span>
        <span class="flex-1"></span>
        <button type="button" class="h-7 px-3 rounded-md bg-neutral-800 hover:bg-neutral-700 text-neutral-100 text-xs font-medium" onclick={cancelCrop}>{locale.t("common.cancel")}</button>
        <button type="button" class="h-7 px-3 rounded-md bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-medium" onclick={applyCrop}>{locale.t("common.apply")}</button>
      </div>
    {/if}

    <div class="flex flex-1 min-h-0">
      <!-- Tool rail -->
      <div class="flex flex-col items-center gap-1 w-12 py-2 shrink-0 bg-neutral-900 border-r border-neutral-800">
        <button type="button" class={railButton(tool === "hand")} title={locale.t("editor.tool.hand")} aria-label={locale.t("editor.tool.hand")} aria-pressed={tool === "hand"} onclick={() => { cancelCrop(); tool = "hand"; }}><Hand size={18} /></button>
        <button type="button" class={railButton(tool === "zoom")} title={locale.t("editor.tool.zoom")} aria-label={locale.t("editor.tool.zoom")} aria-pressed={tool === "zoom"} onclick={() => { cancelCrop(); tool = "zoom"; }}><Search size={18} /></button>
        <button type="button" class={railButton(tool === "crop")} title={locale.t("editor.tool.crop")} aria-label={locale.t("editor.tool.crop")} aria-pressed={tool === "crop"} disabled={busy} onclick={startCrop}><Crop size={18} /></button>
      </div>

      <!-- Stage -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        bind:this={stageEl}
        use:trackStage
        class="relative flex-1 min-w-0 overflow-hidden bg-neutral-950 touch-none"
        style:cursor={cursor}
        onpointerdown={onPointerDown}
        onpointermove={onPointerMove}
        onpointerup={onPointerUp}
        onpointercancel={onPointerUp}
      >
        <canvas bind:this={viewEl} class="absolute inset-0 w-full h-full"></canvas>
        {#if phase === "error"}
          <div class="absolute inset-0 flex items-center justify-center text-sm text-red-400">{error}</div>
        {/if}
      </div>

      <!-- Adjustments -->
      <div class="flex flex-col w-64 shrink-0 bg-neutral-900 border-l border-neutral-800">
        <div class="flex items-center h-9 px-3 border-b border-neutral-800">
          <span class="flex-1 text-xs font-semibold uppercase tracking-wide text-neutral-400">{locale.t("editor.adjustments")}</span>
          <button
            type="button"
            class="text-[11px] text-neutral-400 hover:text-neutral-100 disabled:opacity-35 disabled:hover:text-neutral-400"
            disabled={busy || isNeutral(adjustments)}
            onclick={() => { adjustments = { ...NEUTRAL_ADJUSTMENTS }; commitAdjustments(); }}
          >{locale.t("common.reset")}</button>
        </div>
        <div class="flex flex-wrap items-center gap-1 px-3 py-2 border-b border-neutral-800">
          {#each presets as preset (preset.name)}
            <span class="flex items-center h-6 rounded bg-neutral-800 text-[11px] text-neutral-200">
              <button type="button" class="h-6 pl-2 pr-1 hover:text-white disabled:opacity-40" disabled={busy} onclick={() => applyPreset(preset)}>{preset.name}</button>
              <button type="button" class="flex items-center justify-center w-5 h-6 text-neutral-500 hover:text-red-400" title={locale.t("editor.preset_delete")} aria-label="{locale.t('editor.preset_delete')}: {preset.name}" onclick={() => storePresets(presets.filter((p) => p.name !== preset.name))}><X size={11} /></button>
            </span>
          {/each}
          {#if presetNaming}
            <input
              use:focusOnMount
              type="text"
              maxlength="40"
              class="h-6 w-28 px-2 rounded bg-neutral-800 border border-neutral-600 text-[11px] text-neutral-100 select-text"
              placeholder={locale.t("editor.preset_name")}
              aria-label={locale.t("editor.preset_name")}
              bind:value={presetName}
              onblur={() => { if (!presetName.trim()) presetNaming = false; }}
            />
            <button type="button" class="h-6 px-2 rounded bg-indigo-600 hover:bg-indigo-500 text-white text-[11px] disabled:opacity-40" disabled={!presetName.trim()} onclick={savePreset}>{locale.t("common.save")}</button>
          {:else}
            <button
              type="button"
              class="h-6 px-2 rounded border border-dashed border-neutral-700 text-[11px] text-neutral-400 hover:text-neutral-100 hover:border-neutral-500 disabled:opacity-35"
              disabled={busy || isNeutral(adjustments)}
              onclick={() => { presetName = ""; presetNaming = true; }}
            >+ {locale.t("editor.preset")}</button>
          {/if}
        </div>
        <div class="flex flex-col gap-4 p-3 overflow-y-auto">
          {#each GROUPS as group (group.key)}
            <div class="flex flex-col gap-2.5">
              <span class="text-[10px] font-semibold uppercase tracking-wider text-neutral-500">{locale.t(`editor.group.${group.key}`)}</span>
              {#each group.sliders as slider (slider.key)}
                <label class="flex flex-col gap-1">
                  <span class="flex items-center text-xs">
                    <span class="flex-1 text-neutral-300">{locale.t(`editor.adjust.${slider.key}`)}</span>
                    <span class="tabular-nums {adjustments[slider.key] === 0 ? 'text-neutral-500' : 'text-indigo-300'}">{adjustments[slider.key]}</span>
                  </span>
                  <input
                    type="range"
                    min={slider.min}
                    max={slider.max}
                    step="1"
                    class="w-full accent-indigo-500"
                    disabled={busy}
                    bind:value={adjustments[slider.key]}
                    onchange={commitAdjustments}
                    ondblclick={() => { adjustments[slider.key] = 0; commitAdjustments(); }}
                  />
                </label>
              {/each}
            </div>
            {#if group.key === "light"}
              <div class="flex flex-col gap-2">
                <span class="flex items-center">
                  <span class="flex-1 text-[10px] font-semibold uppercase tracking-wider text-neutral-500">{locale.t("editor.group.curve")}</span>
                  <button type="button" class="text-[11px] text-neutral-400 hover:text-neutral-100 disabled:opacity-35 disabled:hover:text-neutral-400" disabled={busy || isIdentityCurve(adjustments.curve)} onclick={resetCurve}>{locale.t("common.reset")}</button>
                </span>
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <svg
                  bind:this={curveEl}
                  viewBox="-6 -6 267 267"
                  class="w-full aspect-square rounded bg-neutral-950 border border-neutral-800 touch-none cursor-crosshair"
                  role="img"
                  aria-label={locale.t("editor.group.curve")}
                  onpointerdown={onCurveDown}
                  onpointermove={onCurveMove}
                  onpointerup={onCurveUp}
                  onpointercancel={onCurveUp}
                  ondblclick={onCurveDblClick}
                >
                  {#each [63.75, 127.5, 191.25] as line (line)}
                    <line x1={line} y1="0" x2={line} y2="255" stroke="#262626" stroke-width="1" />
                    <line x1="0" y1={line} x2="255" y2={line} stroke="#262626" stroke-width="1" />
                  {/each}
                  <line x1="0" y1="255" x2="255" y2="0" stroke="#404040" stroke-width="1" stroke-dasharray="4 4" />
                  <path d={curvePath} fill="none" stroke="#e5e5e5" stroke-width="2" />
                  {#each adjustments.curve as point, index (index)}
                    <circle cx={point[0]} cy={255 - point[1]} r="5" fill="#171717" stroke="#e5e5e5" stroke-width="2" />
                  {/each}
                </svg>
              </div>
            {/if}
          {/each}
        </div>
      </div>
    </div>

    <!-- Status bar -->
    <div class="flex items-center gap-3 h-7 px-3 shrink-0 bg-neutral-900 border-t border-neutral-800 text-[11px] text-neutral-400">
      <button type="button" class="tabular-nums hover:text-neutral-100" title="Ctrl+1" onclick={() => zoomTo(1)}>{Math.round(zoom * 100)}%</button>
      <button type="button" class="hover:text-neutral-100" title="Ctrl+0" onclick={() => fitView()}>{locale.t("editor.fit")}</button>
      {#if docW}
        <span class="tabular-nums">{docW} × {docH} px</span>
      {/if}
      <span class="flex-1"></span>
      {#if phase === "loading"}
        <span>{locale.t("common.loading")}</span>
      {:else if phase === "ready"}
        <span class="flex items-center gap-1.5" title={metadataTitle}>
          <span class="w-1.5 h-1.5 rounded-full {sourceMetadata ? 'bg-emerald-400' : 'bg-neutral-600'}"></span>
          {locale.t(sourceMetadata ? "editor.metadata" : "editor.no_metadata")}
        </span>
      {/if}
    </div>
  </div>
{/if}
