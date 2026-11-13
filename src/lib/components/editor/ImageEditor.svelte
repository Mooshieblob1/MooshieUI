<script lang="ts">
  import { tick } from "svelte";
  import Move from "@lucide/svelte/icons/move";
  import Type from "@lucide/svelte/icons/type";
  import Bold from "@lucide/svelte/icons/bold";
  import Italic from "@lucide/svelte/icons/italic";
  import VenetianMask from "@lucide/svelte/icons/venetian-mask";
  import Hand from "@lucide/svelte/icons/hand";
  import Search from "@lucide/svelte/icons/search";
  import Crop from "@lucide/svelte/icons/crop";
  import SquareDashed from "@lucide/svelte/icons/square-dashed";
  import Lasso from "@lucide/svelte/icons/lasso";
  import Brush from "@lucide/svelte/icons/brush";
  import Eraser from "@lucide/svelte/icons/eraser";
  import Stamp from "@lucide/svelte/icons/stamp";
  import Pipette from "@lucide/svelte/icons/pipette";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import Plus from "@lucide/svelte/icons/plus";
  import Copy from "@lucide/svelte/icons/copy";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import MergeDown from "@lucide/svelte/icons/arrow-down-to-line";
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
    alphaBounds,
    appendOp,
    applyAdjustments,
    curveLut,
    isIdentityCurve,
    normalizeAdjustments,
    rectFromAnchor,
    strokeStamps,
    dragCropRect,
    editedMetadata,
    fitAspectRect,
    isNeutral,
    roundRect,
    sameAdjustments,
    scaleRect,
  } from "../../utils/imageEditorOps.js";
  import type { AdjustmentKey, Adjustments, CropHandle, CurvePoint, EditOp, PaintTool, Rect } from "../../utils/imageEditorOps.js";
  import type { GenerationMode } from "../../types/index.js";

  type Tool = "move" | "text" | "hand" | "zoom" | "crop" | "marquee" | "lasso" | "brush" | "eraser" | "clone" | "eyedropper";

  const isPaintTool = (t: Tool): t is PaintTool => t === "brush" || t === "eraser" || t === "clone";

  const TOOL_KEYS: Record<string, Tool> = {
    h: "hand",
    v: "move",
    t: "text",
    z: "zoom",
    c: "crop",
    m: "marquee",
    l: "lasso",
    b: "brush",
    e: "eraser",
    s: "clone",
    i: "eyedropper",
  };

  /** Canvas composite operations, in Photoshop's menu order. */
  const BLEND_MODES = [
    "normal",
    "darken",
    "multiply",
    "color-burn",
    "lighten",
    "screen",
    "color-dodge",
    "overlay",
    "soft-light",
    "hard-light",
    "difference",
    "hue",
    "saturation",
    "color",
    "luminosity",
  ] as const;
  type BlendMode = (typeof BLEND_MODES)[number];

  const FONTS = ["sans-serif", "serif", "monospace", "cursive", "Arial", "Georgia", "Impact", "Courier New", "Times New Roman", "Verdana"];
  const MIN_TEXT = 4;
  const MAX_TEXT = 2000;

  /** A line of type. `x`, `y` is the top left corner in document pixels. */
  interface TextSpec {
    text: string;
    font: string;
    size: number;
    bold: boolean;
    italic: boolean;
    color: string;
    x: number;
    y: number;
  }

  /** One document-sized raster. Canvases are never drawn on after they are stored. */
  interface Layer {
    id: number;
    name: string;
    canvas: HTMLCanvasElement;
    visible: boolean;
    /** 0..100 */
    opacity: number;
    blend: BlendMode;
    /** Document-sized; a pixel's alpha is how much of the layer shows there. */
    mask: HTMLCanvasElement | null;
    maskOn: boolean;
    /** Set while the layer is still editable type; painting on it clears this. */
    text: TextSpec | null;
  }

  /** A closed outline in document pixels; `inverted` selects everything outside it. */
  interface Selection {
    points: [number, number][];
    inverted: boolean;
  }

  interface Stroke {
    tool: PaintTool;
    layerId: number;
    /** The stroke paints the layer's mask instead of its pixels. */
    onMask: boolean;
    /** The layer as it was before the stroke. */
    base: HTMLCanvasElement;
    /** The layer with the stroke applied; becomes the layer on pointer up. */
    work: HTMLCanvasElement;
    /** The stroke alone, at full strength. */
    mask: HTMLCanvasElement;
    /** Clone stamp only: the shifted source cut to the stroke's shape. */
    temp: HTMLCanvasElement | null;
    cloneSrc: HTMLCanvasElement | null;
    dx: number;
    dy: number;
    tip: HTMLCanvasElement;
    lastX: number;
    lastY: number;
    carry: number;
  }

  interface HistoryEntry {
    layers: Layer[];
    activeId: number;
    adjustments: Adjustments;
    ops: EditOp[];
  }

  /** Longest side of the buffer the sliders are previewed on. */
  const PREVIEW_MAX = 2048;
  const MAX_DIMENSION = 16384;
  const MAX_HISTORY = 30;
  /** Undo steps share canvases; this caps the distinct pixels they hold. */
  const HISTORY_BYTES = 768 * 1024 * 1024;
  const MAX_LAYERS = 16;
  const MAX_BRUSH = 500;
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

  // The document: layers bottom to top, all the same size. `docVersion`
  // changes whenever any pixel on screen may have.
  let layers = $state.raw<Layer[]>([]);
  let activeId = $state(0);
  let nextLayerId = 1;
  let compCanvas: HTMLCanvasElement | null = null;
  let compResult: HTMLCanvasElement | null = null;
  let compFor = -1;
  let stroke: Stroke | null = null;
  /** A move or resize of one layer, shown live until the pointer is released. */
  let xform: { id: number; from: Rect; to: Rect } | null = null;
  let scratch: HTMLCanvasElement | null = null;
  /** Brush and eraser paint the active layer's mask instead of its pixels. */
  let maskEdit = $state(false);
  let textDraft = $state({ font: "sans-serif", size: 64, bold: false, italic: false });
  let textInput = $state<HTMLInputElement | null>(null);

  let selection = $state.raw<Selection | null>(null);
  let color = $state("#ffffff");
  let brush = $state({ size: 40, hardness: 80, opacity: 100 });
  /** Clone stamp: where Alt+click sampled, and the fixed offset once painting starts. */
  let cloneSource = $state.raw<[number, number] | null>(null);
  let cloneOffset: [number, number] | null = null;
  /** Pointer position on the stage, for the brush outline. */
  let hover = $state.raw<[number, number] | null>(null);
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

  let crop = $state<Rect | null>(null);
  let cropPreset = $state("free");
  let cropRatio = $state<number | null>(null);

  let sizeOpen = $state(false);
  let sizeW = $state(0);
  let sizeH = $state(0);
  let sizeLocked = $state(true);

  // Preview buffer for the adjustment sliders.
  let adjusted: HTMLCanvasElement | null = null;
  let adjustedFor: { version: number; adjustments: Adjustments } | null = null;

  const canUndo = $derived(historyIndex > 0);
  const canRedo = $derived(historyIndex < historyLength - 1);
  const dirty = $derived(historyIndex !== savedIndex);
  const busy = $derived(saving || phase !== "ready");
  const activeLayer = $derived(layers.find((l) => l.id === activeId) ?? null);
  const activeIndex = $derived(layers.findIndex((l) => l.id === activeId));
  const onMask = $derived(maskEdit && !!activeLayer?.mask && activeLayer.maskOn);
  const textSpec = $derived(activeLayer?.text ?? null);
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
    layers = [];
    stroke = null;
    xform = null;
    maskEdit = false;
    drag = null;
    compResult = null;
    compCanvas = null;
    selection = null;
    cloneSource = null;
    cloneOffset = null;
    hover = null;
    nextLayerId = 1;
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
        const background: Layer = {
          id: nextLayerId++,
          name: locale.t("editor.layer.background"),
          canvas,
          visible: true,
          opacity: 100,
          blend: "normal",
          mask: null,
          maskOn: true,
          text: null,
        };
        textDraft.size = Math.min(400, Math.max(12, Math.round(Math.max(canvas.width, canvas.height) / 16)));
        history = [{ layers: [background], activeId: background.id, adjustments: { ...NEUTRAL_ADJUSTMENTS }, ops: [] }];
        historyLength = 1;
        historyIndex = 0;
        savedIndex = 0;
        showLayers([background], background.id);
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

  // ---------------------------------------------------------------- layers

  const layerCanvas = (layer: Layer) =>
    stroke && stroke.layerId === layer.id && !stroke.onMask ? stroke.work : layer.canvas;
  const layerMask = (layer: Layer) =>
    !layer.maskOn ? null : stroke && stroke.layerId === layer.id && stroke.onMask ? stroke.work : layer.mask;

  /** Map the box `from` onto the box `to`. */
  function applyXform(ctx: CanvasRenderingContext2D, from: Rect, to: Rect) {
    const sx = to.w / from.w;
    const sy = to.h / from.h;
    ctx.setTransform(sx, 0, 0, sy, to.x - from.x * sx, to.y - from.y * sy);
    ctx.imageSmoothingQuality = "high";
  }

  /** A layer as it is composited: moved or resized while a drag is live, and cut by its mask. */
  function layerPixels(layer: Layer): HTMLCanvasElement {
    const pixels = layerCanvas(layer);
    const mask = layerMask(layer);
    const moving = xform && xform.id === layer.id ? xform : null;
    if (!mask && !moving) return pixels;
    if (!scratch || scratch.width !== docW || scratch.height !== docH) scratch = makeCanvas(docW, docH);
    const ctx = scratch.getContext("2d")!;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalCompositeOperation = "source-over";
    ctx.clearRect(0, 0, docW, docH);
    if (moving) applyXform(ctx, moving.from, moving.to);
    ctx.drawImage(pixels, 0, 0);
    if (mask) {
      ctx.globalCompositeOperation = "destination-in";
      ctx.drawImage(mask, 0, 0);
    }
    return scratch;
  }

  /** All visible layers flattened, before adjustments. Cached per `docVersion`. */
  function composite(): HTMLCanvasElement | null {
    if (layers.length === 0) return null;
    if (compResult && compFor === docVersion) return compResult;
    const visible = layers.filter((l) => l.visible);
    const only = visible.length === 1 ? visible[0] : null;
    if (only && only.opacity === 100 && !layerMask(only) && xform?.id !== only.id) {
      compResult = layerCanvas(only);
    } else {
      if (!compCanvas || compCanvas.width !== docW || compCanvas.height !== docH) compCanvas = makeCanvas(docW, docH);
      const ctx = compCanvas.getContext("2d")!;
      ctx.clearRect(0, 0, docW, docH);
      for (const layer of visible) {
        const pixels = layerPixels(layer);
        ctx.globalAlpha = layer.opacity / 100;
        ctx.globalCompositeOperation = layer.blend === "normal" ? "source-over" : layer.blend;
        ctx.drawImage(pixels, 0, 0);
      }
      ctx.globalAlpha = 1;
      ctx.globalCompositeOperation = "source-over";
      compResult = compCanvas;
    }
    compFor = docVersion;
    return compResult;
  }

  function showLayers(next: Layer[], active: number) {
    layers = next;
    activeId = next.some((l) => l.id === active) ? active : (next[next.length - 1]?.id ?? 0);
    docW = next[0]?.canvas.width ?? 0;
    docH = next[0]?.canvas.height ?? 0;
    docVersion++;
  }

  // --------------------------------------------------------------- history

  function historyBytes(entries: HistoryEntry[]): number {
    const seen = new Set<HTMLCanvasElement>();
    let bytes = 0;
    for (const entry of entries) {
      for (const layer of entry.layers) {
        for (const canvas of [layer.canvas, layer.mask]) {
          if (!canvas || seen.has(canvas)) continue;
          seen.add(canvas);
          bytes += canvas.width * canvas.height * 4;
        }
      }
    }
    return bytes;
  }

  function pushHistory(entry: HistoryEntry) {
    history = [...history.slice(0, historyIndex + 1), entry];
    // Every paint stroke keeps a full copy of its layer, so cap by memory as
    // well as by count. The current state always survives.
    let dropped = 0;
    while (
      history.length - dropped > 2 &&
      (history.length - dropped > MAX_HISTORY || historyBytes(history.slice(dropped)) > HISTORY_BYTES)
    ) {
      dropped++;
    }
    if (dropped > 0) {
      history = history.slice(dropped);
      savedIndex -= dropped;
    }
    historyLength = history.length;
    historyIndex = history.length - 1;
  }

  /** Make `next` the document and record it as one undo step. */
  function commit(next: Layer[], op: EditOp | null = null, active = activeId) {
    const current = history[historyIndex];
    pushHistory({
      layers: next,
      activeId: active,
      adjustments: { ...adjustments },
      ops: op ? appendOp(current.ops, op) : current.ops,
    });
    showLayers(next, active);
  }

  /** Record the sliders' current values as one undo step. */
  function commitAdjustments() {
    const current = history[historyIndex];
    if (!current || sameAdjustments(current.adjustments, adjustments)) return;
    pushHistory({ ...current, activeId, adjustments: { ...adjustments } });
  }

  function restore(index: number) {
    const entry = history[index];
    if (!entry) return;
    historyIndex = index;
    adjustments = { ...entry.adjustments };
    crop = null;
    xform = null;
    if (tool === "crop") tool = "hand";
    if (entry.layers[0].canvas.width !== docW || entry.layers[0].canvas.height !== docH) selection = null;
    showLayers(entry.layers, entry.activeId);
  }

  function undo() {
    if (canUndo && !busy && !stroke) restore(historyIndex - 1);
  }

  function redo() {
    if (canRedo && !busy && !stroke) restore(historyIndex + 1);
  }

  // ------------------------------------------------------------- layer ops

  function copyCanvas(source: HTMLCanvasElement): HTMLCanvasElement {
    const out = makeCanvas(source.width, source.height);
    out.getContext("2d")!.drawImage(source, 0, 0);
    return out;
  }

  function newLayerRecord(canvas: HTMLCanvasElement): Layer {
    const id = nextLayerId++;
    return {
      id,
      name: locale.t("editor.layer.name", { n: String(id) }),
      canvas,
      visible: true,
      opacity: 100,
      blend: "normal",
      mask: null,
      maskOn: true,
      text: null,
    };
  }

  function insertAbove(layer: Layer, op: EditOp | null = null) {
    const index = layers.findIndex((l) => l.id === activeId);
    const next = [...layers.slice(0, index + 1), layer, ...layers.slice(index + 1)];
    maskEdit = false;
    commit(next, op, layer.id);
  }

  /** The active layer swapped for a changed copy. */
  const withActive = (patch: Partial<Layer>) => layers.map((l) => (l.id === activeId ? { ...l, ...patch } : l));

  /** A layer's pixels with its mask applied for good. */
  function masked(layer: Layer): HTMLCanvasElement {
    if (!layer.mask || !layer.maskOn) return layer.canvas;
    const out = copyCanvas(layer.canvas);
    const ctx = out.getContext("2d")!;
    ctx.globalCompositeOperation = "destination-in";
    ctx.drawImage(layer.mask, 0, 0);
    return out;
  }

  function selectLayer(id: number, editMask = false) {
    const layer = layers.find((l) => l.id === id);
    if (!layer) return;
    maskEdit = editMask && !!layer.mask && !(maskEdit && activeId === id);
    activeId = id;
    if (layer.text) {
      color = layer.text.color;
      textDraft = { font: layer.text.font, size: layer.text.size, bold: layer.text.bold, italic: layer.text.italic };
    }
  }

  function setBlend(blend: BlendMode) {
    if (busy || !activeLayer || activeLayer.blend === blend) return;
    commit(withActive({ blend }));
  }

  /** A new mask shows the selection, or the whole layer when nothing is selected. */
  function addMask() {
    if (busy || !activeLayer || activeLayer.mask) return;
    const mask = makeCanvas(docW, docH);
    const ctx = mask.getContext("2d")!;
    clipToSelection(ctx);
    ctx.fillRect(0, 0, docW, docH);
    commit(withActive({ mask, maskOn: true }), { op: "mask", n: 1 });
    maskEdit = true;
  }

  function deleteMask() {
    if (busy || !activeLayer?.mask) return;
    maskEdit = false;
    commit(withActive({ mask: null, maskOn: true }));
  }

  function toggleMask() {
    if (busy || !activeLayer?.mask) return;
    commit(withActive({ maskOn: !activeLayer.maskOn }));
  }

  function addLayer() {
    if (busy || layers.length >= MAX_LAYERS) return;
    insertAbove(newLayerRecord(makeCanvas(docW, docH)));
  }

  /** Ctrl+J: the selection (or the whole layer) copied to a new layer. */
  function duplicateLayer() {
    const layer = activeLayer;
    if (busy || !layer || layers.length >= MAX_LAYERS) return;
    const canvas = makeCanvas(docW, docH);
    const ctx = canvas.getContext("2d")!;
    clipToSelection(ctx);
    ctx.drawImage(layer.canvas, 0, 0);
    insertAbove({
      ...newLayerRecord(canvas),
      opacity: layer.opacity,
      blend: layer.blend,
      mask: layer.mask,
      maskOn: layer.maskOn,
      text: selection ? null : layer.text,
    });
  }

  function mergeDown() {
    const index = layers.findIndex((l) => l.id === activeId);
    if (busy || index <= 0) return;
    const upper = layers[index];
    const lower = layers[index - 1];
    // Both masks are baked in: the merged pixels are what was on screen.
    const baked = masked(lower);
    const canvas = baked === lower.canvas ? copyCanvas(baked) : baked;
    const ctx = canvas.getContext("2d")!;
    ctx.globalAlpha = upper.opacity / 100;
    ctx.globalCompositeOperation = upper.blend === "normal" ? "source-over" : upper.blend;
    if (upper.visible) ctx.drawImage(masked(upper), 0, 0);
    const merged: Layer = { ...lower, canvas, mask: null, maskOn: true, text: null };
    const next = [...layers.slice(0, index - 1), merged, ...layers.slice(index + 1)];
    maskEdit = false;
    commit(next, null, lower.id);
  }

  function deleteLayer() {
    if (busy || layers.length <= 1) return;
    const index = layers.findIndex((l) => l.id === activeId);
    const next = layers.filter((l) => l.id !== activeId);
    maskEdit = false;
    commit(next, null, next[Math.max(0, index - 1)].id);
  }

  function moveLayer(direction: 1 | -1) {
    const index = layers.findIndex((l) => l.id === activeId);
    const target = index + direction;
    if (busy || index < 0 || target < 0 || target >= layers.length) return;
    const next = [...layers];
    [next[index], next[target]] = [next[target], next[index]];
    commit(next);
  }

  function toggleLayer(id: number) {
    if (busy) return;
    commit(layers.map((l) => (l.id === id ? { ...l, visible: !l.visible } : l)));
  }

  /** Live while the slider moves; `commitLayerOpacity` records the result. */
  function previewLayerOpacity(value: number) {
    layers = layers.map((l) => (l.id === activeId ? { ...l, opacity: value } : l));
    docVersion++;
  }

  function commitLayerOpacity() {
    const current = history[historyIndex];
    const before = current.layers.find((l) => l.id === activeId)?.opacity;
    if (before === activeLayer?.opacity) return;
    commit(layers);
  }

  // ------------------------------------------------------------- selection

  function selectionOutline(sel: Selection): Path2D {
    const path = new Path2D();
    sel.points.forEach(([x, y], i) => (i === 0 ? path.moveTo(x, y) : path.lineTo(x, y)));
    path.closePath();
    return path;
  }

  /** Restrict drawing on a document-sized context to the selection, if any. */
  function clipToSelection(ctx: CanvasRenderingContext2D) {
    if (!selection) return;
    const path = selectionOutline(selection);
    if (selection.inverted) path.rect(0, 0, docW, docH);
    ctx.clip(path, "evenodd");
  }

  function selectAll() {
    selection = { points: [[0, 0], [docW, 0], [docW, docH], [0, docH]], inverted: false };
  }

  function invertSelection() {
    if (!selection) selectAll();
    else selection = { ...selection, inverted: !selection.inverted };
  }

  /** Alt+Backspace: fill the selection (or the layer) with the brush colour. On a mask it hides. */
  function fillSelection() {
    const layer = activeLayer;
    if (busy || !layer || !layer.visible) return;
    const canvas = copyCanvas(onMask && layer.mask ? layer.mask : layer.canvas);
    const ctx = canvas.getContext("2d")!;
    clipToSelection(ctx);
    if (onMask) {
      ctx.clearRect(0, 0, docW, docH);
      commit(withActive({ mask: canvas }), { op: "mask", n: 1 });
      return;
    }
    ctx.fillStyle = color;
    ctx.fillRect(0, 0, docW, docH);
    commit(withActive({ canvas, text: null }), { op: "fill", n: 1 });
  }

  /** Delete: make the selected pixels of the active layer transparent. On a mask it reveals. */
  function clearSelection() {
    const layer = activeLayer;
    if (busy || !layer || !layer.visible || !selection) return;
    const canvas = copyCanvas(onMask && layer.mask ? layer.mask : layer.canvas);
    const ctx = canvas.getContext("2d")!;
    clipToSelection(ctx);
    if (onMask) {
      ctx.fillRect(0, 0, docW, docH);
      commit(withActive({ mask: canvas }), { op: "mask", n: 1 });
      return;
    }
    ctx.clearRect(0, 0, docW, docH);
    commit(withActive({ canvas, text: null }), { op: "clear", n: 1 });
  }

  // -------------------------------------------------------------- painting

  /** One soft or hard dab, pre-rendered so a stroke is a run of drawImage calls. */
  function makeTip(size: number, hardness: number, fill: string): HTMLCanvasElement {
    const radius = Math.max(0.5, size / 2);
    const side = Math.ceil(radius * 2) + 2;
    const tip = makeCanvas(side, side);
    const ctx = tip.getContext("2d")!;
    const c = side / 2;
    const gradient = ctx.createRadialGradient(c, c, 0, c, c, radius);
    gradient.addColorStop(0, fill);
    gradient.addColorStop(Math.min(0.98, hardness / 100), fill);
    gradient.addColorStop(1, `${fill}00`);
    ctx.fillStyle = gradient;
    ctx.beginPath();
    ctx.arc(c, c, radius, 0, Math.PI * 2);
    ctx.fill();
    return tip;
  }

  function stamp(s: Stroke, x: number, y: number) {
    s.mask.getContext("2d")!.drawImage(s.tip, x - s.tip.width / 2, y - s.tip.height / 2);
  }

  function beginStroke(kind: PaintTool, x: number, y: number): boolean {
    const layer = activeLayer;
    if (!layer || !layer.visible) return false;
    const mask = onMask ? layer.mask : null;
    if (mask && kind === "clone") return false;
    const base = mask ?? layer.canvas;
    let cloneSrc: HTMLCanvasElement | null = null;
    let dx = 0;
    let dy = 0;
    if (kind === "clone") {
      const flat = composite();
      if (!cloneSource || !flat) return false;
      // The offset is fixed on the first stroke after picking a source, so
      // later strokes keep painting the same patch (Photoshop's "Aligned").
      if (!cloneOffset) cloneOffset = [x - cloneSource[0], y - cloneSource[1]];
      [dx, dy] = cloneOffset;
      cloneSrc = copyCanvas(flat);
    }
    stroke = {
      tool: kind,
      layerId: layer.id,
      onMask: !!mask,
      base,
      work: copyCanvas(base),
      mask: makeCanvas(docW, docH),
      temp: kind === "clone" ? makeCanvas(docW, docH) : null,
      cloneSrc,
      dx,
      dy,
      tip: makeTip(brush.size, brush.hardness, kind === "brush" && !mask ? color : "#000000"),
      lastX: x,
      lastY: y,
      carry: 0,
    };
    stamp(stroke, x, y);
    renderStroke();
    return true;
  }

  function extendStroke(x: number, y: number) {
    const s = stroke;
    if (!s) return;
    const spacing = Math.max(1, brush.size * (brush.hardness >= 90 ? 0.08 : 0.12));
    const run = strokeStamps(s.lastX, s.lastY, x, y, spacing, s.carry);
    for (const [px, py] of run.points) stamp(s, px, py);
    s.carry = run.carry;
    s.lastX = x;
    s.lastY = y;
    renderStroke();
  }

  /** Rebuild the working layer from the untouched base plus the stroke so far. */
  function renderStroke() {
    const s = stroke;
    if (!s) return;
    let paint = s.mask;
    if (s.temp && s.cloneSrc) {
      const tctx = s.temp.getContext("2d")!;
      tctx.globalCompositeOperation = "source-over";
      tctx.clearRect(0, 0, docW, docH);
      tctx.drawImage(s.cloneSrc, s.dx, s.dy);
      tctx.globalCompositeOperation = "destination-in";
      tctx.drawImage(s.mask, 0, 0);
      paint = s.temp;
    }
    const ctx = s.work.getContext("2d")!;
    ctx.save();
    ctx.globalCompositeOperation = "copy";
    ctx.drawImage(s.base, 0, 0);
    // On a mask the brush hides (removes alpha) and the eraser brings it back.
    const removes = s.onMask ? s.tool === "brush" : s.tool === "eraser";
    ctx.globalCompositeOperation = removes ? "destination-out" : "source-over";
    clipToSelection(ctx);
    ctx.globalAlpha = brush.opacity / 100;
    ctx.drawImage(paint, 0, 0);
    ctx.restore();
    docVersion++;
  }

  function endStroke() {
    const s = stroke;
    if (!s) return;
    stroke = null;
    if (s.onMask) {
      commit(layers.map((l) => (l.id === s.layerId ? { ...l, mask: s.work } : l)), { op: "mask", n: 1 });
    } else {
      commit(layers.map((l) => (l.id === s.layerId ? { ...l, canvas: s.work, text: null } : l)), { op: s.tool, n: 1 });
    }
  }

  function pickColor(x: number, y: number) {
    const flat = composite();
    if (!flat || x < 0 || y < 0 || x >= docW || y >= docH) return;
    const [r, g, b] = flat.getContext("2d")!.getImageData(Math.floor(x), Math.floor(y), 1, 1).data;
    color = `#${[r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("")}`;
  }

  // ------------------------------------------------------------------ text

  const fontOf = (t: TextSpec) =>
    `${t.italic ? "italic " : ""}${t.bold ? "bold " : ""}${t.size}px ${/\s/.test(t.font) ? `"${t.font}"` : t.font}`;

  function renderText(spec: TextSpec, width = docW, height = docH): HTMLCanvasElement {
    const canvas = makeCanvas(width, height);
    const ctx = canvas.getContext("2d")!;
    ctx.font = fontOf(spec);
    ctx.textBaseline = "top";
    ctx.fillStyle = spec.color;
    ctx.fillText(spec.text, spec.x, spec.y);
    return canvas;
  }

  const textName = (spec: TextSpec) => spec.text.trim().slice(0, 24) || locale.t("editor.text.placeholder");
  const clampTextSize = (size: number) => Math.min(MAX_TEXT, Math.max(MIN_TEXT, Math.round(size) || MIN_TEXT));

  /** A click with the Text tool: a new line of type, ready to be typed over. */
  function createText(x: number, y: number) {
    if (busy || layers.length >= MAX_LAYERS) return;
    const spec: TextSpec = {
      text: locale.t("editor.text.placeholder"),
      ...$state.snapshot(textDraft),
      color,
      x: Math.round(x),
      y: Math.round(y - textDraft.size / 2),
    };
    insertAbove({ ...newLayerRecord(renderText(spec)), name: textName(spec), text: spec }, { op: "text", n: 1 });
    void tick().then(() => {
      textInput?.focus();
      textInput?.select();
    });
  }

  /** Restyle the active text layer. Live while typing; `record` makes it one undo step. */
  function updateText(patch: Partial<TextSpec>, record: boolean) {
    if (patch.font !== undefined) textDraft.font = patch.font;
    if (patch.size !== undefined) textDraft.size = patch.size;
    if (patch.bold !== undefined) textDraft.bold = patch.bold;
    if (patch.italic !== undefined) textDraft.italic = patch.italic;
    const layer = activeLayer;
    if (busy || !layer?.text) return;
    const text = { ...layer.text, ...patch };
    const next = withActive({ text, name: textName(text), canvas: renderText(text) });
    if (record) commit(next);
    else {
      layers = next;
      docVersion++;
    }
  }

  function commitText() {
    const before = history[historyIndex]?.layers.find((l) => l.id === activeId);
    if (before && activeLayer && before.text !== activeLayer.text) commit(layers);
  }

  // ------------------------------------------------------------------ move

  const boundsCache = new WeakMap<HTMLCanvasElement, Rect | null>();

  /** The box around a layer's pixels, found on a small copy so 4K layers stay quick. */
  function layerBounds(layer: Layer): Rect | null {
    const canvas = layer.canvas;
    const cached = boundsCache.get(canvas);
    if (cached !== undefined) return cached;
    const scale = Math.min(1, 512 / Math.max(canvas.width, canvas.height));
    const w = Math.max(1, Math.ceil(canvas.width * scale));
    const h = Math.max(1, Math.ceil(canvas.height * scale));
    const small = makeCanvas(w, h);
    const ctx = small.getContext("2d", { willReadFrequently: true })!;
    ctx.drawImage(canvas, 0, 0, w, h);
    const box = alphaBounds(ctx.getImageData(0, 0, w, h).data, w, h);
    let bounds: Rect | null = null;
    if (box) {
      const kx = canvas.width / w;
      const ky = canvas.height / h;
      const x = Math.floor(box.x * kx);
      const y = Math.floor(box.y * ky);
      bounds = {
        x,
        y,
        w: Math.min(canvas.width, Math.ceil((box.x + box.w) * kx)) - x,
        h: Math.min(canvas.height, Math.ceil((box.y + box.h) * ky)) - y,
      };
    }
    boundsCache.set(canvas, bounds);
    return bounds;
  }

  /** The layer redrawn so that its box `from` lands on `to`. Type is set again rather than resampled. */
  function movedLayer(layer: Layer, from: Rect, to: Rect): Layer {
    const bake = (source: HTMLCanvasElement) => {
      const out = makeCanvas(docW, docH);
      const ctx = out.getContext("2d")!;
      applyXform(ctx, from, to);
      ctx.drawImage(source, 0, 0);
      return out;
    };
    const mask = layer.mask ? bake(layer.mask) : null;
    if (layer.text) {
      const scale = to.w / from.w;
      const text: TextSpec = {
        ...layer.text,
        size: clampTextSize(layer.text.size * scale),
        x: to.x + (layer.text.x - from.x) * scale,
        y: to.y + (layer.text.y - from.y) * scale,
      };
      return { ...layer, text, mask, canvas: renderText(text) };
    }
    return { ...layer, mask, canvas: bake(layer.canvas) };
  }

  function finishMove() {
    const move = xform;
    xform = null;
    docVersion++;
    const layer = move ? layers.find((l) => l.id === move.id) : null;
    if (!move || !layer) return;
    const { from, to } = move;
    if (from.x === to.x && from.y === to.y && from.w === to.w && from.h === to.h) return;
    const moved = movedLayer(layer, from, to);
    if (moved.text) textDraft.size = moved.text.size;
    commit(layers.map((l) => (l.id === layer.id ? moved : l)), { op: "move", n: 1 });
  }

  let nudged: { index: number; time: number } | null = null;

  /** Arrow keys. A quick run of taps is one undo step, not one each. */
  function nudge(dx: number, dy: number) {
    const layer = activeLayer;
    if (busy || stroke || drag || !layer || !layer.visible) return;
    const from = { x: 0, y: 0, w: 1, h: 1 };
    const next = withActive(movedLayer(layer, from, { x: dx, y: dy, w: 1, h: 1 }));
    const now = performance.now();
    if (nudged && nudged.index === historyIndex && historyIndex !== savedIndex && now - nudged.time < 1000) {
      history[historyIndex] = { ...history[historyIndex], layers: next };
      showLayers(next, activeId);
    } else {
      commit(next, { op: "move", n: 1 });
    }
    nudged = { index: historyIndex, time: now };
  }

  function textLayerAt(x: number, y: number): Layer | null {
    for (let i = layers.length - 1; i >= 0; i--) {
      const layer = layers[i];
      const box = layer.visible && layer.text ? layerBounds(layer) : null;
      if (box && x >= box.x && x <= box.x + box.w && y >= box.y && y <= box.y + box.h) return layer;
    }
    return null;
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

  /** Run one geometric change over every layer and record it. */
  function transformAll(
    fn: (source: HTMLCanvasElement) => HTMLCanvasElement,
    op: EditOp,
    /** Where a line of type ends up; null turns it into plain pixels. */
    textFn: (text: TextSpec) => TextSpec | null = () => null,
  ) {
    selection = null;
    cloneSource = null;
    cloneOffset = null;
    commit(
      layers.map((l) => {
        const canvas = fn(l.canvas);
        const text = l.text ? textFn(l.text) : null;
        return {
          ...l,
          canvas: text ? renderText(text, canvas.width, canvas.height) : canvas,
          mask: l.mask ? fn(l.mask) : null,
          text,
        };
      }),
      op,
    );
  }

  function rotate(deg: 90 | -90) {
    if (busy || layers.length === 0) return;
    cancelCrop();
    transformAll((source) => {
      const out = makeCanvas(source.height, source.width);
      const ctx = out.getContext("2d")!;
      ctx.translate(out.width / 2, out.height / 2);
      ctx.rotate((deg * Math.PI) / 180);
      ctx.drawImage(source, -source.width / 2, -source.height / 2);
      return out;
    }, { op: "rotate", deg });
    fitView(true);
  }

  function flip(axis: "h" | "v") {
    if (busy || layers.length === 0) return;
    cancelCrop();
    transformAll((source) => {
      const out = makeCanvas(source.width, source.height);
      const ctx = out.getContext("2d")!;
      if (axis === "h") {
        ctx.translate(out.width, 0);
        ctx.scale(-1, 1);
      } else {
        ctx.translate(0, out.height);
        ctx.scale(1, -1);
      }
      ctx.drawImage(source, 0, 0);
      return out;
    }, { op: "flip", axis });
  }

  function setTool(next: Tool) {
    if (next === "crop") {
      startCrop();
      return;
    }
    cancelCrop();
    tool = next;
  }

  function startCrop() {
    if (busy || layers.length === 0) return;
    tool = "crop";
    sizeOpen = false;
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
    if (!crop || busy) return;
    const rect = roundRect(crop, docW, docH);
    if (rect.w === docW && rect.h === docH) {
      cancelCrop();
      return;
    }
    crop = null;
    tool = "hand";
    transformAll((source) => {
      const out = makeCanvas(rect.w, rect.h);
      out.getContext("2d")!.drawImage(source, rect.x, rect.y, rect.w, rect.h, 0, 0, rect.w, rect.h);
      return out;
    }, { op: "crop", w: rect.w, h: rect.h }, (text) => ({ ...text, x: text.x - rect.x, y: text.y - rect.y }));
    fitView(true);
  }

  function openSize() {
    if (busy || layers.length === 0) return;
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
    if (busy || layers.length === 0) return;
    const w = Math.min(MAX_DIMENSION, Math.max(1, Math.round(Number(sizeW) || 0)));
    const h = Math.min(MAX_DIMENSION, Math.max(1, Math.round(Number(sizeH) || 0)));
    sizeOpen = false;
    if (w === docW && h === docH) return;
    const sx = w / docW;
    const sy = h / docH;
    transformAll((source) => {
      // Halve repeatedly on big reductions: one drawImage step samples too
      // few source pixels and aliases.
      let current = source;
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
      return out;
    }, { op: "resize", w, h }, (text) =>
      // A stretched resize cannot be set as type again.
      Math.abs(sx - sy) > 0.005 ? null : { ...text, size: clampTextSize(text.size * sy), x: text.x * sx, y: text.y * sy },
    );
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
      if (first) fitView(true);
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

  /** The canvas shown on screen: the flattened layers, or their adjusted preview. */
  function viewSource(): HTMLCanvasElement | null {
    const flat = composite();
    if (!flat) return null;
    if (isNeutral(adjustments)) return flat;
    if (
      adjusted &&
      adjustedFor &&
      adjustedFor.version === docVersion &&
      sameAdjustments(adjustedFor.adjustments, adjustments)
    ) {
      return adjusted;
    }
    const scale = Math.min(1, PREVIEW_MAX / Math.max(flat.width, flat.height));
    const width = Math.max(1, Math.round(flat.width * scale));
    const height = Math.max(1, Math.round(flat.height * scale));
    if (!adjusted || adjusted.width !== width || adjusted.height !== height) adjusted = makeCanvas(width, height);
    const ctx = adjusted.getContext("2d", { willReadFrequently: true })!;
    ctx.globalCompositeOperation = "copy";
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(flat, 0, 0, width, height);
    const data = ctx.getImageData(0, 0, width, height);
    applyAdjustments(data.data, width, height, adjustments, scale);
    ctx.putImageData(data, 0, 0);
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

    if (selection) {
      // Two offset dash passes read on any background.
      ctx.save();
      ctx.translate(panX, panY);
      ctx.scale(zoom, zoom);
      const path = selectionOutline(selection);
      if (selection.inverted) path.rect(0, 0, docW, docH);
      ctx.lineWidth = 1 / zoom;
      ctx.setLineDash([5 / zoom, 5 / zoom]);
      ctx.strokeStyle = "#ffffff";
      ctx.stroke(path);
      ctx.lineDashOffset = 5 / zoom;
      ctx.strokeStyle = "#000000";
      ctx.stroke(path);
      ctx.restore();
    }

    if ((tool === "move" || tool === "text") && activeLayer?.visible && (tool === "move" || activeLayer.text)) {
      const box = xform && xform.id === activeLayer.id ? xform.to : layerBounds(activeLayer);
      if (box) {
        const bx = Math.round(panX + box.x * zoom) + 0.5;
        const by = Math.round(panY + box.y * zoom) + 0.5;
        ctx.lineWidth = 1;
        ctx.strokeStyle = "#818cf8";
        ctx.strokeRect(bx, by, Math.round(box.w * zoom), Math.round(box.h * zoom));
        if (tool === "move") {
          ctx.fillStyle = "#ffffff";
          for (const [hx, hy] of handlePoints(box)) {
            const px = Math.round(panX + hx * zoom);
            const py = Math.round(panY + hy * zoom);
            ctx.fillRect(px - 4, py - 4, 8, 8);
            ctx.strokeRect(px - 3.5, py - 3.5, 7, 7);
          }
        }
      }
    }

    if (tool === "clone" && cloneSource) {
      const sx = panX + cloneSource[0] * zoom;
      const sy = panY + cloneSource[1] * zoom;
      ctx.strokeStyle = "#ffffff";
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(sx - 7, sy + 0.5);
      ctx.lineTo(sx + 7, sy + 0.5);
      ctx.moveTo(sx + 0.5, sy - 7);
      ctx.lineTo(sx + 0.5, sy + 7);
      ctx.stroke();
    }

    if (hover && isPaintTool(tool) && !spaceDown) {
      const radius = Math.max(1, (brush.size / 2) * zoom);
      ctx.lineWidth = 1;
      ctx.strokeStyle = "rgba(0, 0, 0, 0.8)";
      ctx.beginPath();
      ctx.arc(hover[0], hover[1], radius + 1, 0, Math.PI * 2);
      ctx.stroke();
      ctx.strokeStyle = "#ffffff";
      ctx.beginPath();
      ctx.arc(hover[0], hover[1], radius, 0, Math.PI * 2);
      ctx.stroke();
    }

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
    void selection;
    void hover;
    void tool;
    void spaceDown;
    void cloneSource;
    void activeId;
    void brush.size;
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

  /** Which part of the active layer's box is under the pointer; "move" anywhere else. */
  function hitMove(sx: number, sy: number): CropHandle {
    const box = activeLayer ? layerBounds(activeLayer) : null;
    if (!box) return "move";
    for (const [hx, hy, handle] of handlePoints(box)) {
      if (Math.abs(panX + hx * zoom - sx) <= HANDLE_HIT && Math.abs(panY + hy * zoom - sy) <= HANDLE_HIT) {
        return handle;
      }
    }
    return "move";
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
    | { kind: "paint" }
    | { kind: "move"; handle: CropHandle; x: number; y: number; from: Rect }
    | { kind: "marquee"; ax: number; ay: number }
    | { kind: "lasso"; points: [number, number][] }
    | null = null;
  let dragging = $state(false);
  let hoverCursor = $state("default");

  function stagePoint(e: PointerEvent): [number, number] {
    const rect = stageEl!.getBoundingClientRect();
    return [e.clientX - rect.left, e.clientY - rect.top];
  }

  const clampX = (x: number) => Math.min(docW, Math.max(0, x));
  const clampY = (y: number) => Math.min(docH, Math.max(0, y));

  function onPointerDown(e: PointerEvent) {
    if (phase !== "ready" || !stageEl || drag) return;
    const [sx, sy] = stagePoint(e);
    const ix = (sx - panX) / zoom;
    const iy = (sy - panY) / zoom;
    const panning = spaceDown || e.button === 1 || (tool === "hand" && e.button === 0);
    if (panning) {
      drag = { kind: "pan", x: e.clientX, y: e.clientY, panX, panY };
    } else if (e.button !== 0 || saving) {
      return;
    } else if (tool === "zoom") {
      zoomTo(zoom * (e.altKey ? 0.5 : 2), sx, sy);
      return;
    } else if (tool === "eyedropper" || (tool === "brush" && e.altKey)) {
      pickColor(ix, iy);
      return;
    } else if (tool === "clone" && e.altKey) {
      cloneSource = [clampX(ix), clampY(iy)];
      cloneOffset = null;
      return;
    } else if (isPaintTool(tool)) {
      if (!beginStroke(tool, ix, iy)) return;
      drag = { kind: "paint" };
    } else if (tool === "move" || tool === "text") {
      if (tool === "text") {
        const hit = textLayerAt(ix, iy);
        if (!hit) {
          createText(ix, iy);
          e.preventDefault();
          return;
        }
        selectLayer(hit.id);
      }
      const layer = layers.find((l) => l.id === activeId);
      const from = layer?.visible ? layerBounds(layer) : null;
      if (!from) return;
      drag = { kind: "move", handle: tool === "move" ? hitMove(sx, sy) : "move", x: e.clientX, y: e.clientY, from };
    } else if (tool === "marquee") {
      selection = null;
      drag = { kind: "marquee", ax: clampX(ix), ay: clampY(iy) };
    } else if (tool === "lasso") {
      selection = null;
      drag = { kind: "lasso", points: [[clampX(ix), clampY(iy)]] };
    } else if (tool === "crop") {
      const handle = hitCrop(sx, sy);
      drag = {
        kind: "crop",
        handle,
        x: e.clientX,
        y: e.clientY,
        start: handle === "new" || !crop ? { x: clampX(ix), y: clampY(iy), w: 0, h: 0 } : { ...crop },
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
    if (!stageEl) return;
    const [sx, sy] = stagePoint(e);
    const ix = (sx - panX) / zoom;
    const iy = (sy - panY) / zoom;
    if (isPaintTool(tool)) hover = [sx, sy];
    if (!drag) {
      if (tool === "crop" && !spaceDown) hoverCursor = HANDLE_CURSORS[hitCrop(sx, sy)];
      else if (tool === "move" && !spaceDown) hoverCursor = HANDLE_CURSORS[hitMove(sx, sy)];
      return;
    }
    if (drag.kind === "pan") {
      panX = drag.panX + (e.clientX - drag.x);
      panY = drag.panY + (e.clientY - drag.y);
    } else if (drag.kind === "paint") {
      extendStroke(ix, iy);
    } else if (drag.kind === "move") {
      let dx = (e.clientX - drag.x) / zoom;
      let dy = (e.clientY - drag.y) / zoom;
      if (drag.handle === "move") {
        // Whole pixels keep a moved layer sharp.
        dx = Math.round(dx);
        dy = Math.round(dy);
      }
      // Corners and edges keep the proportions; Shift lets go of them. Type always keeps them.
      const proportional = !e.shiftKey || !!activeLayer?.text;
      xform = { id: activeId, from: drag.from, to: scaleRect(drag.from, drag.handle, dx, dy, proportional) };
      docVersion++;
    } else if (drag.kind === "marquee") {
      const r = rectFromAnchor(drag.ax, drag.ay, ix, iy, docW, docH, null);
      const x0 = Math.round(r.x);
      const y0 = Math.round(r.y);
      const x1 = Math.round(r.x + r.w);
      const y1 = Math.round(r.y + r.h);
      selection = x1 > x0 && y1 > y0 ? { points: [[x0, y0], [x1, y0], [x1, y1], [x0, y1]], inverted: false } : null;
    } else if (drag.kind === "lasso") {
      const last = drag.points[drag.points.length - 1];
      const px = clampX(ix);
      const py = clampY(iy);
      // Skip sub-pixel jitter so the outline stays light.
      if (Math.hypot(px - last[0], py - last[1]) * zoom >= 2) {
        drag.points.push([px, py]);
        selection = drag.points.length >= 3 ? { points: [...drag.points], inverted: false } : null;
      }
    } else {
      const dx = (e.clientX - drag.x) / zoom;
      const dy = (e.clientY - drag.y) / zoom;
      crop = dragCropRect(drag.start, drag.handle, dx, dy, docW, docH, cropRatio);
    }
  }

  function onPointerUp(e: PointerEvent) {
    if (!drag) return;
    if (drag.kind === "crop") {
      // A click or a sliver is not a new crop: keep the one that was there.
      if (crop && (crop.w < 2 || crop.h < 2)) crop = drag.previous ?? fitAspectRect(docW, docH, cropRatio);
    } else if (drag.kind === "paint") {
      endStroke();
    } else if (drag.kind === "move") {
      finishMove();
    } else if (drag.kind === "lasso" && drag.points.length < 3) {
      // A plain click with a selection tool deselects.
      selection = null;
    }
    drag = null;
    dragging = false;
    if (stageEl?.hasPointerCapture(e.pointerId)) stageEl.releasePointerCapture(e.pointerId);
  }

  const cursor = $derived.by(() => {
    if (phase !== "ready") return "default";
    if (spaceDown || tool === "hand") return dragging ? "grabbing" : "grab";
    if (tool === "zoom") return "zoom-in";
    if (tool === "crop" || tool === "move") return hoverCursor;
    if (tool === "text") return "text";
    return "crosshair";
  });

  // ------------------------------------------------------------------ save

  async function renderPng(): Promise<number[]> {
    const flat = composite();
    if (!flat) throw new Error("no document");
    let out = flat;
    if (!isNeutral(adjustments)) {
      out = makeCanvas(flat.width, flat.height);
      const ctx = out.getContext("2d", { willReadFrequently: true })!;
      ctx.drawImage(flat, 0, 0);
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
    if (busy || stroke || layers.length === 0) return;
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
    if (busy || stroke || layers.length === 0) return;
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

  /** True only for fields that take text, so sliders and the colour swatch keep shortcuts working. */
  function isTyping(target: EventTarget | null): boolean {
    if (target instanceof HTMLTextAreaElement) return true;
    return target instanceof HTMLInputElement && ["text", "number", "search"].includes(target.type);
  }

  function onKeyDown(e: KeyboardEvent) {
    // Capture phase on window: the app's own shortcuts (inpaint canvas tools,
    // lightbox arrows) must not fire underneath the editor.
    e.stopPropagation();
    const mod = e.ctrlKey || e.metaKey;
    const key = e.key.toLowerCase();

    if (key === "escape") {
      e.preventDefault();
      if (textInput && e.target === textInput) textInput.blur();
      else if (presetNaming) presetNaming = false;
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
      } else if (key === "enter" && e.target instanceof HTMLInputElement) {
        // Leaving the field records the text or size as one undo step.
        e.target.blur();
      }
      return;
    }
    if (phase !== "ready") return;
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
      } else if (key === "i" && e.shiftKey) {
        e.preventDefault();
        invertSelection();
      } else if (key === "a") {
        e.preventDefault();
        selectAll();
      } else if (key === "d") {
        e.preventDefault();
        selection = null;
      } else if (key === "j") {
        e.preventDefault();
        duplicateLayer();
      } else if (key === "n" && e.shiftKey) {
        e.preventDefault();
        addLayer();
      } else if (key === "e") {
        e.preventDefault();
        mergeDown();
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
    if (key === "backspace" || key === "delete") {
      e.preventDefault();
      if (e.altKey) fillSelection();
      else clearSelection();
      return;
    }
    if (e.altKey || stroke) return;
    if (key.startsWith("arrow") && (tool === "move" || (tool === "text" && textSpec))) {
      e.preventDefault();
      const step = e.shiftKey ? 10 : 1;
      nudge(key === "arrowleft" ? -step : key === "arrowright" ? step : 0, key === "arrowup" ? -step : key === "arrowdown" ? step : 0);
      return;
    }
    if (key === "[" || key === "{") {
      if (e.shiftKey) brush.hardness = Math.max(0, brush.hardness - 25);
      else brush.size = Math.max(1, Math.round(brush.size * 0.85) - 1);
    } else if (key === "]" || key === "}") {
      if (e.shiftKey) brush.hardness = Math.min(100, brush.hardness + 25);
      else brush.size = Math.min(MAX_BRUSH, Math.round(brush.size * 1.15) + 1);
    } else {
      const next = TOOL_KEYS[key];
      if (next) setTool(next);
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
  const optionButton =
    "h-7 px-2.5 rounded-md text-neutral-300 hover:bg-neutral-800 hover:text-neutral-100 transition-colors disabled:opacity-35 disabled:hover:bg-transparent";
  const smallButton =
    "flex items-center justify-center w-7 h-7 rounded text-neutral-400 hover:bg-neutral-800 hover:text-neutral-100 transition-colors disabled:opacity-30 disabled:hover:bg-transparent";
  const RAIL: { tool: Tool; icon: typeof Hand; gap?: boolean }[] = [
    { tool: "move", icon: Move },
    { tool: "hand", icon: Hand },
    { tool: "zoom", icon: Search },
    { tool: "crop", icon: Crop },
    { tool: "marquee", icon: SquareDashed, gap: true },
    { tool: "lasso", icon: Lasso },
    { tool: "brush", icon: Brush, gap: true },
    { tool: "eraser", icon: Eraser },
    { tool: "clone", icon: Stamp },
    { tool: "eyedropper", icon: Pipette },
    { tool: "text", icon: Type, gap: true },
  ];
  const toggleChip = (on: boolean) =>
    `h-6 px-2 rounded text-[11px] transition-colors disabled:opacity-35 ${
      on ? "bg-indigo-600 text-white" : "bg-neutral-800 text-neutral-300 hover:bg-neutral-700"
    }`;
  const field = "h-7 px-2 rounded bg-neutral-800 border border-neutral-700 text-neutral-100 disabled:opacity-40";
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

    <!-- Options for the active tool -->
    <div class="flex items-center gap-1 h-10 px-3 shrink-0 bg-neutral-900/70 border-b border-neutral-800 text-xs">
      {#if tool === "crop" && crop}
        {#each CROP_PRESETS as preset (preset.key)}
          <button
            type="button"
            class="h-7 px-2.5 rounded-md transition-colors {cropPreset === preset.key ? 'bg-indigo-600 text-white' : 'text-neutral-300 hover:bg-neutral-800'}"
            aria-pressed={cropPreset === preset.key}
            onclick={() => setCropPreset(preset.key)}
          >{preset.label || locale.t(`editor.crop.${preset.key}`)}</button>
        {/each}
        <span class="px-3 tabular-nums text-neutral-400">{Math.round(crop.w)} × {Math.round(crop.h)}</span>
        <span class="flex-1"></span>
        <button type="button" class="h-7 px-3 rounded-md bg-neutral-800 hover:bg-neutral-700 text-neutral-100 font-medium" onclick={cancelCrop}>{locale.t("common.cancel")}</button>
        <button type="button" class="h-7 px-3 rounded-md bg-indigo-600 hover:bg-indigo-500 text-white font-medium" onclick={applyCrop}>{locale.t("common.apply")}</button>
      {:else if isPaintTool(tool) || tool === "eyedropper"}
        {#if onMask && (tool === "brush" || tool === "eraser")}
          <span class="flex items-center gap-1.5 h-6 px-2 rounded bg-indigo-600/25 text-indigo-200">
            <VenetianMask size={13} />{locale.t("editor.mask")}: {locale.t(tool === "brush" ? "editor.mask.hide" : "editor.mask.reveal")}
          </span>
        {:else if tool === "brush" || tool === "eyedropper"}
          <input type="color" class="w-7 h-7 p-0 rounded border border-neutral-700 bg-transparent cursor-pointer" title={locale.t("editor.brush.color")} aria-label={locale.t("editor.brush.color")} bind:value={color} />
        {/if}
        {#if isPaintTool(tool)}
          <label class="flex items-center gap-2 pl-2 text-neutral-400">
            {locale.t("editor.brush.size")}
            <input type="range" min="1" max={MAX_BRUSH} step="1" class="w-28 accent-indigo-500" bind:value={brush.size} />
            <span class="w-8 tabular-nums text-neutral-200">{brush.size}</span>
          </label>
          <label class="flex items-center gap-2 pl-2 text-neutral-400">
            {locale.t("editor.brush.hardness")}
            <input type="range" min="0" max="100" step="1" class="w-24 accent-indigo-500" bind:value={brush.hardness} />
            <span class="w-8 tabular-nums text-neutral-200">{brush.hardness}</span>
          </label>
          <label class="flex items-center gap-2 pl-2 text-neutral-400">
            {locale.t("editor.brush.opacity")}
            <input type="range" min="1" max="100" step="1" class="w-24 accent-indigo-500" bind:value={brush.opacity} />
            <span class="w-8 tabular-nums text-neutral-200">{brush.opacity}</span>
          </label>
          {#if tool === "clone" && !cloneSource}
            <span class="pl-3 text-amber-300">{locale.t("editor.clone_hint")}</span>
          {/if}
        {/if}
      {:else if tool === "text"}
        <input
          bind:this={textInput}
          type="text"
          maxlength="200"
          class="{field} w-56 select-text"
          placeholder={locale.t("editor.text.placeholder")}
          aria-label={locale.t("editor.text.placeholder")}
          disabled={busy || !textSpec}
          value={textSpec?.text ?? ""}
          oninput={(e) => updateText({ text: e.currentTarget.value }, false)}
          onchange={commitText}
        />
        <select
          class="{field} w-36"
          title={locale.t("editor.text.font")}
          aria-label={locale.t("editor.text.font")}
          disabled={busy}
          value={textSpec?.font ?? textDraft.font}
          onchange={(e) => { updateText({ font: e.currentTarget.value }, true); e.currentTarget.blur(); }}
        >
          {#each FONTS as font (font)}
            <option value={font}>{font}</option>
          {/each}
        </select>
        <input
          type="number"
          min={MIN_TEXT}
          max={MAX_TEXT}
          class="{field} w-16 select-text"
          title={locale.t("editor.brush.size")}
          aria-label={locale.t("editor.brush.size")}
          disabled={busy}
          value={textSpec?.size ?? textDraft.size}
          onchange={(e) => updateText({ size: clampTextSize(Number(e.currentTarget.value)) }, true)}
        />
        <button type="button" class="{smallButton} {(textSpec?.bold ?? textDraft.bold) ? 'bg-indigo-600 text-white' : ''}" title={locale.t("editor.text.bold")} aria-label={locale.t("editor.text.bold")} aria-pressed={textSpec?.bold ?? textDraft.bold} disabled={busy} onclick={() => updateText({ bold: !(textSpec?.bold ?? textDraft.bold) }, true)}><Bold size={14} /></button>
        <button type="button" class="{smallButton} {(textSpec?.italic ?? textDraft.italic) ? 'bg-indigo-600 text-white' : ''}" title={locale.t("editor.text.italic")} aria-label={locale.t("editor.text.italic")} aria-pressed={textSpec?.italic ?? textDraft.italic} disabled={busy} onclick={() => updateText({ italic: !(textSpec?.italic ?? textDraft.italic) }, true)}><Italic size={14} /></button>
        <input
          type="color"
          class="w-7 h-7 p-0 rounded border border-neutral-700 bg-transparent cursor-pointer"
          title={locale.t("editor.brush.color")}
          aria-label={locale.t("editor.brush.color")}
          value={color}
          oninput={(e) => { color = e.currentTarget.value; updateText({ color }, false); }}
          onchange={commitText}
        />
      {:else if tool === "marquee" || tool === "lasso"}
        <button type="button" class={optionButton} disabled={busy} onclick={selectAll}>{locale.t("editor.select.all")}</button>
        <button type="button" class={optionButton} disabled={busy || !selection} onclick={() => (selection = null)}>{locale.t("editor.select.none")}</button>
        <button type="button" class={optionButton} disabled={busy} onclick={invertSelection}>{locale.t("editor.select.invert")}</button>
        <span class="w-px h-5 mx-1 bg-neutral-700"></span>
        <input type="color" class="w-7 h-7 p-0 rounded border border-neutral-700 bg-transparent cursor-pointer" title={locale.t("editor.brush.color")} aria-label={locale.t("editor.brush.color")} bind:value={color} />
        <button type="button" class={optionButton} disabled={busy || !activeLayer?.visible} onclick={fillSelection}>{locale.t("editor.select.fill")}</button>
        <button type="button" class={optionButton} disabled={busy || !selection || !activeLayer?.visible} onclick={clearSelection}>{locale.t("editor.select.clear")}</button>
      {:else}
        <button type="button" class={optionButton} onclick={() => fitView()}>{locale.t("editor.fit")}</button>
        <button type="button" class={optionButton} onclick={() => zoomTo(1)}>100%</button>
      {/if}
    </div>

    <div class="flex flex-1 min-h-0">
      <!-- Tool rail -->
      <div class="flex flex-col items-center gap-1 w-12 py-2 shrink-0 bg-neutral-900 border-r border-neutral-800">
        {#each RAIL as item (item.tool)}
          {#if item.gap}<span class="w-6 h-px my-1 bg-neutral-800"></span>{/if}
          <button
            type="button"
            class={railButton(tool === item.tool)}
            title={locale.t(`editor.tool.${item.tool}`)}
            aria-label={locale.t(`editor.tool.${item.tool}`)}
            aria-pressed={tool === item.tool}
            disabled={phase !== "ready"}
            onclick={() => setTool(item.tool)}
          ><item.icon size={18} /></button>
        {/each}
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
        onpointerleave={() => (hover = null)}
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
        <div class="flex flex-col flex-1 min-h-0 gap-4 p-3 overflow-y-auto">
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

        <!-- Layers -->
        <div class="flex flex-col h-72 shrink-0 border-t border-neutral-800">
          <div class="flex items-center h-9 pl-3 pr-1 border-b border-neutral-800">
            <span class="flex-1 min-w-0 truncate text-xs font-semibold uppercase tracking-wide text-neutral-400">{locale.t("editor.layers")}</span>
            <button type="button" class={smallButton} title={locale.t("editor.layer.new")} aria-label={locale.t("editor.layer.new")} disabled={busy || layers.length >= MAX_LAYERS} onclick={addLayer}><Plus size={14} /></button>
            <button type="button" class={smallButton} title={locale.t("editor.layer.duplicate")} aria-label={locale.t("editor.layer.duplicate")} disabled={busy || layers.length >= MAX_LAYERS} onclick={duplicateLayer}><Copy size={14} /></button>
            <button type="button" class={smallButton} title={locale.t("editor.mask.add")} aria-label={locale.t("editor.mask.add")} disabled={busy || !activeLayer || !!activeLayer.mask} onclick={addMask}><VenetianMask size={14} /></button>
            <button type="button" class={smallButton} title={locale.t("editor.layer.merge")} aria-label={locale.t("editor.layer.merge")} disabled={busy || activeIndex <= 0} onclick={mergeDown}><MergeDown size={14} /></button>
            <button type="button" class={smallButton} title={locale.t("editor.layer.up")} aria-label={locale.t("editor.layer.up")} disabled={busy || activeIndex < 0 || activeIndex >= layers.length - 1} onclick={() => moveLayer(1)}><ArrowUp size={14} /></button>
            <button type="button" class={smallButton} title={locale.t("editor.layer.down")} aria-label={locale.t("editor.layer.down")} disabled={busy || activeIndex <= 0} onclick={() => moveLayer(-1)}><ArrowDown size={14} /></button>
            <button type="button" class={smallButton} title={locale.t("editor.layer.delete")} aria-label={locale.t("editor.layer.delete")} disabled={busy || layers.length <= 1} onclick={deleteLayer}><Trash2 size={14} /></button>
          </div>
          {#if activeLayer}
            <div class="flex items-center gap-2 h-9 px-2 border-b border-neutral-800 text-xs text-neutral-400">
              <select
                class="h-6 w-28 px-1 rounded bg-neutral-800 border border-neutral-700 text-[11px] text-neutral-100 disabled:opacity-40"
                title={locale.t("editor.layer.blend")}
                aria-label={locale.t("editor.layer.blend")}
                disabled={busy}
                value={activeLayer.blend}
                onchange={(e) => { setBlend(e.currentTarget.value as BlendMode); e.currentTarget.blur(); }}
              >
                {#each BLEND_MODES as mode (mode)}
                  <option value={mode}>{locale.t(`editor.blend.${mode.replace("-", "_")}`)}</option>
                {/each}
              </select>
              <input
                type="range"
                min="0"
                max="100"
                step="1"
                class="flex-1 min-w-0 accent-indigo-500"
                title={locale.t("editor.brush.opacity")}
                aria-label={locale.t("editor.brush.opacity")}
                disabled={busy}
                value={activeLayer.opacity}
                oninput={(e) => previewLayerOpacity(Number(e.currentTarget.value))}
                onchange={commitLayerOpacity}
              />
              <span class="w-7 text-right tabular-nums text-neutral-200">{activeLayer.opacity}</span>
            </div>
            {#if activeLayer.mask}
              <div class="flex items-center gap-1 h-8 pl-3 pr-1 border-b border-neutral-800 text-xs">
                <span class="flex-1 min-w-0 truncate text-neutral-400">{locale.t("editor.mask")}</span>
                <button type="button" class={toggleChip(onMask)} aria-pressed={onMask} disabled={busy || !activeLayer.maskOn} onclick={() => (maskEdit = !maskEdit)}>{locale.t("editor.mask.paint")}</button>
                <button type="button" class={toggleChip(activeLayer.maskOn)} aria-pressed={activeLayer.maskOn} disabled={busy} onclick={toggleMask}>{locale.t("editor.mask.on")}</button>
                <button type="button" class={smallButton} title={locale.t("editor.mask.delete")} aria-label={locale.t("editor.mask.delete")} disabled={busy} onclick={deleteMask}><Trash2 size={13} /></button>
              </div>
            {/if}
          {/if}
          <div class="flex-1 min-h-0 overflow-y-auto">
            {#each [...layers].reverse() as layer (layer.id)}
              <div class="flex items-center h-8 text-xs {layer.id === activeId ? 'bg-indigo-600/25 text-neutral-100' : 'text-neutral-300 hover:bg-neutral-800/60'}">
                <button type="button" class="flex items-center justify-center w-8 h-8 shrink-0 {layer.visible ? 'text-neutral-300' : 'text-neutral-600'} hover:text-white" title={locale.t("editor.layer.visibility")} aria-label="{locale.t('editor.layer.visibility')}: {layer.name}" aria-pressed={layer.visible} disabled={busy} onclick={() => toggleLayer(layer.id)}>
                  {#if layer.visible}<Eye size={14} />{:else}<EyeOff size={14} />{/if}
                </button>
                <button type="button" class="flex items-center gap-1.5 flex-1 min-w-0 h-8 pr-2 text-left" aria-pressed={layer.id === activeId} onclick={() => selectLayer(layer.id)}>
                  {#if layer.text}<Type size={12} class="shrink-0 text-neutral-400" />{/if}
                  <span class="truncate">{layer.name}</span>
                </button>
                {#if layer.mask}
                  <button
                    type="button"
                    class="flex items-center justify-center w-6 h-6 mr-1 shrink-0 rounded border {layer.id === activeId && onMask ? 'border-indigo-400 bg-indigo-600 text-white' : 'border-neutral-600 text-neutral-300 hover:text-white'} {layer.maskOn ? '' : 'opacity-40'}"
                    title={locale.t("editor.mask.paint")}
                    aria-label="{locale.t('editor.mask')}: {layer.name}"
                    aria-pressed={layer.id === activeId && onMask}
                    disabled={busy}
                    onclick={() => selectLayer(layer.id, true)}
                  ><VenetianMask size={12} /></button>
                {/if}
              </div>
            {/each}
          </div>
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
