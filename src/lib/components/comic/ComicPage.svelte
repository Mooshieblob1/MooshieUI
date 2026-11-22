<script lang="ts">
  // Comic book builder: lay out multi-page books, assign cast and prompts,
  // generate panels (or a whole page on NovelAI), check continuity, and export
  // to the gallery, CBZ, or PDF.
  import { onMount } from "svelte";
  import { comic } from "../../stores/comic.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { shortcuts } from "../../stores/shortcuts.svelte.js";
  import { isTypingTarget } from "../../utils/shortcuts.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { architectureLabel, savedCharacters, type SavedCharacter } from "../../stores/savedCharacters.svelte.js";
  import {
    LAYOUT_TEMPLATES,
    MAX_PANELS,
    ONE_SHOT_MAX_PANELS,
    PANEL_BORDER_STYLES,
    buildTemplate,
    clampFreeformBox,
    clampPan,
    clampZoom,
    coverCrop,
    dividers,
    freeformRects,
    normalizePanelBorderStyle,
    pageAspect,
    pageSize,
    panelBorderPolygon,
    panelBorderRadiusCss,
    panelClipPathCss,
    panelCornerRadius,
    panelImageCss,
    panelPixelRect,
    panelRects,
    panelSlantInset,
    ratioFromPointer,
    resizeFreeformBox,
    snapFreeformBox,
    type Divider,
    type FreeformBox,
    type LayoutTemplate,
    type PageShape,
    type PanelBorderStyle,
    type Rect,
    type RefMode,
    type ResizeHandle,
  } from "../../utils/comicLayout.js";
  import {
    BUBBLE_FONTS,
    BUBBLE_SHAPES,
    bubbleFontFamily,
    bubbleFontPx,
    bubblePathData,
    clampBubble,
    estimateTextWidth,
    layoutVerticalText,
    resizeBubble,
    wrapBubbleText,
    type BubbleCorner,
    type BubbleFont,
    type BubbleShape,
    type SpeechBubble,
  } from "../../utils/comicBubbles.js";
  import {
    assemblePage,
    checkPanel,
    checkStory,
    draftFromBeats,
    enhancePanel,
    exportBookCbz,
    exportBookPdf,
    generateOneImage,
    generatePanels,
    previewPage,
    restoreImages,
    restoreImagesForPage,
    saveAllPages,
    savePage,
    stopRun,
    supportsPanel1Reference,
    switchPanelTake,
    undoDraft,
  } from "../../utils/comicRunner.js";
  import { naiV5Variant } from "../../utils/novelaiModels.js";
  import { portal } from "../../utils/portal.js";
  import InfoTip from "../ui/InfoTip.svelte";
  import ComicPromptField from "./ComicPromptField.svelte";
  import ModelSelector from "../generation/ModelSelector.svelte";
  import SamplerSettings from "../generation/SamplerSettings.svelte";
  import DimensionControls from "../generation/DimensionControls.svelte";

  // The page's own model and settings (a hidden generation tab App.svelte swaps
  // in while this page is open), shown open until the user folds them away.
  const SETTINGS_OPEN_KEY = "mooshieui.comic.settingsOpen.v1";
  function readSettingsOpen(): boolean {
    try {
      return localStorage.getItem(SETTINGS_OPEN_KEY) !== "false";
    } catch {
      return true;
    }
  }
  let showSettings = $state(readSettingsOpen());
  function toggleSettings() {
    showSettings = !showSettings;
    try {
      localStorage.setItem(SETTINGS_OPEN_KEY, String(showSettings));
    } catch {
      /* storage blocked: the panel just opens again next time */
    }
  }

  interface FreeformDrag {
    kind: "move" | "resize";
    id: string;
    handle?: ResizeHandle;
    orig: FreeformBox;
    startClientX: number;
    startClientY: number;
    moved: boolean;
  }

  interface BubbleDrag {
    kind: "move" | "resize" | "tail";
    id: string;
    corner?: BubbleCorner;
    orig: SpeechBubble;
    startClientX: number;
    startClientY: number;
    moved: boolean;
  }

  interface PanDrag {
    id: string;
    boxPx: Rect;
    panelW: number;
    panelH: number;
    origPanX: number;
    origPanY: number;
    zoom: number;
    startClientX: number;
    startClientY: number;
    moved: boolean;
  }

  let showTemplates = $state(false);
  let showPage = $state(false);
  let modalPageUrl = $state<string | null>(null);
  let showCharPicker = $state(false);
  let pickingForCastId = $state<string | null>(null);
  let renamingBook = $state(false);
  let renameTitle = $state("");
  let error = $state<string | null>(null);
  let saving = $state(false);
  let exporting = $state(false);
  let panMode = $state(false);
  let altHeld = $state(false);
  let imgDims = $state<Record<string, { width: number; height: number }>>({});
  let canvasEl = $state<HTMLDivElement | null>(null);
  let dragging = $state<Divider | null>(null);
  let freeformDrag = $state<FreeformDrag | null>(null);
  let bubbleDrag = $state<BubbleDrag | null>(null);
  let bubbleTextUndoArmed = $state(true);
  let panDrag = $state<PanDrag | null>(null);
  let lastWheelZoom: { id: string; at: number } | null = null;
  let snapGuides = $state<Array<{ axis: "x" | "y"; pos: number }>>([]);

  onMount(() => {
    comic.load();
    void restoreImages();
  });

  const aspect = $derived(pageAspect(comic.pageShape));
  const pagePx = $derived(pageSize(comic.pageShape));
  /** A panel's box with gutters, as percentages of the page: what the saved page will look like. */
  function boxStyle(r: { x: number; y: number; w: number; h: number }): string {
    const b = panelPixelRect(r, pagePx, comic.gutter);
    return `left: ${(b.x / pagePx.width) * 100}%; top: ${(b.y / pagePx.height) * 100}%; width: ${(b.w / pagePx.width) * 100}%; height: ${(b.h / pagePx.height) * 100}%;`;
  }
  const rects = $derived(comic.rects);
  const isFreeform = $derived(comic.layoutMode === "freeform");
  const sortedFreeform = $derived([...comic.freeformBoxes].sort((a, b) => a.z - b.z));
  const selectedFreeformIdx = $derived(
    comic.selectedId ? sortedFreeform.findIndex((b) => b.id === comic.selectedId) : -1,
  );
  const selectedFreeformBox = $derived(selectedFreeformIdx >= 0 ? sortedFreeform[selectedFreeformIdx] : null);
  const lines = $derived(isFreeform ? [] : dividers(comic.layout));
  const order = $derived(comic.order);

  const borderPx = $derived(Math.max(2, Math.round(Math.max(pagePx.width, pagePx.height) / 600)));
  const freeformHaloPx = $derived(Math.max(4, Math.round(comic.gutter / 2)));

  function freeformPixelBox(b: FreeformBox): Rect {
    const x = Math.round(b.x * pagePx.width);
    const y = Math.round(b.y * pagePx.height);
    const x1 = Math.round((b.x + b.w) * pagePx.width);
    const y1 = Math.round((b.y + b.h) * pagePx.height);
    return { x, y, w: Math.max(1, x1 - x), h: Math.max(1, y1 - y) };
  }

  function svgPolygonPoints(box: Rect, style: PanelBorderStyle): string {
    return panelBorderPolygon({ x: 0, y: 0, w: box.w, h: box.h }, style)
      .map((pt) => `${pt.x},${pt.y}`)
      .join(" ");
  }

  function recordImgDims(id: string, img: HTMLImageElement) {
    if (img.naturalWidth > 0 && img.naturalHeight > 0) {
      const cur = imgDims[id];
      if (!cur || cur.width !== img.naturalWidth || cur.height !== img.naturalHeight) {
        imgDims = { ...imgDims, [id]: { width: img.naturalWidth, height: img.naturalHeight } };
      }
    }
  }

  function freeformBoxStyle(b: FreeformBox, zOrder: number): string {
    return `left: ${b.x * 100}%; top: ${b.y * 100}%; width: ${b.w * 100}%; height: ${b.h * 100}%; z-index: ${zOrder + 1};`;
  }
  const oneImage = $derived(comic.mode === "one_image");
  const tooManyForOneImage = $derived(oneImage && comic.enabledPanels.length > ONE_SHOT_MAX_PANELS);
  /** On NovelAI V5 the Prompt Assistant writes panels by the V5 Enhance rules, dialogue included. */
  const v5 = $derived(generation.isNovelAi && naiV5Variant(generation.checkpoint) !== null);
  const canCharacterRef = $derived(!oneImage && supportsPanel1Reference());
  const modelName = $derived((generation.checkpoint || "").split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? "");
  const issueCount = $derived(
    Object.values(comic.results).reduce((n, r) => n + (r.issues?.length ?? 0), 0),
  );
  const anyPageHasContent = $derived(comic.pages.some((p) => comic.pageHasContent(p)));
  const sortedCharacters = $derived.by(() => {
    const cur = savedCharacters.currentArchitecture;
    return [...savedCharacters.characters].sort((a, b) => {
      const aMatch = cur && a.architecture === cur ? 0 : 1;
      const bMatch = cur && b.architecture === cur ? 0 : 1;
      if (aMatch !== bMatch) return aMatch - bMatch;
      return b.updatedAt - a.updatedAt;
    });
  });

  async function run(task: () => Promise<unknown>) {
    error = null;
    try {
      await task();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function generateAll() {
    void run(() => (oneImage ? generateOneImage() : generatePanels()));
  }

  async function openPage() {
    if (!oneImage) {
      if (modalPageUrl) {
        URL.revokeObjectURL(modalPageUrl);
        modalPageUrl = null;
      }
      await run(previewPage);
      if (comic.pageUrl) showPage = true;
      return;
    }
    if (comic.bubbles.length > 0) {
      await run(async () => {
        const blob = await assemblePage();
        if (modalPageUrl) URL.revokeObjectURL(modalPageUrl);
        modalPageUrl = URL.createObjectURL(blob);
      });
      if (modalPageUrl) showPage = true;
      return;
    }
    if (comic.pageUrl) showPage = true;
  }

  function closePagePreview() {
    showPage = false;
    if (modalPageUrl) {
      URL.revokeObjectURL(modalPageUrl);
      modalPageUrl = null;
    }
  }

  async function save() {
    saving = true;
    await run(async () => {
      await savePage();
      gallery.showToast(locale.t("comic.saved"), "success");
    });
    saving = false;
  }

  async function saveAll() {
    saving = true;
    await run(async () => {
      const saved = await saveAllPages();
      gallery.showToast(locale.t("comic.saved_all", { count: String(saved.length) }), "success");
    });
    saving = false;
  }

  async function exportCbz() {
    exporting = true;
    await run(async () => {
      const status = await exportBookCbz();
      if (status !== "cancelled") {
        gallery.showToast(locale.t("comic.exported", { name: `${comic.activeBook.title}.cbz` }), "success");
      }
    });
    exporting = false;
  }

  async function exportPdf() {
    exporting = true;
    await run(async () => {
      const status = await exportBookPdf();
      if (status !== "cancelled") {
        gallery.showToast(locale.t("comic.exported", { name: `${comic.activeBook.title}.pdf` }), "success");
      }
    });
    exporting = false;
  }

  function selectPage(index: number) {
    comic.selectPage(index);
    void restoreImagesForPage(comic.activePage.id);
  }

  function switchBook(id: string) {
    renamingBook = false;
    comic.switchBook(id);
    void restoreImages();
  }

  function startRenameBook() {
    renameTitle = comic.activeBook.title;
    renamingBook = true;
  }

  function commitRenameBook() {
    if (renameTitle.trim()) comic.renameBook(renameTitle);
    renamingBook = false;
  }

  function openCharacterPicker(forCastId: string | null = null) {
    pickingForCastId = forCastId;
    showCharPicker = true;
  }

  function chooseCharacter(ch: SavedCharacter) {
    if (pickingForCastId) {
      comic.updateCastMember(pickingForCastId, {
        savedCharacterId: ch.id,
        referenceImage: ch.reference ?? ch.thumbnail ?? null,
      });
      comic.persist();
    } else {
      comic.addCastMemberFromCharacter(ch);
    }
    showCharPicker = false;
    pickingForCastId = null;
  }

  function pickTemplate(t: LayoutTemplate) {
    comic.applyTemplate(t);
    showTemplates = false;
  }

  function startDrag(event: PointerEvent, divider: Divider) {
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    comic.beginResize();
    dragging = divider;
  }

  function startPanDrag(event: PointerEvent, id: string, boxPx: Rect) {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    comic.selectedId = id;
    const el = event.currentTarget as HTMLElement;
    el.setPointerCapture(event.pointerId);
    const rect = el.getBoundingClientRect();
    const panel = comic.panels[id];
    panDrag = {
      id,
      boxPx,
      panelW: Math.max(1, rect.width),
      panelH: Math.max(1, rect.height),
      origPanX: clampPan(panel?.panX),
      origPanY: clampPan(panel?.panY),
      zoom: clampZoom(panel?.zoom),
      startClientX: event.clientX,
      startClientY: event.clientY,
      moved: false,
    };
  }

  function onPanelPointerDown(event: PointerEvent, id: string, boxPx: Rect, freeformBox?: FreeformBox) {
    comic.selectedBubbleId = null;
    const hasImage = !!comic.results[id]?.url;
    if ((panMode || event.altKey) && hasImage) {
      startPanDrag(event, id, boxPx);
      return;
    }
    if (freeformBox) {
      startFreeformMove(event, freeformBox);
    }
  }

  function onPanelWheel(event: WheelEvent, id: string) {
    const result = comic.results[id];
    if (comic.selectedId !== id || !result?.url) return;
    event.preventDefault();
    const now = Date.now();
    if (!lastWheelZoom || lastWheelZoom.id !== id || now - lastWheelZoom.at > 600) {
      comic.beginPanelCropEdit();
    }
    lastWheelZoom = { id, at: now };
    const panel = comic.panels[id];
    const curZoom = clampZoom(panel?.zoom);
    const factor = event.deltaY < 0 ? 1.1 : 1 / 1.1;
    const nextZoom = clampZoom(Math.round(curZoom * factor * 100) / 100);
    comic.updatePanel(id, { zoom: nextZoom });
  }

  function startFreeformMove(event: PointerEvent, box: FreeformBox) {
    if (event.button !== 0) return;
    comic.selectedId = box.id;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    freeformDrag = {
      kind: "move",
      id: box.id,
      orig: { ...box },
      startClientX: event.clientX,
      startClientY: event.clientY,
      moved: false,
    };
    snapGuides = [];
  }

  function startFreeformResize(event: PointerEvent, box: FreeformBox, handle: ResizeHandle) {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    freeformDrag = {
      kind: "resize",
      id: box.id,
      handle,
      orig: { ...box },
      startClientX: event.clientX,
      startClientY: event.clientY,
      moved: false,
    };
    snapGuides = [];
  }

  function selectBubble(id: string) {
    comic.selectedBubbleId = id;
    bubbleTextUndoArmed = true;
  }

  function startBubbleMove(event: PointerEvent, bubble: SpeechBubble) {
    if (event.button !== 0) return;
    event.stopPropagation();
    selectBubble(bubble.id);
    (event.currentTarget as Element).setPointerCapture(event.pointerId);
    bubbleDrag = {
      kind: "move",
      id: bubble.id,
      orig: { ...bubble },
      startClientX: event.clientX,
      startClientY: event.clientY,
      moved: false,
    };
  }

  function startBubbleResize(event: PointerEvent, bubble: SpeechBubble, corner: BubbleCorner) {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    selectBubble(bubble.id);
    (event.currentTarget as Element).setPointerCapture(event.pointerId);
    bubbleDrag = {
      kind: "resize",
      id: bubble.id,
      corner,
      orig: { ...bubble },
      startClientX: event.clientX,
      startClientY: event.clientY,
      moved: false,
    };
  }

  function startBubbleTail(event: PointerEvent, bubble: SpeechBubble) {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    selectBubble(bubble.id);
    (event.currentTarget as Element).setPointerCapture(event.pointerId);
    bubbleDrag = {
      kind: "tail",
      id: bubble.id,
      orig: { ...bubble },
      startClientX: event.clientX,
      startClientY: event.clientY,
      moved: false,
    };
  }

  function onDrag(event: PointerEvent) {
    if (bubbleDrag && canvasEl) {
      const dist = Math.hypot(event.clientX - bubbleDrag.startClientX, event.clientY - bubbleDrag.startClientY);
      if (!bubbleDrag.moved) {
        if (dist < 3) return;
        comic.beginBubbleEdit();
        bubbleDrag.moved = true;
      }
      const box = canvasEl.getBoundingClientRect();
      const dx = (event.clientX - bubbleDrag.startClientX) / Math.max(1, box.width);
      const dy = (event.clientY - bubbleDrag.startClientY) / Math.max(1, box.height);
      if (bubbleDrag.kind === "move") {
        const nextBox = clampBubble({
          ...bubbleDrag.orig,
          x: bubbleDrag.orig.x + dx,
          y: bubbleDrag.orig.y + dy,
        });
        const adx = nextBox.x - bubbleDrag.orig.x;
        const ady = nextBox.y - bubbleDrag.orig.y;
        comic.updateBubble(bubbleDrag.id, {
          x: nextBox.x,
          y: nextBox.y,
          tailX: bubbleDrag.orig.tailX + adx,
          tailY: bubbleDrag.orig.tailY + ady,
        });
      } else if (bubbleDrag.kind === "resize" && bubbleDrag.corner) {
        const resized = resizeBubble(bubbleDrag.orig, bubbleDrag.corner, dx, dy);
        comic.updateBubble(bubbleDrag.id, {
          x: resized.x,
          y: resized.y,
          w: resized.w,
          h: resized.h,
        });
      } else if (bubbleDrag.kind === "tail") {
        comic.updateBubble(bubbleDrag.id, {
          tailX: bubbleDrag.orig.tailX + dx,
          tailY: bubbleDrag.orig.tailY + dy,
        });
      }
      return;
    }
    if (panDrag) {
      const dist = Math.hypot(event.clientX - panDrag.startClientX, event.clientY - panDrag.startClientY);
      if (!panDrag.moved) {
        if (dist < 2) return;
        comic.beginPanelCropEdit();
        panDrag.moved = true;
      }
      const src = imgDims[panDrag.id] ?? { width: panDrag.boxPx.w, height: panDrag.boxPx.h };
      const crop = coverCrop(src, panDrag.boxPx, 0, 0, panDrag.zoom);
      const overflowPxX = (src.width / crop.w - 1) * panDrag.panelW;
      const overflowPxY = (src.height / crop.h - 1) * panDrag.panelH;
      const dx = event.clientX - panDrag.startClientX;
      const dy = event.clientY - panDrag.startClientY;
      const nextPanX = overflowPxX > 0.5 ? clampPan(panDrag.origPanX - dx / (overflowPxX / 2)) : panDrag.origPanX;
      const nextPanY = overflowPxY > 0.5 ? clampPan(panDrag.origPanY - dy / (overflowPxY / 2)) : panDrag.origPanY;
      comic.updatePanel(panDrag.id, { panX: nextPanX, panY: nextPanY });
      return;
    }
    if (dragging && canvasEl) {
      const box = canvasEl.getBoundingClientRect();
      const pointer =
        dragging.dir === "row" ? (event.clientX - box.left) / box.width : (event.clientY - box.top) / box.height;
      comic.resize(dragging.splitId, ratioFromPointer(dragging, pointer));
      return;
    }
    if (!freeformDrag || !canvasEl) return;
    const dist = Math.hypot(event.clientX - freeformDrag.startClientX, event.clientY - freeformDrag.startClientY);
    if (!freeformDrag.moved) {
      if (dist < 3) return;
      comic.beginFreeformEdit();
      freeformDrag.moved = true;
    }
    const box = canvasEl.getBoundingClientRect();
    const dx = (event.clientX - freeformDrag.startClientX) / Math.max(1, box.width);
    const dy = (event.clientY - freeformDrag.startClientY) / Math.max(1, box.height);
    const others = comic.freeformBoxes.filter((b) => b.id !== freeformDrag!.id);
    const snap = !event.shiftKey;
    if (freeformDrag.kind === "move") {
      const movedBox = clampFreeformBox({
        ...freeformDrag.orig,
        x: freeformDrag.orig.x + dx,
        y: freeformDrag.orig.y + dy,
      });
      if (snap) {
        const res = snapFreeformBox(movedBox, others);
        comic.updateFreeformBox(freeformDrag.id, res.box);
        snapGuides = res.guides;
      } else {
        comic.updateFreeformBox(freeformDrag.id, movedBox);
        snapGuides = [];
      }
    } else if (freeformDrag.handle) {
      const res = resizeFreeformBox(freeformDrag.orig, freeformDrag.handle, dx, dy, others, snap);
      comic.updateFreeformBox(freeformDrag.id, res.box);
      snapGuides = res.guides;
    }
  }

  function endDrag() {
    if (dragging) comic.persist();
    if (freeformDrag?.moved) comic.persist();
    if (bubbleDrag?.moved) comic.persist();
    if (panDrag?.moved) comic.persist();
    dragging = null;
    freeformDrag = null;
    bubbleDrag = null;
    panDrag = null;
    snapGuides = [];
  }

  const RESIZE_HANDLES: Array<{ id: ResizeHandle; cls: string }> = [
    { id: "nw", cls: "-left-1.5 -top-1.5 cursor-nwse-resize" },
    { id: "n", cls: "left-1/2 -top-1.5 -translate-x-1/2 cursor-ns-resize" },
    { id: "ne", cls: "-right-1.5 -top-1.5 cursor-nesw-resize" },
    { id: "e", cls: "-right-1.5 top-1/2 -translate-y-1/2 cursor-ew-resize" },
    { id: "se", cls: "-right-1.5 -bottom-1.5 cursor-nwse-resize" },
    { id: "s", cls: "left-1/2 -bottom-1.5 -translate-x-1/2 cursor-ns-resize" },
    { id: "sw", cls: "-left-1.5 -bottom-1.5 cursor-nesw-resize" },
    { id: "w", cls: "-left-1.5 top-1/2 -translate-y-1/2 cursor-ew-resize" },
  ];

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Alt") altHeld = true;
    if (event.defaultPrevented || document.querySelector("[data-modal-open]")) return;
    if (isTypingTarget(event.target)) return;
    if (shortcuts.matches("comic.undo", event)) {
      event.preventDefault();
      comic.undo();
    } else if ((event.key === "Delete" || event.key === "Backspace") && comic.selectedBubbleId) {
      event.preventDefault();
      comic.removeBubble(comic.selectedBubbleId);
    }
  }

  function onKeyup(event: KeyboardEvent) {
    if (event.key === "Alt") altHeld = false;
  }

  const statusRing: Record<string, string> = {
    queued: "ring-2 ring-neutral-500/60",
    running: "ring-2 ring-indigo-400 animate-pulse",
    done: "",
    failed: "ring-2 ring-red-500",
  };

  const shapes: PageShape[] = ["portrait", "square", "landscape"];
</script>

<svelte:window onkeydown={onKeydown} onkeyup={onKeyup} onblur={() => (altHeld = false)} />

<div class="flex h-full flex-col overflow-hidden text-neutral-200">
  <!-- Toolbar -->
  <div class="flex flex-wrap items-center gap-2 border-b border-neutral-800 px-3 py-2 text-xs">
    <h1 class="mr-1 text-sm font-medium text-neutral-100">{locale.t("comic.title")}</h1>

    <!-- Book switcher / rename / new / delete -->
    <div class="flex items-center gap-1">
      {#if renamingBook}
        <input
          class="w-32 rounded border border-indigo-500 bg-neutral-900 px-2 py-1 text-neutral-100 focus:outline-none"
          aria-label={locale.t("comic.rename_book")}
          bind:value={renameTitle}
          onkeydown={(e) => {
            if (e.key === "Enter") commitRenameBook();
            else if (e.key === "Escape") renamingBook = false;
          }}
          onblur={commitRenameBook}
        />
      {:else}
        <select
          class="max-w-36 rounded border border-neutral-700 bg-neutral-900 px-1.5 py-1 text-neutral-200"
          aria-label={locale.t("comic.title")}
          disabled={comic.running}
          value={comic.activeBookId}
          onchange={(e) => switchBook((e.currentTarget as HTMLSelectElement).value)}
        >
          {#each comic.books as book (book.id)}
            <option value={book.id}>{book.title}</option>
          {/each}
        </select>
        <button
          class="rounded border border-neutral-700 bg-neutral-900 px-1.5 py-1 text-neutral-300 hover:border-neutral-500"
          title={locale.t("comic.rename_book")}
          aria-label={locale.t("comic.rename_book")}
          onclick={startRenameBook}
        >✎</button>
      {/if}
      <button
        class="rounded border border-neutral-700 bg-neutral-900 px-1.5 py-1 text-neutral-300 hover:border-neutral-500 disabled:opacity-40"
        disabled={comic.running}
        title={locale.t("comic.new_book")}
        aria-label={locale.t("comic.new_book")}
        onclick={() => comic.newBook(locale.t("comic.book_default_title", { n: String(comic.books.length + 1) }))}
      >+</button>
      <button
        class="rounded border border-neutral-700 bg-neutral-900 px-1.5 py-1 text-neutral-400 hover:border-red-500 hover:text-red-300 disabled:opacity-40"
        disabled={comic.running || comic.books.length <= 1}
        title={locale.t("comic.delete_book")}
        aria-label={locale.t("comic.delete_book")}
        onclick={() => comic.deleteBook()}
      >&times;</button>
    </div>

    <span class="h-5 w-px bg-neutral-800" aria-hidden="true"></span>
    <button
      class="rounded-lg border px-2.5 py-1.5 transition-colors {showTemplates ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 bg-neutral-900 hover:border-neutral-500'}"
      onclick={() => (showTemplates = !showTemplates)}
      aria-expanded={showTemplates}
    >{locale.t("comic.templates")}</button>
    {#if comic.overflowPanels.length > 0}
      <span
        class="rounded-full border border-indigo-500/40 bg-indigo-500/10 px-1.5 py-0.5 text-[10px] font-medium text-indigo-300"
        title={locale.t("comic.overflow_tip", { count: String(comic.overflowPanels.length) })}
      >+{comic.overflowPanels.length}</span>
    {/if}
    <div class="flex overflow-hidden rounded-lg border border-neutral-700" role="radiogroup" aria-label={locale.t("comic.layout_mode")}>
      <button
        role="radio"
        aria-checked={!isFreeform}
        class="px-2.5 py-1.5 {!isFreeform ? 'bg-indigo-600 text-white' : 'bg-neutral-900 text-neutral-300 hover:bg-neutral-800'}"
        title={locale.t("comic.layout_grid_title")}
        onclick={() => comic.setLayoutMode("grid")}
      >{locale.t("comic.layout_grid")}</button>
      <button
        role="radio"
        aria-checked={isFreeform}
        class="px-2.5 py-1.5 {isFreeform ? 'bg-indigo-600 text-white' : 'bg-neutral-900 text-neutral-300 hover:bg-neutral-800'}"
        title={locale.t("comic.layout_freeform_title")}
        onclick={() => comic.setLayoutMode("freeform")}
      >{locale.t("comic.layout_freeform")}</button>
    </div>
    <span class="h-5 w-px bg-neutral-800" aria-hidden="true"></span>
    {#if isFreeform}
      <button
        class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-neutral-500 disabled:opacity-40"
        disabled={comic.panelCount >= MAX_PANELS}
        title={locale.t("comic.add_panel_title")}
        onclick={() => comic.addFreeformPanel()}
      >{locale.t("comic.add_panel")}</button>
      <button
        class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-neutral-500 disabled:opacity-40"
        disabled={selectedFreeformIdx < 0 || selectedFreeformIdx >= sortedFreeform.length - 1}
        title={locale.t("comic.layer_up_title")}
        onclick={() => comic.selectedId && comic.reorderPanelZ(comic.selectedId, "forward")}
      >{locale.t("comic.layer_up")}</button>
      <button
        class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-neutral-500 disabled:opacity-40"
        disabled={selectedFreeformIdx <= 0}
        title={locale.t("comic.layer_down_title")}
        onclick={() => comic.selectedId && comic.reorderPanelZ(comic.selectedId, "backward")}
      >{locale.t("comic.layer_down")}</button>
    {:else}
      <button
        class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-neutral-500 disabled:opacity-40"
        disabled={!comic.selectedId || comic.panelCount >= MAX_PANELS}
        title={locale.t("comic.split_side_title")}
        onclick={() => comic.split("row")}
      >{locale.t("comic.split_side")}</button>
      <button
        class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-neutral-500 disabled:opacity-40"
        disabled={!comic.selectedId || comic.panelCount >= MAX_PANELS}
        title={locale.t("comic.split_stack_title")}
        onclick={() => comic.split("col")}
      >{locale.t("comic.split_stack")}</button>
    {/if}
    <button
      class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-red-500 hover:text-red-300 disabled:opacity-40"
      disabled={!comic.selectedId || comic.panelCount <= 1}
      onclick={() => comic.removeSelected()}
    >{locale.t("comic.remove_panel")}</button>
    <button
      class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-indigo-500 hover:text-indigo-200"
      title={locale.t("comic.add_bubble_title")}
      onclick={() => comic.addBubble(comic.selectedId ?? undefined)}
    >{locale.t("comic.add_bubble")}</button>
    <button
      class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-neutral-500 disabled:opacity-40"
      disabled={!comic.canUndo}
      title="Ctrl+Z"
      onclick={() => comic.undo()}
    >{locale.t("comic.undo")}</button>
    <button
      class="rounded-lg border px-2.5 py-1.5 transition-colors {panMode ? 'border-indigo-500 bg-indigo-500/20 text-indigo-200' : 'border-neutral-700 bg-neutral-900 hover:border-neutral-500'}"
      aria-pressed={panMode}
      title={locale.t("comic.pan_mode_title")}
      onclick={() => (panMode = !panMode)}
    >{locale.t("comic.pan_mode")}</button>
    <span class="h-5 w-px bg-neutral-800" aria-hidden="true"></span>
    <label class="flex items-center gap-1.5 text-neutral-400">
      {locale.t("comic.page_shape")}
      <select
        class="rounded border border-neutral-700 bg-neutral-900 px-1.5 py-1 text-neutral-200"
        value={comic.pageShape}
        onchange={(e) => comic.setPageField("pageShape", (e.currentTarget as HTMLSelectElement).value as PageShape)}
      >
        {#each shapes as shape (shape)}
          <option value={shape}>{locale.t(`comic.shape_${shape}`)}</option>
        {/each}
      </select>
    </label>
    <label class="flex items-center gap-1.5 text-neutral-400" title={locale.t("comic.gutter_title")}>
      {locale.t("comic.gutter")}
      <input
        type="range"
        min="0"
        max="64"
        step="2"
        class="w-20 accent-indigo-500"
        value={comic.gutter}
        oninput={(e) => comic.setPageField("gutter", Number((e.currentTarget as HTMLInputElement).value))}
      />
    </label>
    <button
      class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-neutral-500"
      title={locale.t("comic.reading_order_title")}
      onclick={() => comic.setPageField("rightToLeft", !comic.rightToLeft)}
    >{comic.rightToLeft ? locale.t("comic.order_rtl") : locale.t("comic.order_ltr")}</button>

    <div class="ml-auto flex flex-wrap items-center gap-2">
      <div class="flex overflow-hidden rounded-lg border border-neutral-700" role="radiogroup" aria-label={locale.t("comic.mode")}>
        <button
          role="radio"
          aria-checked={!oneImage}
          class="px-2.5 py-1.5 {!oneImage ? 'bg-indigo-600 text-white' : 'bg-neutral-900 text-neutral-300 hover:bg-neutral-800'}"
          title={locale.t("comic.mode_panels_title")}
          onclick={() => comic.setPageField("mode", "panels")}
        >{locale.t("comic.mode_panels")}</button>
        <button
          role="radio"
          aria-checked={oneImage}
          class="px-2.5 py-1.5 {oneImage ? 'bg-indigo-600 text-white' : 'bg-neutral-900 text-neutral-300 hover:bg-neutral-800'}"
          title={locale.t("comic.mode_one_image_title")}
          onclick={() => comic.setPageField("mode", "one_image")}
        >{locale.t("comic.mode_one_image")}</button>
      </div>
      {#if comic.running}
        <button class="rounded-lg bg-red-700 px-3 py-1.5 font-medium text-white hover:bg-red-600" onclick={stopRun}>
          {comic.stopRequested ? locale.t("comic.stopping") : locale.t("comic.stop")}
        </button>
      {:else}
        <button
          class="rounded-lg bg-indigo-600 px-3 py-1.5 font-medium text-white hover:bg-indigo-500 disabled:opacity-40"
          disabled={comic.enabledPanels.length === 0 || tooManyForOneImage || (oneImage && !generation.isNovelAi)}
          onclick={generateAll}
        >{oneImage ? locale.t("comic.generate_page") : locale.t("comic.generate_all")}</button>
      {/if}
      <button
        class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-neutral-500 disabled:opacity-40"
        disabled={oneImage ? !comic.pageUrl : !Object.values(comic.results).some((r) => r.url)}
        onclick={() => { void openPage(); }}
      >{locale.t("comic.view_page")}</button>
      <button
        class="rounded-lg border border-emerald-700 bg-emerald-900/40 px-2.5 py-1.5 text-emerald-200 hover:bg-emerald-800/50 disabled:opacity-40"
        disabled={saving || exporting || (oneImage ? !comic.pageUrl : !Object.values(comic.results).some((r) => r.url))}
        onclick={() => { void save(); }}
      >{locale.t("comic.save_page")}</button>
      {#if comic.pages.length > 1}
        <button
          class="rounded-lg border border-emerald-700/70 bg-emerald-950/40 px-2.5 py-1.5 text-emerald-200 hover:bg-emerald-900/50 disabled:opacity-40"
          disabled={saving || exporting || !anyPageHasContent}
          onclick={() => { void saveAll(); }}
        >{locale.t("comic.save_all")}</button>
      {/if}
      <button
        class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-neutral-500 disabled:opacity-40"
        disabled={saving || exporting || !anyPageHasContent}
        onclick={() => { void exportCbz(); }}
      >{locale.t("comic.export_cbz")}</button>
      <button
        class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-neutral-500 disabled:opacity-40"
        disabled={saving || exporting || !anyPageHasContent}
        onclick={() => { void exportPdf(); }}
      >{locale.t("comic.export_pdf")}</button>
    </div>
  </div>

  {#if (oneImage && !generation.isNovelAi) || tooManyForOneImage || error}
    <div class="flex flex-wrap items-center gap-2 border-b border-neutral-800/60 px-3 py-1.5 text-[11px] text-neutral-500">
      {#if oneImage && !generation.isNovelAi}
        <span class="text-amber-400">{locale.t("comic.error_one_image_novelai")}</span>
      {:else if tooManyForOneImage}
        <span class="text-amber-400">{locale.t("comic.error_one_image_panels", { max: String(ONE_SHOT_MAX_PANELS) })}</span>
      {/if}
      {#if error}
        <span class="text-red-400">{error}</span>
      {/if}
    </div>
  {/if}

  {#if showTemplates}
    <div class="flex flex-wrap gap-2 border-b border-neutral-800 bg-neutral-900/50 px-3 py-2">
      {#each LAYOUT_TEMPLATES as t (t.id)}
        {@const preview = panelRects(buildTemplate(t, (() => { let n = 0; return () => `t${n++}`; })()))}
        <button
          class="group flex flex-col items-center gap-1 rounded-lg p-1.5 hover:bg-neutral-800"
          title={locale.t("comic.template_panels", { count: String(t.panels) })}
          onclick={() => pickTemplate(t)}
        >
          <svg viewBox="0 0 100 {Math.round(100 / aspect)}" class="h-16 border border-neutral-700 bg-neutral-100 group-hover:border-indigo-400" style="aspect-ratio: {aspect}">
            {#each [...preview.values()] as r, i (i)}
              <rect
                x={r.x * 100 + 3}
                y={(r.y * 100) / aspect + 3}
                width={Math.max(1, r.w * 100 - 6)}
                height={Math.max(1, (r.h * 100) / aspect - 6)}
                fill="#e5e7eb"
                stroke="#111827"
                stroke-width="2"
              />
            {/each}
          </svg>
          <span class="text-[10px] text-neutral-400">{t.panels}</span>
        </button>
      {/each}
    </div>
  {/if}

  <div class="flex min-h-0 flex-1 flex-col overflow-y-auto lg:flex-row lg:overflow-hidden">
    <!-- Page setup: style, setting, cast, story -->
    <aside class="space-y-4 border-neutral-800 p-3 text-xs lg:w-80 lg:shrink-0 lg:overflow-y-auto lg:border-r">
      <!-- The Comic page's own model and settings, apart from the Generate page's. -->
      <section class="rounded-lg border border-neutral-800 bg-neutral-900/40">
        <div class="flex w-full items-center gap-1.5 px-2.5 py-2 hover:bg-neutral-800/40">
          <button
            class="shrink-0 text-left font-medium text-neutral-300 focus:outline-none"
            aria-expanded={showSettings}
            onclick={toggleSettings}
          >{locale.t("comic.settings")}</button>
          <InfoTip text={locale.t("comic.settings_hint")} />
          <button
            class="flex min-w-0 flex-1 items-center gap-2 text-right focus:outline-none"
            aria-expanded={showSettings}
            aria-label={locale.t("comic.settings")}
            onclick={toggleSettings}
          >
            <span class="min-w-0 flex-1 truncate text-[11px] {modelName ? 'text-neutral-500' : 'text-amber-400'}">
              {modelName ? `${modelName} · ~${Math.round(Math.sqrt(generation.width * generation.height))}px` : locale.t("comic.settings_no_model")}
            </span>
            <svg class="h-3.5 w-3.5 shrink-0 text-neutral-500 transition-transform {showSettings ? 'rotate-180' : ''}" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
              <path fill-rule="evenodd" d="M5.23 7.21a.75.75 0 011.06.02L10 11.17l3.71-3.94a.75.75 0 111.08 1.04l-4.25 4.5a.75.75 0 01-1.08 0l-4.25-4.5a.75.75 0 01.02-1.06z" clip-rule="evenodd" />
            </svg>
          </button>
        </div>
        {#if showSettings}
          <div class="space-y-3 border-t border-neutral-800 px-2.5 pb-3 pt-2.5">
            <ModelSelector />
            <div class="border-t border-neutral-800 pt-3">
              <SamplerSettings showBatchSize={false} showSeed={false} />
            </div>
            <div class="border-t border-neutral-800 pt-3">
              <DimensionControls pixelBudgetOnly={true} />
            </div>
          </div>
        {/if}
      </section>

      <section class="space-y-1.5">
        <label class="block text-neutral-400" for="comic-style">{locale.t("comic.style")}</label>
        <ComicPromptField
          id="comic-style"
          value={comic.style}
          onchange={(next) => comic.setPageField("style", next)}
          placeholder={locale.t("comic.style_placeholder")}
          ariaLabel={locale.t("comic.style")}
          rows={2}
          minHeight="min-h-12"
          storageKey="mooshieui.promptHeight.comic.style"
        />
        <label class="block pt-1 text-neutral-400" for="comic-setting">{locale.t("comic.setting")}</label>
        <ComicPromptField
          id="comic-setting"
          value={comic.setting}
          onchange={(next) => comic.setPageField("setting", next)}
          placeholder={locale.t("comic.setting_placeholder")}
          ariaLabel={locale.t("comic.setting")}
          rows={2}
          minHeight="min-h-12"
          storageKey="mooshieui.promptHeight.comic.setting"
        />
        {#if comic.pages.length > 1}
          <label class="block pt-1 text-[11px] text-neutral-400" for="comic-page-setting-override">{locale.t("comic.page_setting_override")}</label>
          <ComicPromptField
            id="comic-page-setting-override"
            value={comic.settingOverride}
            onchange={(next) => comic.setPageField("settingOverride", next)}
            placeholder={comic.setting.trim() ? locale.t("comic.page_setting_override_placeholder", { setting: comic.setting.trim() }) : locale.t("comic.setting_placeholder")}
            ariaLabel={locale.t("comic.page_setting_override")}
            rows={1}
            minHeight="min-h-10"
            storageKey="mooshieui.promptHeight.comic.pageSettingOverride"
          />
        {/if}
      </section>

      <section class="space-y-2">
        <div class="flex items-center justify-between gap-2">
          <span class="text-neutral-400">{locale.t("comic.cast")}</span>
          <div class="flex items-center gap-2">
            <button class="text-emerald-300 hover:underline" onclick={() => openCharacterPicker(null)}>{locale.t("comic.from_characters")}</button>
            <button class="text-indigo-300 hover:underline" onclick={() => comic.addCastMember()}>{locale.t("comic.add_cast")}</button>
          </div>
        </div>
        {#if comic.cast.length === 0}
          <p class="text-[11px] text-neutral-500">{locale.t("comic.cast_empty")}</p>
        {/if}
        {#each comic.cast as member (member.id)}
          <div class="space-y-1.5 rounded-lg border border-neutral-800 bg-neutral-900/60 p-2">
            <div class="flex items-center gap-1.5">
              {#if member.referenceImage}
                <div class="relative shrink-0">
                  <img
                    src={member.referenceImage}
                    alt={member.name || locale.t("comic.cast_name")}
                    class="h-8 w-8 rounded border border-neutral-700 object-cover"
                  />
                  <button
                    class="absolute -right-1 -top-1 flex h-3.5 w-3.5 items-center justify-center rounded-full bg-neutral-900 text-[9px] text-neutral-300 ring-1 ring-neutral-700 hover:text-red-400"
                    title={locale.t("comic.clear_ref")}
                    aria-label={locale.t("comic.clear_ref")}
                    onclick={() => {
                      comic.updateCastMember(member.id, { referenceImage: null, savedCharacterId: null });
                      comic.persist();
                    }}
                  >&times;</button>
                </div>
              {:else}
                <button
                  class="flex h-8 w-8 shrink-0 items-center justify-center rounded border border-dashed border-neutral-700 text-[10px] text-neutral-500 hover:border-indigo-500 hover:text-indigo-300"
                  title={locale.t("comic.pick_ref")}
                  aria-label={locale.t("comic.pick_ref")}
                  onclick={() => openCharacterPicker(member.id)}
                >+</button>
              {/if}
              <input
                class="min-w-0 flex-1 rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-neutral-100 placeholder-neutral-600 focus:border-indigo-500 focus:outline-none"
                placeholder={locale.t("comic.cast_name")}
                aria-label={locale.t("comic.cast_name")}
                value={member.name}
                oninput={(e) => comic.updateCastMember(member.id, { name: (e.currentTarget as HTMLInputElement).value })}
              />
              <button
                class="rounded px-1.5 text-neutral-500 hover:text-red-400"
                aria-label={locale.t("comic.remove_cast")}
                title={locale.t("comic.remove_cast")}
                onclick={() => comic.removeCastMember(member.id)}
              >&times;</button>
            </div>
            <ComicPromptField
              value={member.tags}
              onchange={(next) => comic.updateCastMember(member.id, { tags: next })}
              placeholder={locale.t("comic.cast_tags_placeholder")}
              ariaLabel={locale.t("comic.cast_tags")}
              rows={2}
              minHeight="min-h-14"
              suggestLoraKeywords={true}
              storageKey="mooshieui.promptHeight.comic.castTags"
            />
          </div>
        {/each}
      </section>

      <section class="space-y-2">
        <span class="block text-neutral-400">{locale.t("comic.story")}</span>
        <label class="flex items-start gap-2 text-[11px] text-neutral-300">
          <input
            type="checkbox"
            class="mt-0.5 accent-indigo-500"
            checked={comic.checkContinuity}
            onchange={(e) => comic.setPageField("checkContinuity", (e.currentTarget as HTMLInputElement).checked)}
          />
          <span>{locale.t("comic.check_continuity")}</span>
        </label>
        {#if !oneImage}
          <label class="flex items-center gap-1.5 text-[11px] {canCharacterRef ? 'text-neutral-300' : 'text-neutral-500'}">
            <span class="shrink-0">{locale.t("comic.ref_mode")}</span>
            <select
              class="min-w-0 flex-1 rounded border border-neutral-700 bg-neutral-900 px-1.5 py-1 text-neutral-200 disabled:opacity-50"
              value={comic.refMode}
              disabled={!canCharacterRef}
              onchange={(e) => comic.setPageField("refMode", (e.currentTarget as HTMLSelectElement).value as RefMode)}
            >
              <option value="off">{locale.t("comic.ref_off")}</option>
              <option value="page1_panel1">{locale.t("comic.ref_page1_panel1")}</option>
              <option value="prev_panel">{locale.t("comic.ref_prev_panel")}</option>
              <option value="cast">{locale.t("comic.ref_cast")}</option>
            </select>
            <InfoTip text={locale.t(canCharacterRef ? "comic.ref_mode_tip" : "comic.panel1_ref_unsupported")} />
          </label>
        {/if}
        <div class="flex flex-wrap gap-1.5">
          <button
            class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-indigo-500 disabled:opacity-40"
            disabled={!promptAssistant.isAvailable || comic.drafting}
            title={promptAssistant.isAvailable ? locale.t(v5 ? "comic.draft_title_v5" : "comic.draft_title") : locale.t("comic.assistant_needed")}
            onclick={() => { void run(draftFromBeats); }}
          >{comic.drafting ? locale.t("comic.working") : locale.t("comic.draft")}</button>
          {#if comic.draftUndo}
            <button class="rounded-lg px-2 py-1.5 text-neutral-400 hover:text-neutral-200" onclick={undoDraft}>{locale.t("comic.undo_draft")}</button>
          {/if}
          <button
            class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-indigo-500 disabled:opacity-40"
            disabled={!promptAssistant.isAvailable || comic.storyChecking}
            title={promptAssistant.isAvailable ? locale.t("comic.check_story_title") : locale.t("comic.assistant_needed")}
            onclick={() => { void run(checkStory); }}
          >{comic.storyChecking ? locale.t("comic.working") : locale.t("comic.check_story")}</button>
        </div>
        {#if comic.storyError}
          <p class="text-[11px] text-red-400">{comic.storyError}</p>
        {/if}
        {#if comic.storyIssues}
          {#if comic.storyIssues.length === 0}
            <p class="text-[11px] text-emerald-400">{locale.t("comic.story_ok")}</p>
          {:else}
            <ul class="space-y-1">
              {#each comic.storyIssues as item, i (i)}
                <li class="rounded border border-amber-500/30 bg-amber-500/10 px-2 py-1 text-[11px] text-amber-200">
                  {#if item.panel > 0}<span class="font-medium">{locale.t("comic.panel_n", { n: String(item.panel) })}:</span>{/if}
                  {item.issue}
                </li>
              {/each}
            </ul>
          {/if}
        {/if}
        {#if issueCount > 0}
          <p class="text-[11px] text-amber-300">{locale.t("comic.issues_total", { count: String(issueCount) })}</p>
        {/if}
      </section>
    </aside>

    <!-- The page + page strip -->
    <div class="flex min-h-[420px] min-w-0 flex-1 flex-col bg-neutral-950 lg:min-h-0">
      <div class="flex min-h-0 flex-1 items-center justify-center p-4">
        <div
          bind:this={canvasEl}
          class="relative max-h-full w-full max-w-[640px] select-none bg-white shadow-xl shadow-black/40"
          style="aspect-ratio: {aspect}"
          onpointermove={onDrag}
          onpointerup={endDrag}
          onpointercancel={endDrag}
          role="application"
          aria-label={locale.t("comic.page_label")}
        >
        {#if isFreeform}
          {#each sortedFreeform as box, zOrder (box.id)}
            {@const panel = comic.panels[box.id]}
            {@const result = comic.results[box.id]}
            {@const number = order.indexOf(box.id) + 1}
            {@const boxPx = freeformPixelBox(box)}
            {@const bStyle = normalizePanelBorderStyle(panel?.borderStyle)}
            {@const isSel = comic.selectedId === box.id}
            {@const canPan = (panMode || altHeld) && !!result?.url}
            {@const imgCss = panelImageCss(imgDims[box.id] ?? { width: boxPx.w, height: boxPx.h }, boxPx, panel?.panX ?? 0, panel?.panY ?? 0, panel?.zoom ?? 1)}
            <div
              class="absolute {isSel && bStyle !== 'slanted' ? 'outline outline-3 outline-offset-[3px] outline-indigo-500' : ''} {result ? statusRing[result.status] : ''} {panel?.enabled === false ? 'opacity-40' : ''}"
              style="{freeformBoxStyle(box, zOrder)}{bStyle === 'rounded' ? ` border-radius: ${panelBorderRadiusCss(boxPx, 'rounded')};` : ''}"
            >
              <!-- White overlap halo rendered behind the clipped panel so CSS clip-path never clips it -->
              <svg
                class="pointer-events-none absolute inset-0 h-full w-full overflow-visible"
                viewBox="0 0 {boxPx.w} {boxPx.h}"
                preserveAspectRatio="none"
                aria-hidden="true"
              >
                {#if bStyle === "slanted"}
                  <polygon
                    points={svgPolygonPoints(boxPx, "slanted")}
                    fill="#ffffff"
                    stroke="#ffffff"
                    stroke-width={freeformHaloPx * 2}
                    stroke-linejoin="miter"
                    stroke-miterlimit="4"
                  />
                  {#if isSel}
                    <polygon
                      points={svgPolygonPoints(boxPx, "slanted")}
                      fill="none"
                      stroke="#6366f1"
                      stroke-width={borderPx * 2 + 4}
                      stroke-linejoin="miter"
                      stroke-miterlimit="4"
                    />
                  {/if}
                {:else if bStyle === "rounded"}
                  {@const cr = panelCornerRadius(boxPx)}
                  <rect
                    x="0"
                    y="0"
                    width={boxPx.w}
                    height={boxPx.h}
                    rx={cr}
                    ry={cr}
                    fill="#ffffff"
                    stroke="#ffffff"
                    stroke-width={freeformHaloPx * 2}
                  />
                {:else}
                  <rect
                    x="0"
                    y="0"
                    width={boxPx.w}
                    height={boxPx.h}
                    fill="#ffffff"
                    stroke="#ffffff"
                    stroke-width={freeformHaloPx * 2}
                  />
                {/if}
              </svg>
              <button
                class="relative h-full w-full touch-none overflow-hidden bg-neutral-200 text-left {canPan ? (panDrag?.id === box.id ? 'cursor-grabbing' : 'cursor-grab') : 'cursor-move'}"
                style="clip-path: {panelClipPathCss(boxPx, bStyle)}; border-radius: {panelBorderRadiusCss(boxPx, bStyle)};"
                onpointerdown={(e) => onPanelPointerDown(e, box.id, boxPx, box)}
                onwheel={(e) => onPanelWheel(e, box.id)}
                onclick={() => {
                  comic.selectedBubbleId = null;
                  comic.selectedId = box.id;
                }}
                aria-pressed={isSel}
                aria-label={locale.t("comic.panel_n", { n: String(number) })}
              >
                {#if result?.url}
                  <img
                    src={result.url}
                    alt=""
                    class="pointer-events-none absolute max-w-none max-h-none"
                    style="left: {imgCss.left}; top: {imgCss.top}; width: {imgCss.width}; height: {imgCss.height};"
                    onload={(e) => recordImgDims(box.id, e.currentTarget as HTMLImageElement)}
                    draggable="false"
                  />
                {:else}
                  <span class="block p-1.5 text-[10px] leading-snug text-neutral-600 line-clamp-4 {bStyle === 'slanted' ? 'pl-[18%]' : ''}">{panel?.beat || panel?.prompt || ""}</span>
                {/if}
                {#if bStyle !== "borderless"}
                  <svg
                    class="pointer-events-none absolute inset-0 h-full w-full"
                    viewBox="0 0 {boxPx.w} {boxPx.h}"
                    preserveAspectRatio="none"
                    aria-hidden="true"
                  >
                    <defs>
                      <clipPath id="comic-clip-ff-{box.id}">
                        {#if bStyle === "slanted"}
                          <polygon points={svgPolygonPoints(boxPx, "slanted")} />
                        {:else if bStyle === "rounded"}
                          {@const cr = panelCornerRadius(boxPx)}
                          <rect x="0" y="0" width={boxPx.w} height={boxPx.h} rx={cr} ry={cr} />
                        {:else}
                          <rect x="0" y="0" width={boxPx.w} height={boxPx.h} />
                        {/if}
                      </clipPath>
                    </defs>
                    {#if bStyle === "slanted"}
                      <polygon
                        points={svgPolygonPoints(boxPx, "slanted")}
                        fill="none"
                        stroke="#000000"
                        stroke-width={borderPx * 2}
                        stroke-linejoin="miter"
                        stroke-miterlimit="4"
                        clip-path="url(#comic-clip-ff-{box.id})"
                      />
                    {:else if bStyle === "rounded"}
                      {@const cr = panelCornerRadius(boxPx)}
                      <rect
                        x="0"
                        y="0"
                        width={boxPx.w}
                        height={boxPx.h}
                        rx={cr}
                        ry={cr}
                        fill="none"
                        stroke="#000000"
                        stroke-width={borderPx * 2}
                        clip-path="url(#comic-clip-ff-{box.id})"
                      />
                    {:else}
                      <rect
                        x="0"
                        y="0"
                        width={boxPx.w}
                        height={boxPx.h}
                        fill="none"
                        stroke="#000000"
                        stroke-width={borderPx * 2}
                        clip-path="url(#comic-clip-ff-{box.id})"
                      />
                    {/if}
                  </svg>
                {/if}
                <span
                  class="absolute top-1 rounded-full bg-neutral-900/85 px-1.5 text-[10px] font-semibold text-white"
                  style={bStyle === "slanted" ? `left: calc(${((panelSlantInset(boxPx) / boxPx.w) * 100).toFixed(4)}% + 4px);` : "left: 4px;"}
                >{number}</span>
                {#if result?.status === "running"}
                  <span class="absolute inset-x-0 bottom-0 bg-indigo-600/85 py-0.5 text-center text-[10px] text-white">{locale.t("comic.generating")}</span>
                {:else if result?.status === "failed"}
                  <span class="absolute inset-x-0 bottom-0 bg-red-700/90 py-0.5 text-center text-[10px] text-white">{locale.t("comic.failed")}</span>
                {:else if result?.issues && result.issues.length > 0}
                  <span class="absolute right-1 top-1 rounded-full bg-amber-500 px-1.5 text-[10px] font-semibold text-neutral-950" title={locale.t("comic.issues_badge_title")}>!{result.issues.length}</span>
                {:else if result?.issues && result.issues.length === 0}
                  <span class="absolute right-1 top-1 rounded-full bg-emerald-600 px-1.5 text-[10px] text-white" title={locale.t("comic.check_passed")}>✓</span>
                {/if}
              </button>
            </div>
          {/each}
          {#if selectedFreeformBox}
            {@const sb = selectedFreeformBox}
            <div
              class="pointer-events-none absolute z-20"
              style="left: {sb.x * 100}%; top: {sb.y * 100}%; width: {sb.w * 100}%; height: {sb.h * 100}%;"
            >
              {#each RESIZE_HANDLES as h (h.id)}
                <button
                  type="button"
                  class="pointer-events-auto absolute h-3 w-3 touch-none rounded-sm border border-indigo-600 bg-white shadow {h.cls}"
                  aria-label={locale.t("comic.resize_handle")}
                  onpointerdown={(e) => startFreeformResize(e, sb, h.id)}
                ></button>
              {/each}
            </div>
          {/if}
          {#each snapGuides as g, i (i)}
            <div
              class="pointer-events-none absolute z-30 bg-indigo-400/90"
              style={g.axis === "x"
                ? `left: ${g.pos * 100}%; top: 0; bottom: 0; width: 1px;`
                : `top: ${g.pos * 100}%; left: 0; right: 0; height: 1px;`}
            ></div>
          {/each}
        {:else}
          {#each [...rects.entries()] as [id, r] (id)}
            {@const panel = comic.panels[id]}
            {@const result = comic.results[id]}
            {@const number = order.indexOf(id) + 1}
            {@const boxPx = panelPixelRect(r, pagePx, comic.gutter)}
            {@const bStyle = normalizePanelBorderStyle(panel?.borderStyle)}
            {@const isSel = comic.selectedId === id}
            {@const canPan = (panMode || altHeld) && !!result?.url}
            {@const imgCss = panelImageCss(imgDims[id] ?? { width: boxPx.w, height: boxPx.h }, boxPx, panel?.panX ?? 0, panel?.panY ?? 0, panel?.zoom ?? 1)}
            <div
              class="absolute {isSel && bStyle !== 'slanted' ? 'outline outline-3 outline-offset-[3px] outline-indigo-500' : ''} {result ? statusRing[result.status] : ''} {panel?.enabled === false ? 'opacity-40' : ''}"
              style="{boxStyle(r)}{bStyle === 'rounded' ? ` border-radius: ${panelBorderRadiusCss(boxPx, 'rounded')};` : ''}"
            >
              {#if isSel && bStyle === "slanted"}
                <svg
                  class="pointer-events-none absolute inset-0 h-full w-full overflow-visible"
                  viewBox="0 0 {boxPx.w} {boxPx.h}"
                  preserveAspectRatio="none"
                  aria-hidden="true"
                >
                  <polygon
                    points={svgPolygonPoints(boxPx, "slanted")}
                    fill="none"
                    stroke="#6366f1"
                    stroke-width={borderPx * 2 + 4}
                    stroke-linejoin="miter"
                    stroke-miterlimit="4"
                  />
                </svg>
              {/if}
              <button
                class="relative h-full w-full touch-none overflow-hidden bg-neutral-200 text-left {canPan ? (panDrag?.id === id ? 'cursor-grabbing' : 'cursor-grab') : ''}"
                style="clip-path: {panelClipPathCss(boxPx, bStyle)}; border-radius: {panelBorderRadiusCss(boxPx, bStyle)};"
                onpointerdown={(e) => onPanelPointerDown(e, id, boxPx)}
                onwheel={(e) => onPanelWheel(e, id)}
                onclick={() => {
                  comic.selectedBubbleId = null;
                  comic.selectedId = id;
                }}
                aria-pressed={isSel}
                aria-label={locale.t("comic.panel_n", { n: String(number) })}
              >
                {#if result?.url}
                  <img
                    src={result.url}
                    alt=""
                    class="pointer-events-none absolute max-w-none max-h-none"
                    style="left: {imgCss.left}; top: {imgCss.top}; width: {imgCss.width}; height: {imgCss.height};"
                    onload={(e) => recordImgDims(id, e.currentTarget as HTMLImageElement)}
                    draggable="false"
                  />
                {:else}
                  <span class="block p-1.5 text-[10px] leading-snug text-neutral-600 line-clamp-4 {bStyle === 'slanted' ? 'pl-[18%]' : ''}">{panel?.beat || panel?.prompt || ""}</span>
                {/if}
                {#if bStyle !== "borderless"}
                  <svg
                    class="pointer-events-none absolute inset-0 h-full w-full"
                    viewBox="0 0 {boxPx.w} {boxPx.h}"
                    preserveAspectRatio="none"
                    aria-hidden="true"
                  >
                    <defs>
                      <clipPath id="comic-clip-grid-{id}">
                        {#if bStyle === "slanted"}
                          <polygon points={svgPolygonPoints(boxPx, "slanted")} />
                        {:else if bStyle === "rounded"}
                          {@const cr = panelCornerRadius(boxPx)}
                          <rect x="0" y="0" width={boxPx.w} height={boxPx.h} rx={cr} ry={cr} />
                        {:else}
                          <rect x="0" y="0" width={boxPx.w} height={boxPx.h} />
                        {/if}
                      </clipPath>
                    </defs>
                    {#if bStyle === "slanted"}
                      <polygon
                        points={svgPolygonPoints(boxPx, "slanted")}
                        fill="none"
                        stroke="#000000"
                        stroke-width={borderPx * 2}
                        stroke-linejoin="miter"
                        stroke-miterlimit="4"
                        clip-path="url(#comic-clip-grid-{id})"
                      />
                    {:else if bStyle === "rounded"}
                      {@const cr = panelCornerRadius(boxPx)}
                      <rect
                        x="0"
                        y="0"
                        width={boxPx.w}
                        height={boxPx.h}
                        rx={cr}
                        ry={cr}
                        fill="none"
                        stroke="#000000"
                        stroke-width={borderPx * 2}
                        clip-path="url(#comic-clip-grid-{id})"
                      />
                    {:else}
                      <rect
                        x="0"
                        y="0"
                        width={boxPx.w}
                        height={boxPx.h}
                        fill="none"
                        stroke="#000000"
                        stroke-width={borderPx * 2}
                        clip-path="url(#comic-clip-grid-{id})"
                      />
                    {/if}
                  </svg>
                {/if}
                <span
                  class="absolute top-1 rounded-full bg-neutral-900/85 px-1.5 text-[10px] font-semibold text-white"
                  style={bStyle === "slanted" ? `left: calc(${((panelSlantInset(boxPx) / boxPx.w) * 100).toFixed(4)}% + 4px);` : "left: 4px;"}
                >{number}</span>
                {#if result?.status === "running"}
                  <span class="absolute inset-x-0 bottom-0 bg-indigo-600/85 py-0.5 text-center text-[10px] text-white">{locale.t("comic.generating")}</span>
                {:else if result?.status === "failed"}
                  <span class="absolute inset-x-0 bottom-0 bg-red-700/90 py-0.5 text-center text-[10px] text-white">{locale.t("comic.failed")}</span>
                {:else if result?.issues && result.issues.length > 0}
                  <span class="absolute right-1 top-1 rounded-full bg-amber-500 px-1.5 text-[10px] font-semibold text-neutral-950" title={locale.t("comic.issues_badge_title")}>!{result.issues.length}</span>
                {:else if result?.issues && result.issues.length === 0}
                  <span class="absolute right-1 top-1 rounded-full bg-emerald-600 px-1.5 text-[10px] text-white" title={locale.t("comic.check_passed")}>✓</span>
                {/if}
              </button>
            </div>
          {/each}
          {#each lines as line (line.splitId)}
            <!-- A divider: drag to resize the two sides. -->
            <div
              class="absolute z-10 {line.dir === 'row' ? 'cursor-col-resize' : 'cursor-row-resize'} group"
              style={line.dir === "row"
                ? `left: calc(${line.pos * 100}% - 6px); top: ${line.box.y * 100}%; width: 12px; height: ${line.box.h * 100}%;`
                : `top: calc(${line.pos * 100}% - 6px); left: ${line.box.x * 100}%; height: 12px; width: ${line.box.w * 100}%;`}
              onpointerdown={(e) => startDrag(e, line)}
              role="separator"
              aria-orientation={line.dir === "row" ? "vertical" : "horizontal"}
              aria-label={locale.t("comic.divider")}
              tabindex="-1"
            >
              <span class="absolute rounded bg-indigo-500 opacity-0 transition-opacity group-hover:opacity-80 {dragging?.splitId === line.splitId ? 'opacity-100' : ''} {line.dir === 'row' ? 'left-[4px] top-0 h-full w-[4px]' : 'top-[4px] left-0 w-full h-[4px]'}"></span>
            </div>
          {/each}
        {/if}

        {#if comic.activePage.bubbles.length > 0}
          {@const bubbleStroke = Math.max(2, Math.round(Math.max(pagePx.width, pagePx.height) / 550))}
          {@const handleSize = Math.max(12, pagePx.width * 0.015)}
          {@const tailHandleRadius = Math.max(9, pagePx.width * 0.011)}
          <svg
            class="pointer-events-none absolute inset-0 z-30 h-full w-full overflow-visible"
            viewBox="0 0 {pagePx.width} {pagePx.height}"
            aria-label={locale.t("comic.bubble_inspector")}
          >
            {#each comic.activePage.bubbles as b (b.id)}
              {@const geom = bubblePathData(b, pagePx.width, pagePx.height)}
              {@const selected = comic.selectedBubbleId === b.id}
              {@const pxFont = bubbleFontPx(b.fontSize, pagePx.width, pagePx.height)}
              {@const fontFam = bubbleFontFamily(b.font)}
              <g
                class="pointer-events-auto cursor-move outline-none"
                role="button"
                tabindex="0"
                aria-pressed={selected}
                aria-label={b.text.trim() || locale.t("comic.bubble_empty")}
                onpointerdown={(e) => startBubbleMove(e, b)}
                onclick={(e) => {
                  e.stopPropagation();
                  selectBubble(b.id);
                }}
                onkeydown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    e.stopPropagation();
                    selectBubble(b.id);
                  }
                }}
              >
                <path
                  d={geom.path}
                  fill="#ffffff"
                  stroke={selected ? "#4f46e5" : "#111827"}
                  stroke-width={bubbleStroke}
                  stroke-linejoin="round"
                  stroke-linecap="round"
                />
                {#each geom.dots as dot, di (di)}
                  <circle
                    cx={dot.cx}
                    cy={dot.cy}
                    r={dot.r}
                    fill="#ffffff"
                    stroke={selected ? "#4f46e5" : "#111827"}
                    stroke-width={bubbleStroke}
                  />
                {/each}
                {#if b.text.trim()}
                  {#if b.vertical}
                    {@const vLayout = layoutVerticalText(b.text, geom.textBox.w, geom.textBox.h, pxFont)}
                    {#each vLayout.glyphs as glyph, gi (gi)}
                      {@const gx = geom.textBox.x + glyph.x}
                      {@const gy = geom.textBox.y + glyph.y}
                      <text
                        x={gx}
                        y={gy}
                        transform={glyph.rotate ? `rotate(${(glyph.rotate * 180) / Math.PI}, ${gx}, ${gy})` : undefined}
                        text-anchor="middle"
                        dominant-baseline="central"
                        font-size={pxFont}
                        font-family={fontFam}
                        fill="#111827"
                        class="select-none"
                        pointer-events="none"
                      >{glyph.char}</text>
                    {/each}
                  {:else}
                    {@const lines = wrapBubbleText(b.text, geom.textBox.w, (s) => estimateTextWidth(s, pxFont, b.font))}
                    {@const lineHeight = pxFont * 1.22}
                    {@const startY = geom.textBox.y + (geom.textBox.h - lines.length * lineHeight) / 2 + lineHeight / 2}
                    {#each lines as line, li (li)}
                      {#if line}
                        <text
                          x={geom.textBox.x + geom.textBox.w / 2}
                          y={startY + li * lineHeight}
                          text-anchor="middle"
                          dominant-baseline="central"
                          font-size={pxFont}
                          font-family={fontFam}
                          fill="#111827"
                          class="select-none"
                          pointer-events="none"
                        >{line}</text>
                      {/if}
                    {/each}
                  {/if}
                {/if}
              </g>
              {#if selected}
                {@const bx = b.x * pagePx.width}
                {@const by = b.y * pagePx.height}
                {@const bw = b.w * pagePx.width}
                {@const bh = b.h * pagePx.height}
                {@const tx = b.tailX * pagePx.width}
                {@const ty = b.tailY * pagePx.height}
                {@const corners: Array<{ id: BubbleCorner; x: number; y: number; cls: string }> = [
                  { id: "nw", x: bx, y: by, cls: "cursor-nwse-resize" },
                  { id: "ne", x: bx + bw, y: by, cls: "cursor-nesw-resize" },
                  { id: "sw", x: bx, y: by + bh, cls: "cursor-nesw-resize" },
                  { id: "se", x: bx + bw, y: by + bh, cls: "cursor-nwse-resize" },
                ]}
                <rect
                  x={bx}
                  y={by}
                  width={bw}
                  height={bh}
                  fill="none"
                  stroke="#4f46e5"
                  stroke-width={Math.max(1.5, pagePx.width * 0.002)}
                  stroke-dasharray="{pagePx.width * 0.006} {pagePx.width * 0.004}"
                  pointer-events="none"
                />
                {#each corners as c (c.id)}
                  <rect
                    x={c.x - handleSize / 2}
                    y={c.y - handleSize / 2}
                    width={handleSize}
                    height={handleSize}
                    rx={2}
                    fill="#ffffff"
                    stroke="#4f46e5"
                    stroke-width={Math.max(1.5, pagePx.width * 0.002)}
                    class="pointer-events-auto {c.cls}"
                    role="button"
                    tabindex="-1"
                    aria-label={locale.t("comic.bubble_resize_handle")}
                    onpointerdown={(e) => startBubbleResize(e, b, c.id)}
                  />
                {/each}
                {#if b.shape !== "caption"}
                  <line
                    x1={geom.tailAnchor.x}
                    y1={geom.tailAnchor.y}
                    x2={tx}
                    y2={ty}
                    stroke="#6366f1"
                    stroke-width={Math.max(1.5, pagePx.width * 0.0018)}
                    stroke-dasharray="{pagePx.width * 0.005} {pagePx.width * 0.004}"
                    pointer-events="none"
                  />
                  <circle
                    cx={tx}
                    cy={ty}
                    r={tailHandleRadius}
                    fill="#eef2ff"
                    stroke="#4f46e5"
                    stroke-width={Math.max(1.5, pagePx.width * 0.0022)}
                    class="pointer-events-auto cursor-grab"
                    role="button"
                    tabindex="-1"
                    aria-label={locale.t("comic.bubble_tail_handle")}
                    onpointerdown={(e) => startBubbleTail(e, b)}
                  />
                {/if}
              {/if}
            {/each}
          </svg>
        {/if}
        </div>
      </div>

      <!-- Page Strip -->
      <div class="flex shrink-0 items-center gap-2 overflow-x-auto border-t border-neutral-800 bg-neutral-900/60 px-3 py-2 text-xs">
        {#each comic.pages as page, idx (page.id)}
          {@const active = idx === comic.activePageIndex}
          {@const thumbUrl = comic.pageUrlForPage(page.id)}
          {@const preview = page.layoutMode === "freeform" ? freeformRects([...page.freeformBoxes].sort((a, b) => a.z - b.z)) : panelRects(page.layout)}
          <div class="flex shrink-0 flex-col items-center gap-1 rounded-lg border p-1.5 transition-colors {active ? 'border-indigo-500 bg-indigo-500/10' : 'border-neutral-800 bg-neutral-900/70 hover:border-neutral-600'}">
            <button
              class="flex flex-col items-center gap-1"
              onclick={() => selectPage(idx)}
              aria-current={active ? "page" : undefined}
              aria-label={locale.t("comic.page_n", { n: String(idx + 1) })}
            >
              {#if thumbUrl}
                <img
                  src={thumbUrl}
                  alt={locale.t("comic.page_n", { n: String(idx + 1) })}
                  class="h-14 border border-neutral-700 bg-white object-cover"
                  style="aspect-ratio: {aspect}"
                />
              {:else}
                <svg viewBox="0 0 100 {Math.round(100 / aspect)}" class="h-14 border border-neutral-700 bg-neutral-100" style="aspect-ratio: {aspect}">
                  {#each [...preview.entries()] as [pid, r] (pid)}
                    {@const hasImg = !!(comic.resultsForPage(page.id)[pid]?.url || page.images[pid])}
                    <rect
                      x={r.x * 100 + 3}
                      y={(r.y * 100) / aspect + 3}
                      width={Math.max(1, r.w * 100 - 6)}
                      height={Math.max(1, (r.h * 100) / aspect - 6)}
                      fill={hasImg ? "#818cf8" : "#e5e7eb"}
                      stroke="#111827"
                      stroke-width="2"
                    />
                  {/each}
                </svg>
              {/if}
              <span class="text-[10px] font-medium {active ? 'text-indigo-300' : 'text-neutral-400'}">{idx + 1}</span>
            </button>
            <div class="flex items-center gap-0.5 text-[10px] text-neutral-400">
              <button
                class="rounded px-1 hover:bg-neutral-800 hover:text-neutral-200 disabled:opacity-30"
                disabled={comic.running || idx === 0}
                title={locale.t("comic.move_page_left")}
                aria-label={locale.t("comic.move_page_left")}
                onclick={() => comic.movePage(idx, idx - 1)}
              >←</button>
              <button
                class="rounded px-1 hover:bg-neutral-800 hover:text-neutral-200 disabled:opacity-30"
                disabled={comic.running || idx === comic.pages.length - 1}
                title={locale.t("comic.move_page_right")}
                aria-label={locale.t("comic.move_page_right")}
                onclick={() => comic.movePage(idx, idx + 1)}
              >→</button>
              <button
                class="rounded px-1 hover:bg-neutral-800 hover:text-neutral-200 disabled:opacity-30"
                disabled={comic.running}
                title={locale.t("comic.duplicate_page")}
                aria-label={locale.t("comic.duplicate_page")}
                onclick={() => comic.duplicatePage(idx)}
              >⧉</button>
              <button
                class="rounded px-1 hover:bg-neutral-800 hover:text-red-400 disabled:opacity-30"
                disabled={comic.running || comic.pages.length <= 1}
                title={locale.t("comic.delete_page")}
                aria-label={locale.t("comic.delete_page")}
                onclick={() => comic.deletePage(idx)}
              >&times;</button>
            </div>
          </div>
        {/each}
        <button
          class="flex h-24 shrink-0 items-center rounded-lg border border-dashed border-neutral-700 px-3 text-neutral-400 hover:border-indigo-500 hover:text-indigo-300 disabled:opacity-40"
          disabled={comic.running}
          onclick={() => comic.addPage()}
        >{locale.t("comic.add_page")}</button>
      </div>
    </div>

    <!-- Panels in reading order -->
    <aside class="space-y-2 border-neutral-800 p-3 text-xs lg:w-96 lg:shrink-0 lg:overflow-y-auto lg:border-l">
      {#if comic.selectedBubble}
        {@const sb = comic.selectedBubble}
        <div class="space-y-2 rounded-lg border border-indigo-500/70 bg-neutral-900 p-2">
          <div class="flex items-center justify-between gap-2">
            <span class="font-medium text-indigo-200">{locale.t("comic.bubble_inspector")}</span>
            <div class="flex items-center gap-1.5">
              <button
                type="button"
                class="rounded border border-red-700/60 px-1.5 py-0.5 text-[11px] text-red-300 hover:border-red-500 hover:bg-red-900/30"
                onclick={() => comic.removeBubble(sb.id)}
              >{locale.t("comic.bubble_delete")}</button>
              <button
                type="button"
                class="rounded px-1.5 py-0.5 text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200"
                aria-label={locale.t("comic.close")}
                onclick={() => (comic.selectedBubbleId = null)}
              >&times;</button>
            </div>
          </div>

          <div class="grid grid-cols-4 gap-1" role="group" aria-label={locale.t("comic.bubble_shape")}>
            {#each BUBBLE_SHAPES as shape (shape)}
              <button
                type="button"
                aria-pressed={sb.shape === shape}
                class="rounded border px-1.5 py-1 text-[11px] transition-colors {sb.shape === shape ? 'border-indigo-500 bg-indigo-600 text-white' : 'border-neutral-700 bg-neutral-950 text-neutral-300 hover:border-neutral-500'}"
                onclick={() => {
                  comic.beginBubbleEdit();
                  comic.updateBubble(sb.id, { shape });
                  comic.persist();
                }}
              >{locale.t(`comic.bubble_shape_${shape}`)}</button>
            {/each}
          </div>

          <div class="flex flex-wrap items-center gap-2">
            <label class="flex items-center gap-1 text-[11px] text-neutral-400">
              <span>{locale.t("comic.bubble_font")}</span>
              <select
                class="rounded border border-neutral-700 bg-neutral-950 px-1.5 py-1 text-neutral-200"
                value={sb.font}
                onchange={(e) => {
                  comic.beginBubbleEdit();
                  comic.updateBubble(sb.id, { font: (e.currentTarget as HTMLSelectElement).value as BubbleFont });
                  comic.persist();
                }}
              >
                {#each BUBBLE_FONTS as font (font)}
                  <option value={font}>{locale.t(`comic.bubble_font_${font}`)}</option>
                {/each}
              </select>
            </label>

            <label class="flex min-w-0 flex-1 items-center gap-1.5 text-[11px] text-neutral-400">
              <span>{locale.t("comic.bubble_size")}</span>
              <input
                type="range"
                min="10"
                max="64"
                step="1"
                class="min-w-0 flex-1 accent-indigo-500"
                value={sb.fontSize}
                onpointerdown={() => comic.beginBubbleEdit()}
                oninput={(e) => comic.updateBubble(sb.id, { fontSize: Number((e.currentTarget as HTMLInputElement).value) })}
                onchange={() => comic.persist()}
              />
              <input
                type="number"
                min="8"
                max="96"
                class="w-12 rounded border border-neutral-700 bg-neutral-950 px-1 py-0.5 text-center text-neutral-200"
                aria-label={locale.t("comic.bubble_size")}
                value={sb.fontSize}
                onchange={(e) => {
                  comic.beginBubbleEdit();
                  comic.updateBubble(sb.id, { fontSize: Number((e.currentTarget as HTMLInputElement).value) });
                  comic.persist();
                }}
              />
            </label>
          </div>

          <label class="flex items-center gap-1.5 text-[11px] text-neutral-300" title={locale.t("comic.bubble_vertical_title")}>
            <input
              type="checkbox"
              class="accent-indigo-500"
              checked={sb.vertical}
              onchange={(e) => {
                comic.beginBubbleEdit();
                comic.updateBubble(sb.id, { vertical: (e.currentTarget as HTMLInputElement).checked });
                comic.persist();
              }}
            />
            <span>{locale.t("comic.bubble_vertical")}</span>
          </label>

          <textarea
            rows="3"
            class="w-full resize-y rounded border border-neutral-700 bg-neutral-950 px-2 py-1 text-neutral-100 placeholder-neutral-600 focus:border-indigo-500 focus:outline-none"
            placeholder={locale.t("comic.bubble_text_placeholder")}
            aria-label={locale.t("comic.bubble_inspector")}
            value={sb.text}
            onfocus={() => (bubbleTextUndoArmed = true)}
            oninput={(e) => {
              if (bubbleTextUndoArmed) {
                comic.beginBubbleEdit();
                bubbleTextUndoArmed = false;
              }
              comic.updateBubble(sb.id, { text: (e.currentTarget as HTMLTextAreaElement).value });
            }}
            onblur={() => {
              bubbleTextUndoArmed = true;
              comic.persist();
            }}
          ></textarea>
        </div>
      {/if}
      {#each comic.orderedPanels as panel, i (panel.id)}
        {@const result = comic.results[panel.id]}
        {@const open = comic.selectedId === panel.id}
        <div class="rounded-lg border {open ? 'border-indigo-500/70 bg-neutral-900' : 'border-neutral-800 bg-neutral-900/50'}">
          <div class="flex items-center gap-2 px-2 py-1.5">
            <button class="flex min-w-0 flex-1 items-center gap-2 text-left" onclick={() => (comic.selectedId = open ? null : panel.id)} aria-expanded={open}>
              <span class="rounded-full bg-neutral-800 px-1.5 text-[10px] font-semibold text-neutral-200">{i + 1}</span>
              <span class="min-w-0 flex-1 truncate {panel.prompt || panel.beat ? 'text-neutral-200' : 'text-neutral-500'}">{panel.beat || panel.prompt || locale.t("comic.empty_panel")}</span>
            </button>
            {#if result?.issues && result.issues.length > 0}
              <span class="rounded-full bg-amber-500 px-1.5 text-[10px] font-semibold text-neutral-950">!{result.issues.length}</span>
            {/if}
            <label class="flex items-center gap-1 text-[10px] text-neutral-400" title={locale.t("comic.panel_on_title")}>
              <input
                type="checkbox"
                class="accent-indigo-500"
                checked={panel.enabled}
                onchange={(e) => comic.updatePanel(panel.id, { enabled: (e.currentTarget as HTMLInputElement).checked })}
              />
              {locale.t("comic.panel_on")}
            </label>
          </div>
          {#if open}
            <div class="space-y-1.5 border-t border-neutral-800 px-2 pb-2 pt-1.5">
              <input
                class="w-full rounded border border-neutral-700 bg-neutral-950 px-2 py-1 text-neutral-100 placeholder-neutral-600 focus:border-indigo-500 focus:outline-none"
                placeholder={locale.t("comic.beat_placeholder")}
                aria-label={locale.t("comic.beat")}
                value={panel.beat}
                oninput={(e) => comic.updatePanel(panel.id, { beat: (e.currentTarget as HTMLInputElement).value })}
              />
              <ComicPromptField
                value={panel.prompt}
                onchange={(next) => comic.updatePanel(panel.id, { prompt: next })}
                placeholder={locale.t("comic.prompt_placeholder")}
                ariaLabel={locale.t("comic.prompt")}
                rows={3}
                minHeight="min-h-16"
                suggestLoraKeywords={true}
                storageKey="mooshieui.promptHeight.comic.panelPrompt"
              />
              <div class="flex items-start gap-1.5">
                <textarea
                  rows="2"
                  class="min-w-0 flex-1 resize-y rounded border border-neutral-700 bg-neutral-950 px-2 py-1 text-neutral-100 placeholder-neutral-600 focus:border-indigo-500 focus:outline-none"
                  placeholder={locale.t("comic.dialogue_placeholder")}
                  aria-label={locale.t("comic.dialogue")}
                  value={panel.dialogue}
                  oninput={(e) => comic.updatePanel(panel.id, { dialogue: (e.currentTarget as HTMLTextAreaElement).value })}
                ></textarea>
                <button
                  type="button"
                  class="shrink-0 rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-[11px] text-neutral-200 hover:border-indigo-500 hover:text-indigo-200 disabled:opacity-40"
                  disabled={!panel.dialogue.trim()}
                  title={locale.t("comic.from_dialogue_title")}
                  onclick={() => comic.syncDialogueToBubbles(panel.id, generation.isNovelAi)}
                >{locale.t("comic.from_dialogue")}</button>
              </div>
              {#if panel.dialogue.trim() && !generation.isNovelAi}
                <p class="text-[10px] text-neutral-500">{locale.t("comic.dialogue_nai_only")}</p>
              {/if}
              <div class="flex flex-wrap items-center gap-1.5">
                <button
                  class="rounded border border-indigo-700/70 bg-indigo-900/30 px-2 py-0.5 text-[11px] text-indigo-200 hover:bg-indigo-800/40 disabled:opacity-40"
                  disabled={!promptAssistant.isAvailable || comic.enhancingId !== null || comic.drafting}
                  title={promptAssistant.isAvailable ? locale.t(v5 ? "comic.enhance_panel_title_v5" : "comic.enhance_panel_title") : locale.t("comic.assistant_needed")}
                  onclick={() => { void run(() => enhancePanel(panel.id)); }}
                >{comic.enhancingId === panel.id ? locale.t("comic.working") : locale.t("comic.enhance_panel")}</button>
                {#if comic.draftUndo && comic.draftUndo[panel.id]}
                  <button class="px-1 text-[11px] text-neutral-400 hover:text-neutral-200" onclick={undoDraft}>{locale.t("comic.undo_draft")}</button>
                {/if}
              </div>
              <ComicPromptField
                value={panel.negative}
                onchange={(next) => comic.updatePanel(panel.id, { negative: next })}
                placeholder={locale.t("comic.negative_placeholder")}
                ariaLabel={locale.t("comic.negative")}
                rows={2}
                minHeight="min-h-12"
                suggestLoraKeywords={false}
                storageKey="mooshieui.promptHeight.comic.panelNegative"
              />
              {#if comic.cast.length > 0}
                <div class="flex flex-wrap items-center gap-1">
                  <span class="text-[10px] text-neutral-500">{locale.t("comic.in_panel")}</span>
                  {#each comic.cast as member, i (member.id)}
                    {@const on = panel.cast.includes(member.id)}
                    <button
                      class="rounded-full border px-2 py-0.5 text-[11px] {on ? 'border-indigo-500 bg-indigo-500/15 text-indigo-200' : 'border-neutral-700 text-neutral-400 hover:border-neutral-500'}"
                      aria-pressed={on}
                      onclick={() => comic.toggleCastInPanel(panel.id, member.id)}
                    >{member.name || `#${i + 1}`}</button>
                  {/each}
                </div>
              {/if}
              <ComicPromptField
                value={panel.setting}
                onchange={(next) => comic.updatePanel(panel.id, { setting: next })}
                placeholder={comic.effectiveSetting.trim() ? locale.t("comic.setting_override_placeholder", { setting: comic.effectiveSetting.trim() }) : locale.t("comic.setting_override_empty")}
                ariaLabel={locale.t("comic.setting_override")}
                rows={1}
                minHeight="min-h-10"
                storageKey="mooshieui.promptHeight.comic.panelSetting"
              />
              {#if result?.error}
                <p class="text-[11px] text-red-400">{result.error}</p>
              {/if}
              {#if result?.checking}
                <p class="text-[11px] text-neutral-500">{locale.t("comic.checking")}</p>
              {:else if result?.checkError}
                <p class="text-[11px] text-amber-400">{locale.t("comic.check_failed", { message: result.checkError })}</p>
              {/if}
              {#if result?.issues && result.issues.length > 0}
                <ul class="space-y-0.5">
                  {#each result.issues as issue, k (k)}
                    <li class="rounded bg-amber-500/10 px-2 py-0.5 text-[11px] text-amber-200">
                      {#if issue.kind === "changed"}
                        {locale.t(issue.who ? "comic.issue_changed" : "comic.issue_setting_changed", { who: issue.who ?? "", expected: issue.expected, got: issue.got ?? "" })}
                      {:else}
                        {locale.t(issue.who ? "comic.issue_missing" : "comic.issue_setting_missing", { who: issue.who ?? "", expected: issue.expected })}
                      {/if}
                    </li>
                  {/each}
                </ul>
              {/if}
              <div class="flex flex-wrap items-center gap-2 pt-0.5">
                <label class="flex items-center gap-1.5 text-[11px] text-neutral-400">
                  <span>{locale.t("comic.border_style")}</span>
                  <select
                    class="rounded border border-neutral-700 bg-neutral-950 px-1.5 py-0.5 text-[11px] text-neutral-200"
                    value={normalizePanelBorderStyle(panel.borderStyle)}
                    onchange={(e) => comic.setPanelBorder(panel.id, (e.currentTarget as HTMLSelectElement).value as PanelBorderStyle)}
                  >
                    {#each PANEL_BORDER_STYLES as styleOpt (styleOpt)}
                      <option value={styleOpt}>{locale.t(`comic.border_${styleOpt}`)}</option>
                    {/each}
                  </select>
                </label>
                {#if result?.url || (panel.zoom ?? 1) > 1 || (panel.panX ?? 0) !== 0 || (panel.panY ?? 0) !== 0}
                  <span class="text-[11px] text-neutral-400 tabular-nums" title={locale.t("comic.zoom_title")}>
                    {locale.t("comic.zoom_readout", { zoom: (panel.zoom ?? 1).toFixed(1) })}
                  </span>
                  <button
                    type="button"
                    class="rounded border border-neutral-700 px-2 py-0.5 text-[11px] text-neutral-300 hover:border-neutral-500 disabled:opacity-40"
                    disabled={(panel.zoom ?? 1) === 1 && (panel.panX ?? 0) === 0 && (panel.panY ?? 0) === 0}
                    onclick={() => comic.resetPanelCrop(panel.id)}
                  >{locale.t("comic.reset_crop")}</button>
                {/if}
              </div>
              {#if !oneImage}
                {@const takes = comic.activePage.imageHistory[panel.id] ?? []}
                {@const currentTake = result?.galleryFilename ?? comic.activePage.images[panel.id] ?? takes[takes.length - 1]}
                {@const takeIndex = currentTake ? takes.indexOf(currentTake) : -1}
                <div class="flex flex-wrap items-center gap-1.5 pt-0.5">
                  <button
                    class="rounded-lg bg-indigo-600 px-2.5 py-1 font-medium text-white hover:bg-indigo-500 disabled:opacity-40"
                    disabled={comic.running}
                    onclick={() => { void run(() => generatePanels([panel.id])); }}
                  >{result?.url ? locale.t("comic.redo_panel") : locale.t("comic.generate_panel")}</button>
                  {#if takes.length > 1}
                    <div class="inline-flex items-center overflow-hidden rounded-lg border border-neutral-700 bg-neutral-950 text-[11px]">
                      <button
                        type="button"
                        class="px-1.5 py-1 text-neutral-300 hover:bg-neutral-800 disabled:opacity-30"
                        disabled={comic.running || takeIndex <= 0}
                        title={locale.t("comic.prev_take")}
                        aria-label={locale.t("comic.prev_take")}
                        onclick={() => { void run(() => switchPanelTake(panel.id, -1, "delta")); }}
                      >&lsaquo;</button>
                      <span class="px-1.5 text-neutral-400 tabular-nums" title={locale.t("comic.take_history_title")}>
                        {(takeIndex >= 0 ? takeIndex : takes.length - 1) + 1}/{takes.length}
                      </span>
                      <button
                        type="button"
                        class="px-1.5 py-1 text-neutral-300 hover:bg-neutral-800 disabled:opacity-30"
                        disabled={comic.running || (takeIndex >= 0 ? takeIndex >= takes.length - 1 : true)}
                        title={locale.t("comic.next_take")}
                        aria-label={locale.t("comic.next_take")}
                        onclick={() => { void run(() => switchPanelTake(panel.id, 1, "delta")); }}
                      >&rsaquo;</button>
                    </div>
                  {/if}
                  {#if result?.url && !result.checking}
                    <button
                      class="rounded-lg border border-neutral-700 px-2.5 py-1 hover:border-neutral-500"
                      onclick={() => { void checkPanel(panel.id); }}
                    >{locale.t("comic.check_again")}</button>
                  {/if}
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
      <button class="w-full rounded-lg border border-dashed border-neutral-700 py-1.5 text-neutral-500 hover:border-neutral-500 hover:text-neutral-300" onclick={() => comic.resetPanels()}>
        {locale.t("comic.start_over")}
      </button>
    </aside>
  </div>
</div>

{#if showPage && (modalPageUrl || comic.pageUrl)}
  <div
    use:portal
    data-modal-open="true"
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/80 p-4"
    role="dialog"
    aria-modal="true"
    aria-label={locale.t("comic.view_page")}
    tabindex="-1"
    onclick={(e) => { if (e.target === e.currentTarget) closePagePreview(); }}
    onkeydown={(e) => { if (e.key === "Escape") closePagePreview(); }}
  >
    <div class="flex max-h-full flex-col items-center gap-2">
      <img src={modalPageUrl ?? comic.pageUrl} alt={locale.t("comic.page_label")} class="max-h-[85vh] max-w-full object-contain shadow-2xl" />
      <div class="flex gap-2">
        <button class="rounded-lg bg-emerald-700 px-3 py-1.5 text-sm text-white hover:bg-emerald-600 disabled:opacity-40" disabled={saving} onclick={() => { void save(); }}>{locale.t("comic.save_page")}</button>
        <button class="rounded-lg bg-neutral-800 px-3 py-1.5 text-sm text-neutral-200 hover:bg-neutral-700" onclick={closePagePreview}>{locale.t("comic.close")}</button>
      </div>
    </div>
  </div>
{/if}

{#if showCharPicker}
  <div
    use:portal
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/75 p-4"
    role="dialog"
    aria-modal="true"
    aria-label={locale.t("comic.pick_character")}
    tabindex="-1"
    onclick={(e) => { if (e.target === e.currentTarget) showCharPicker = false; }}
    onkeydown={(e) => { if (e.key === "Escape") showCharPicker = false; }}
  >
    <div class="flex max-h-[80vh] w-full max-w-lg flex-col rounded-xl border border-neutral-700 bg-neutral-900 text-xs text-neutral-200 shadow-2xl">
      <div class="flex items-center justify-between border-b border-neutral-800 px-4 py-3">
        <h2 class="text-sm font-medium text-neutral-100">{locale.t("comic.pick_character")}</h2>
        <button
          class="rounded px-2 py-0.5 text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200"
          onclick={() => (showCharPicker = false)}
          aria-label={locale.t("comic.close")}
        >&times;</button>
      </div>
      <div class="flex-1 space-y-2 overflow-y-auto p-3">
        {#if sortedCharacters.length === 0}
          <p class="py-6 text-center text-neutral-500">{locale.t("comic.pick_character_empty")}</p>
        {:else}
          {#each sortedCharacters as ch (ch.id)}
            {@const img = ch.reference ?? ch.thumbnail}
            <button
              class="flex w-full items-center gap-3 rounded-lg border border-neutral-800 bg-neutral-950/60 p-2 text-left transition-colors hover:border-indigo-500 hover:bg-neutral-900"
              onclick={() => chooseCharacter(ch)}
            >
              {#if img}
                <img src={img} alt={ch.name} class="h-12 w-12 shrink-0 rounded border border-neutral-700 object-cover" />
              {:else}
                <div class="flex h-12 w-12 shrink-0 items-center justify-center rounded border border-neutral-800 bg-neutral-900 text-neutral-600">—</div>
              {/if}
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="truncate font-medium text-neutral-100">{ch.name}</span>
                  <span class="shrink-0 rounded bg-neutral-800 px-1.5 py-0.5 text-[10px] text-neutral-400">{architectureLabel(ch.architecture)}</span>
                </div>
                <p class="mt-0.5 truncate text-[11px] text-neutral-400">{ch.prompt}</p>
              </div>
            </button>
          {/each}
        {/if}
      </div>
    </div>
  </div>
{/if}
