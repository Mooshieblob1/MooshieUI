// Comic page builder: running it. Generates panels through the normal queue,
// checks continuity with the tagger, asks the Prompt Assistant about the story,
// assembles pages, and exports multi-page books to gallery, CBZ, and PDF.
// State lives in stores/comic.svelte.ts.

import { autocomplete } from "../stores/autocomplete.svelte.js";
import { comic, type ComicBook, type ComicPageState } from "../stores/comic.svelte.js";
import { gallery } from "../stores/gallery.svelte.js";
import { generation } from "../stores/generation.svelte.js";
import { locale } from "../stores/locale.svelte.js";
import { progress } from "../stores/progress.svelte.js";
import { promptAssistant } from "../stores/promptAssistant.svelte.js";
import type { GenerationParams, OutputImage, TagResult } from "../types/index.js";
import {
  callExternalLlm,
  interrogateGalleryImage,
  interrogateImage,
  loadGalleryImageDisplay,
  loadGalleryImagePng,
  saveImageFile,
  saveToGalleryBytes,
  uploadImageBytes,
} from "./api.js";
import { buildCbzBlob, buildPdfBlob, type CbzPageInput, type PdfPageInput } from "./comicExport.js";
import { drawBubbleOnCanvas } from "./comicBubbles.js";
import {
  DRAFT_SYSTEM,
  ONE_SHOT_MAX_PANELS,
  STORY_CHECK_SYSTEM,
  continuityIssues,
  coverCrop,
  dialogueLines,
  enhancePanelInstruction,
  joinTags,
  normalizePanelBorderStyle,
  oneShotBasePrompt,
  oneShotCentre,
  oneShotPanelPrompt,
  pageAspect,
  pageSize,
  panelBorderPolygon,
  panelCornerRadius,
  panelGenerationSize,
  panelPixelRect,
  panelPrompt,
  panelRects,
  panelSetting,
  parseDraftPrompts,
  parseStoryIssues,
  splitTags,
  storyBrief,
  summarizePageForContext,
  type ComicPanel,
  type PanelBorderStyle,
  type PanelDraft,
  type PreviousPageSummary,
  type Rect,
} from "./comicLayout.js";
import { assertAllowedRequest } from "./contentSafety.js";
import { loadOutputImageForGenerationInput } from "./galleryActions.js";
import { submitGeneration } from "./generationSubmit.js";
import { isTauri } from "./ipc.js";
import { resolveNaiLanguage } from "./naiLanguage.js";
import { NAI_MAX_TOKENS, naiComicSystemPrompt } from "./naiPrompt.js";
import { naiSystemWithSkill } from "./naiSkill.js";
import { NOVELAI_MAX_DIRECTOR_REFERENCES, naiV5Variant } from "./novelaiModels.js";

/** How long a finished prompt may take to show up as an image before it counts as lost. */
const IMAGE_GRACE_MS = 60_000;
/** Count tags say who is in frame, not what they look like; the tagger reads them loosely. */
const UNCHECKED_TAG = /^(\d+\+?(girl|boy|other)s?|solo|multiple (girls|boys)|no humans)$/;

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

function bytesToBase64(bytes: number[] | Uint8Array): string {
  const arr = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < arr.length; i += chunk) {
    binary += String.fromCharCode(...arr.subarray(i, i + chunk));
  }
  return btoa(binary);
}

/** Params for one picture of its own, the way the character portrait builds them. */
function baseParams(positive: string, negative: string, width: number, height: number): GenerationParams {
  const params = generation.toParams({
    prompts: { positive, negative },
    includeConditioningRegions: false,
    outsidePausedRun: true,
    seed: "-1",
    overrides: { mode: "txt2img", input_image: null, mask_image: null },
  });
  params.width = width;
  params.height = height;
  params.batch_size = 1;
  params.upscale_enabled = false;
  params.save_pre_upscale_image = false;
  params.controlnet = null;
  params.positive_regions = [];
  params.style_transfer_enabled = false;
  params.style_ref_enabled = false;
  params.edit_reference_images = [];
  return params;
}

/** Wait for a queued prompt's image. Resolves null when stopped. */
async function waitForImage(promptId: string): Promise<OutputImage | null> {
  let leftQueueAt = 0;
  for (;;) {
    if (comic.stopRequested && !progress.pendingPrompts.some((p) => p.promptId === promptId)) return null;
    const pending = progress.pendingPrompts.some((p) => p.promptId === promptId);
    if (!pending && progress.takeFailed(promptId)) throw new Error(locale.t("comic.error_generation_failed"));
    const image = gallery.sessionImages.find((i) => i.prompt_id === promptId);
    if (image && !pending) return image;
    if (!pending) {
      if (!leftQueueAt) leftQueueAt = Date.now();
      else if (Date.now() - leftQueueAt > IMAGE_GRACE_MS) throw new Error(locale.t("comic.error_no_image"));
    }
    await sleep(300);
  }
}

async function imageBytes(image: OutputImage): Promise<{ png: Uint8Array; galleryFilename: string | null }> {
  const galleryFilename = await gallery.resolveGalleryFilename(image);
  if (galleryFilename) {
    return { png: new Uint8Array(await loadGalleryImagePng(galleryFilename)), galleryFilename };
  }
  const { bytes } = await loadOutputImageForGenerationInput(image);
  return { png: new Uint8Array(bytes), galleryFilename: null };
}

function objectUrl(png: Uint8Array): string {
  return URL.createObjectURL(new Blob([png as BlobPart], { type: "image/png" }));
}

function checkGenerationReady() {
  // The page's own settings only ever make still images. App.svelte already
  // keeps them on txt2img; this covers anything that switched them since.
  if (generation.mode !== "txt2img") generation.setMode("txt2img");
  if (!generation.checkpoint) throw new Error(locale.t("generation.error_no_checkpoint"));
}

// ---------------------------------------------------------------------------
// Per-panel generation & cross-page visual references
// ---------------------------------------------------------------------------

/**
 * A panel's request, built from the Comic page's own settings. Built for every
 * panel before the first one is queued, so leaving the page mid-run (which
 * swaps the Generate tab's settings back in) can't change the later panels.
 */
function panelParams(panel: ComicPanel, page: ComicPageState, book: ComicBook): GenerationParams | null {
  const rect = comic.rectsForPage(page).get(panel.id);
  if (!rect) return null;
  const size = panelGenerationSize(rect, pageSize(book.pageShape), { width: generation.width, height: generation.height });
  const setting = comic.effectiveSettingFor(page, book);
  // NovelAI letters the panel's dialogue from a Text: block; other models get none.
  const positive = panelPrompt(panel, { style: book.style, setting, cast: book.cast }, generation.isNovelAi);
  if (!positive) throw new Error(locale.t("comic.error_empty_prompt"));
  const params = baseParams(positive, panel.negative.trim(), size.width, size.height);
  if (params.novelai) {
    params.novelai = { ...params.novelai, characters: [], use_coords: false, vibes: [], director_references: [] };
  }
  return params;
}

export type Panel1RefKind = "novelai" | "anima_ref" | "image_edit";

/** Which character-reference path the active checkpoint supports. */
export function panel1ReferenceKind(): Panel1RefKind | null {
  if (generation.isNovelAi) {
    return generation.supportsNovelAiPreciseReference ? "novelai" : null;
  }
  if (generation.modelFamily === "anima") return "anima_ref";
  if (generation.supportsImageEditMode) return "image_edit";
  return null;
}

/** True when the active checkpoint supports passing character reference images. */
export function supportsPanel1Reference(): boolean {
  return panel1ReferenceKind() !== null;
}

async function resolvePanelPng(page: ComicPageState, panelId: string): Promise<Uint8Array | null> {
  const res = comic.resultsForPage(page.id)[panelId];
  const filename = res?.galleryFilename ?? page.images[panelId];
  if (filename) {
    try {
      return new Uint8Array(await loadGalleryImagePng(filename));
    } catch {
      /* fall through to object URL */
    }
  }
  if (res?.url) {
    try {
      return new Uint8Array(await (await fetch(res.url)).arrayBuffer());
    } catch {
      return null;
    }
  }
  return null;
}

async function resolveReferenceSourceToPng(source: string): Promise<Uint8Array | null> {
  if (!source) return null;
  try {
    if (source.startsWith("data:")) {
      const blob = await (await fetch(source)).blob();
      const bitmap = await createImageBitmap(blob);
      const canvas = document.createElement("canvas");
      canvas.width = bitmap.width;
      canvas.height = bitmap.height;
      const ctx = canvas.getContext("2d");
      if (!ctx) {
        bitmap.close();
        return null;
      }
      ctx.drawImage(bitmap, 0, 0);
      bitmap.close();
      const pngBlob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
      if (!pngBlob) return null;
      return new Uint8Array(await pngBlob.arrayBuffer());
    }
    return new Uint8Array(await loadGalleryImagePng(source));
  } catch {
    return null;
  }
}

async function applyReferenceImages(
  params: GenerationParams,
  kind: Panel1RefKind,
  pngs: Uint8Array[],
  uploadCache: Map<Uint8Array, string>,
): Promise<void> {
  if (pngs.length === 0) return;
  if (kind === "novelai") {
    if (params.novelai) {
      params.novelai = {
        ...params.novelai,
        director_references: pngs.slice(0, NOVELAI_MAX_DIRECTOR_REFERENCES).map((png) => ({
          image: bytesToBase64(png),
          description: "character",
          strength: 0.6,
          fidelity: 1.0,
        })),
      };
    }
    return;
  }

  const maxUploads = kind === "anima_ref" ? 2 : pngs.length;
  const uploadedNames: string[] = [];
  for (let i = 0; i < Math.min(pngs.length, maxUploads); i++) {
    const png = pngs[i];
    let name = uploadCache.get(png);
    if (!name) {
      const upload = await uploadImageBytes(Array.from(png), `comic_ref_${Date.now()}_${i}.png`);
      name = upload.name;
      uploadCache.set(png, name);
    }
    uploadedNames.push(name);
  }
  if (uploadedNames.length === 0) return;

  if (kind === "anima_ref") {
    params.style_ref_enabled = true;
    params.anima_ref_method = "anima_ref_adapter";
    params.style_ref_image = uploadedNames[0];
    params.style_ref_image_2 = uploadedNames[1] ?? null;
    params.style_ref_end = 1.0;
    params.anima_ref_guidance = generation.animaRefGuidance;
    params.anima_ref_correction = generation.animaRefCorrection;
  } else {
    params.mode = "image_edit";
    params.edit_reference_images = uploadedNames;
    params.input_image = uploadedNames[0];
  }
}

/** Find the immediately preceding enabled panel across the book (crossing page boundaries). */
function findPrecedingPanel(
  book: ComicBook,
  pageIndex: number,
  panelId: string,
): { page: ComicPageState; panel: ComicPanel } | null {
  const curPage = book.pages[pageIndex];
  if (!curPage) return null;
  const curOrdered = comic.orderedPanelsForPage(curPage, book.rightToLeft);
  const idx = curOrdered.findIndex((p) => p.id === panelId);
  for (let i = idx - 1; i >= 0; i--) {
    if (curOrdered[i].enabled) return { page: curPage, panel: curOrdered[i] };
  }
  for (let p = pageIndex - 1; p >= 0; p--) {
    const prevPage = book.pages[p];
    const prevOrdered = comic.orderedPanelsForPage(prevPage, book.rightToLeft);
    for (let i = prevOrdered.length - 1; i >= 0; i--) {
      if (prevOrdered[i].enabled) return { page: prevPage, panel: prevOrdered[i] };
    }
  }
  return null;
}

async function generateOne(
  panel: ComicPanel,
  params: GenerationParams,
  pageId: string,
  book: ComicBook,
  page: ComicPageState,
): Promise<Uint8Array | null> {
  comic.setResult(panel.id, { status: "running", error: undefined, issues: null, checkError: undefined }, pageId);
  const promptId = await submitGeneration(params);
  const image = await waitForImage(promptId);
  if (!image) {
    comic.setResult(panel.id, { status: "queued" }, pageId);
    return null;
  }
  const { png, galleryFilename } = await imageBytes(image);
  const url = objectUrl(png);
  if (galleryFilename) {
    comic.cacheTakeUrl(galleryFilename, url, pageId);
    comic.recordPanelTake(panel.id, galleryFilename, pageId);
  }
  comic.setResult(panel.id, { status: "done", url, galleryFilename }, pageId);
  if (book.checkContinuity) void checkPanel(panel.id, png, galleryFilename, pageId, book, page);
  return png;
}

/** Generate the given panels on the active page (default: every enabled panel), one after another. */
export async function generatePanels(ids?: string[]): Promise<void> {
  if (comic.running) return;
  checkGenerationReady();
  const book = comic.activeBook;
  const page = comic.activePage;
  const pageIndex = comic.activePageIndex;
  const ordered = comic.orderedPanelsForPage(page, book.rightToLeft);
  const targets = (ids ? ordered.filter((p) => ids.includes(p.id)) : ordered.filter((p) => p.enabled)).filter(
    (p) => p.enabled || ids,
  );
  if (targets.length === 0) return;

  comic.running = true;
  comic.stopRequested = false;
  for (const p of targets) comic.setResult(p.id, { status: "queued", error: undefined }, page.id);

  const prepared = new Map<string, GenerationParams | Error | null>();
  for (const p of targets) {
    try {
      prepared.set(p.id, panelParams(p, page, book));
    } catch (e) {
      prepared.set(p.id, e instanceof Error ? e : new Error(String(e)));
    }
  }

  const refMode = book.refMode;
  const refKind = refMode !== "off" ? panel1ReferenceKind() : null;
  const page1 = book.pages[0];
  const page1Panel1Id = page1 ? (comic.orderedPanelsForPage(page1, book.rightToLeft)[0]?.id ?? null) : null;
  let page1Panel1Png: Uint8Array | null = null;
  const generatedPngs = new Map<string, Uint8Array>();
  const castPngCache = new Map<string, Uint8Array | null>();
  const uploadCache = new Map<Uint8Array, string>();

  try {
    for (const panel of targets) {
      if (comic.stopRequested) break;
      try {
        const params = prepared.get(panel.id);
        if (params instanceof Error) throw params;
        if (!params) continue;

        if (refKind) {
          let refPngs: Uint8Array[] = [];
          if (refMode === "page1_panel1" && page1 && page1Panel1Id) {
            if (!(page.id === page1.id && panel.id === page1Panel1Id)) {
              if (!page1Panel1Png) page1Panel1Png = await resolvePanelPng(page1, page1Panel1Id);
              if (page1Panel1Png) refPngs = [page1Panel1Png];
            }
          } else if (refMode === "prev_panel") {
            const prev = findPrecedingPanel(book, pageIndex, panel.id);
            if (prev) {
              const key = `${prev.page.id}:${prev.panel.id}`;
              const prevPng = generatedPngs.get(key) ?? (await resolvePanelPng(prev.page, prev.panel.id));
              if (prevPng) refPngs = [prevPng];
            }
          } else if (refMode === "cast") {
            for (const castId of panel.cast) {
              const member = book.cast.find((c) => c.id === castId);
              if (!member?.referenceImage) continue;
              const cacheKey = `${member.id}:${member.referenceImage}`;
              let png = castPngCache.get(cacheKey);
              if (png === undefined) {
                png = await resolveReferenceSourceToPng(member.referenceImage);
                castPngCache.set(cacheKey, png);
              }
              if (png) refPngs.push(png);
            }
          }
          if (refPngs.length > 0) {
            await applyReferenceImages(params, refKind, refPngs, uploadCache);
          }
        }

        const generated = await generateOne(panel, params, page.id, book, page);
        if (generated) {
          generatedPngs.set(`${page.id}:${panel.id}`, generated);
          if (page1 && page.id === page1.id && panel.id === page1Panel1Id) {
            page1Panel1Png = generated;
          }
        }
      } catch (e) {
        comic.setResult(panel.id, { status: "failed", error: messageOf(e) }, page.id);
      }
    }
  } finally {
    // Panels still marked queued after a stop go back to their last state.
    for (const p of targets) {
      const results = comic.resultsForPage(page.id);
      const r = results[p.id];
      if (r?.status === "queued" && !r.url) {
        comic.clearResult(p.id, page.id);
      } else if (r?.status === "queued") {
        comic.setResult(p.id, { status: "done" }, page.id);
      }
    }
    comic.running = false;
    comic.stopRequested = false;
  }
}

export function stopRun() {
  if (comic.running) comic.stopRequested = true;
}

// ---------------------------------------------------------------------------
// Continuity: the tagger on one panel
// ---------------------------------------------------------------------------

function checkable(tags: string): string[] {
  return splitTags(tags).filter((t) => !UNCHECKED_TAG.test(t) && autocomplete.isKnownTag(t));
}

export async function checkPanel(
  id: string,
  png?: Uint8Array,
  galleryFilename?: string | null,
  pageId = comic.activePage.id,
  book: ComicBook = comic.activeBook,
  page: ComicPageState = comic.activePage,
): Promise<void> {
  const panel = page.panels[id];
  const result = comic.resultsForPage(pageId)[id];
  if (!panel || !result) return;
  comic.setResult(id, { checking: true, checkError: undefined }, pageId);
  try {
    const name = galleryFilename ?? result.galleryFilename ?? null;
    let detected: TagResult[] = [];
    let bytes = png;
    if (!bytes && !name && result.url) bytes = new Uint8Array(await (await fetch(result.url)).arrayBuffer());
    const res = name ? await interrogateGalleryImage(name) : await interrogateImage(bytesToBase64(bytes ?? new Uint8Array()));
    detected = [...res.general_tags, ...res.character_tags];
    const effectiveSetting = comic.effectiveSettingFor(page, book);
    const expected = [
      ...book.cast
        .filter((c) => panel.cast.includes(c.id))
        .map((c) => ({ who: c.name || "?", tags: checkable(c.tags) })),
      { who: null, tags: checkable(panelSetting(panel, effectiveSetting)) },
    ].filter((g) => g.tags.length > 0);
    comic.setResult(id, { checking: false, issues: continuityIssues(expected, detected.map((t) => t.name)) }, pageId);
  } catch (e) {
    comic.setResult(id, { checking: false, checkError: messageOf(e) }, pageId);
  }
}

// ---------------------------------------------------------------------------
// One image (NovelAI): every panel as a positioned character prompt
// ---------------------------------------------------------------------------

export async function generateOneImage(): Promise<void> {
  if (comic.running) return;
  checkGenerationReady();
  if (!generation.isNovelAi) throw new Error(locale.t("comic.error_one_image_novelai"));
  const book = comic.activeBook;
  const page = comic.activePage;
  const panels = comic.enabledPanels;
  if (panels.length === 0) return;
  if (panels.length > ONE_SHOT_MAX_PANELS) {
    throw new Error(locale.t("comic.error_one_image_panels", { max: String(ONE_SHOT_MAX_PANELS) }));
  }
  const aspect = pageAspect(book.pageShape);
  const pixels = generation.width * generation.height;
  const snap = (v: number) => Math.max(256, Math.round(v / 64) * 64);
  const width = snap(Math.sqrt(pixels * aspect));
  const height = snap(Math.sqrt(pixels / aspect));

  const effectiveSetting = comic.effectiveSettingFor(page, book);
  const dialogue = panels.flatMap((p) => dialogueLines(p.dialogue ?? ""));
  const params = baseParams(
    oneShotBasePrompt({ style: book.style, setting: effectiveSetting }, panels.length, dialogue),
    "",
    width,
    height,
  );
  if (!params.novelai) throw new Error(locale.t("comic.error_one_image_novelai"));
  params.novelai = {
    ...params.novelai,
    use_coords: true,
    vibes: [],
    director_references: [],
    characters: panels.map((p) => ({
      prompt: oneShotPanelPrompt(p, comic.numberOf(p.id), { setting: effectiveSetting, cast: book.cast }),
      negative_prompt: p.negative.trim(),
      center: oneShotCentre(comic.rects.get(p.id)!),
      enabled: true,
    })),
  };

  comic.running = true;
  comic.stopRequested = false;
  comic.pageStatus = "running";
  comic.pageError = null;
  try {
    const promptId = await submitGeneration(params);
    const image = await waitForImage(promptId);
    if (!image) {
      comic.pageStatus = "idle";
      return;
    }
    const { png, galleryFilename } = await imageBytes(image);
    comic.setPage(objectUrl(png), galleryFilename, page.id);
    comic.pageStatus = "done";
  } catch (e) {
    comic.pageStatus = "failed";
    comic.pageError = messageOf(e);
  } finally {
    comic.running = false;
    comic.stopRequested = false;
  }
}

// ---------------------------------------------------------------------------
// Assembling, saving and exporting pages (CBZ & PDF)
// ---------------------------------------------------------------------------

async function loadBitmap(url: string): Promise<ImageBitmap> {
  const blob = await (await fetch(url)).blob();
  return createImageBitmap(blob);
}

async function loadBitmapForPanel(page: ComicPageState, panelId: string): Promise<ImageBitmap | null> {
  const rtUrl = comic.resultsForPage(page.id)[panelId]?.url;
  if (rtUrl) {
    try {
      return await loadBitmap(rtUrl);
    } catch {
      /* fall through to gallery file */
    }
  }
  const filename = comic.resultsForPage(page.id)[panelId]?.galleryFilename ?? page.images[panelId];
  if (filename) {
    try {
      const bytes = await loadGalleryImageDisplay(filename);
      return await createImageBitmap(new Blob([new Uint8Array(bytes)], { type: "image/webp" }));
    } catch {
      /* deleted from gallery */
    }
  }
  return null;
}

async function loadBitmapForOneImagePage(page: ComicPageState): Promise<ImageBitmap | null> {
  const rtUrl = comic.pageUrlForPage(page.id);
  if (rtUrl) {
    try {
      return await loadBitmap(rtUrl);
    } catch {
      /* fall through */
    }
  }
  if (page.pageImage) {
    try {
      const bytes = await loadGalleryImagePng(page.pageImage);
      return await createImageBitmap(new Blob([new Uint8Array(bytes)], { type: "image/png" }));
    } catch {
      /* gone */
    }
  }
  return null;
}

async function renderPageCanvas(pageIndex: number): Promise<HTMLCanvasElement> {
  const book = comic.activeBook;
  const page = book.pages[pageIndex];
  if (!page) throw new Error(`Page ${pageIndex + 1} not found`);

  if (book.mode === "one_image") {
    const bitmap = await loadBitmapForOneImagePage(page);
    if (!bitmap) throw new Error(locale.t("comic.error_nothing_to_save"));
    const canvas = document.createElement("canvas");
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      bitmap.close();
      throw new Error("Canvas is not available");
    }
    ctx.drawImage(bitmap, 0, 0);
    bitmap.close();
    for (const b of page.bubbles ?? []) {
      drawBubbleOnCanvas(ctx, b, canvas.width, canvas.height);
    }
    return canvas;
  }

  const px = pageSize(book.pageShape);
  const canvas = document.createElement("canvas");
  canvas.width = px.width;
  canvas.height = px.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("Canvas is not available");
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, px.width, px.height);

  const border = Math.max(2, Math.round(Math.max(px.width, px.height) / 600));
  const traceBorderPath = (targetBox: Rect, style: PanelBorderStyle, expand = 0) => {
    const b: Rect =
      expand === 0
        ? targetBox
        : {
            x: targetBox.x - expand,
            y: targetBox.y - expand,
            w: targetBox.w + expand * 2,
            h: targetBox.h + expand * 2,
          };
    ctx.beginPath();
    if (style === "rounded") {
      const r = panelCornerRadius(targetBox) + Math.max(0, expand);
      ctx.roundRect(b.x, b.y, b.w, b.h, r);
    } else if (style === "slanted") {
      const poly = panelBorderPolygon(targetBox, "slanted");
      ctx.moveTo(poly[0].x, poly[0].y);
      for (let i = 1; i < poly.length; i++) ctx.lineTo(poly[i].x, poly[i].y);
      ctx.closePath();
    } else {
      ctx.rect(b.x, b.y, b.w, b.h);
    }
  };

  const drawPanel = async (id: string, box: Rect, halo: number) => {
    const panel = page.panels[id];
    const style = normalizePanelBorderStyle(panel?.borderStyle);

    if (halo > 0) {
      ctx.fillStyle = "#ffffff";
      if (style === "slanted") {
        traceBorderPath(box, "slanted", 0);
        ctx.fill();
        ctx.strokeStyle = "#ffffff";
        ctx.lineWidth = halo * 2;
        ctx.lineJoin = "miter";
        ctx.miterLimit = 4;
        ctx.stroke();
      } else {
        traceBorderPath(box, style, halo);
        ctx.fill();
      }
    }

    ctx.save();
    traceBorderPath(box, style, 0);
    ctx.clip();
    if (halo > 0) {
      // Ensure overlapping upper panels occlude lower panels even before an image is generated.
      ctx.fillStyle = "#ffffff";
      ctx.fillRect(box.x, box.y, box.w, box.h);
    }
    if (panel?.enabled) {
      const bitmap = await loadBitmapForPanel(page, id);
      if (bitmap) {
        const crop = coverCrop(bitmap, box, panel.panX ?? 0, panel.panY ?? 0, panel.zoom ?? 1);
        ctx.drawImage(bitmap, crop.x, crop.y, crop.w, crop.h, box.x, box.y, box.w, box.h);
        bitmap.close();
      }
    }
    if (style !== "borderless") {
      traceBorderPath(box, style, 0);
      ctx.strokeStyle = "#000000";
      ctx.lineWidth = border * 2;
      ctx.lineJoin = style === "slanted" ? "miter" : "round";
      ctx.miterLimit = 4;
      ctx.stroke();
    }
    ctx.restore();
  };

  if (page.layoutMode === "freeform") {
    const halo = Math.max(4, Math.round(book.gutter / 2));
    const ordered = [...page.freeformBoxes].sort((a, b) => a.z - b.z);
    for (const b of ordered) {
      const x = Math.round(b.x * px.width);
      const y = Math.round(b.y * px.height);
      const x1 = Math.round((b.x + b.w) * px.width);
      const y1 = Math.round((b.y + b.h) * px.height);
      await drawPanel(b.id, { x, y, w: Math.max(1, x1 - x), h: Math.max(1, y1 - y) }, halo);
    }
  } else {
    for (const [id, rect] of panelRects(page.layout)) {
      await drawPanel(id, panelPixelRect(rect, px, book.gutter), 0);
    }
  }
  for (const b of page.bubbles ?? []) {
    drawBubbleOnCanvas(ctx, b, canvas.width, canvas.height);
  }
  return canvas;
}

function canvasToBlob(canvas: HTMLCanvasElement, mimeType: "image/png" | "image/jpeg", quality?: number): Promise<Blob> {
  return new Promise<Blob>((resolve, reject) =>
    canvas.toBlob((b) => (b ? resolve(b) : reject(new Error("Could not encode the page"))), mimeType, quality),
  );
}

/** Draw the finished panels onto one page for the given page index. */
export async function assemblePageFor(pageIndex: number): Promise<Blob> {
  const canvas = await renderPageCanvas(pageIndex);
  return canvasToBlob(canvas, "image/png");
}

/** Draw the finished panels onto the active page. Panels without an image stay blank. */
export async function assemblePage(): Promise<Blob> {
  return assemblePageFor(comic.activePageIndex);
}

export async function previewPage(): Promise<void> {
  const blob = await assemblePage();
  comic.setPage(URL.createObjectURL(blob), null);
  comic.pageStatus = "done";
}

async function savePageAt(pageIndex: number): Promise<string> {
  const book = comic.activeBook;
  const page = book.pages[pageIndex];
  if (!page || !comic.pageHasContent(page, book)) throw new Error(locale.t("comic.error_nothing_to_save"));

  let bytes: Uint8Array;
  const hasBubbles = (page.bubbles?.length ?? 0) > 0;
  if (book.mode === "one_image" && !hasBubbles) {
    if (page.pageImage) return page.pageImage;
    const url = comic.pageUrlForPage(page.id);
    if (!url) throw new Error(locale.t("comic.error_nothing_to_save"));
    bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
  } else if (book.mode === "one_image") {
    const blob = await assemblePageFor(pageIndex);
    bytes = new Uint8Array(await blob.arrayBuffer());
  } else {
    const blob = await assemblePageFor(pageIndex);
    bytes = new Uint8Array(await blob.arrayBuffer());
    comic.setPage(URL.createObjectURL(blob), null, page.id);
  }

  const ordered = comic.orderedPanelsForPage(page, book.rightToLeft).filter((p) => p.enabled);
  const summary = ordered.map((p, i) => `Panel ${i + 1}: ${p.beat.trim() || p.prompt.trim()}`).join("\n");
  const effectiveSetting = comic.effectiveSettingFor(page, book);
  const saved = await saveToGalleryBytes(
    Array.from(bytes),
    "comic_page.png",
    `comic${Date.now().toString(36)}`,
    "txt2img",
    { positive_prompt: joinTags(book.style, effectiveSetting), comic_panels: summary },
  );
  await gallery.addPersistedImage(saved, undefined, true);
  if (book.mode !== "one_image" || !hasBubbles) {
    comic.setPage(comic.pageUrlForPage(page.id), saved, page.id);
  }
  return saved;
}

/** Save the active page to the gallery. */
export async function savePage(): Promise<string> {
  return savePageAt(comic.activePageIndex);
}

/** Save every page in the active book that has generated content to the gallery. */
export async function saveAllPages(): Promise<string[]> {
  const book = comic.activeBook;
  const indices = book.pages
    .map((p, i) => (comic.pageHasContent(p, book) ? i : -1))
    .filter((i) => i >= 0);
  if (indices.length === 0) throw new Error(locale.t("comic.error_nothing_to_save"));
  const saved: string[] = [];
  for (const idx of indices) {
    saved.push(await savePageAt(idx));
  }
  return saved;
}

function bookExportSlug(title: string): string {
  return title
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "") || "comic";
}

async function saveExportBlob(
  blob: Blob,
  defaultFilename: string,
  filterName: string,
  extension: string,
): Promise<"saved" | "downloaded" | "cancelled"> {
  if (isTauri) {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({
      defaultPath: defaultFilename,
      filters: [{ name: filterName, extensions: [extension] }],
    });
    if (!path) return "cancelled";
    const bytes = new Uint8Array(await blob.arrayBuffer());
    await saveImageFile(Array.from(bytes), path);
    return "saved";
  }
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = defaultFilename;
  document.body.appendChild(a);
  try {
    a.click();
  } finally {
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
  }
  return "downloaded";
}

/** Export all non-empty pages of the active book as a CBZ archive. */
export async function exportBookCbz(): Promise<"saved" | "downloaded" | "cancelled"> {
  const book = comic.activeBook;
  const indices = book.pages
    .map((p, i) => (comic.pageHasContent(p, book) ? i : -1))
    .filter((i) => i >= 0);
  if (indices.length === 0) throw new Error(locale.t("comic.error_nothing_to_save"));

  const pages: CbzPageInput[] = [];
  for (let k = 0; k < indices.length; k++) {
    const blob = await assemblePageFor(indices[k]);
    pages.push({
      name: `${String(k + 1).padStart(3, "0")}.png`,
      bytes: new Uint8Array(await blob.arrayBuffer()),
    });
  }
  const cbz = buildCbzBlob(pages);
  return saveExportBlob(cbz, `${bookExportSlug(book.title)}.cbz`, "CBZ", "cbz");
}

/** Export all non-empty pages of the active book as a PDF document. */
export async function exportBookPdf(): Promise<"saved" | "downloaded" | "cancelled"> {
  const book = comic.activeBook;
  const indices = book.pages
    .map((p, i) => (comic.pageHasContent(p, book) ? i : -1))
    .filter((i) => i >= 0);
  if (indices.length === 0) throw new Error(locale.t("comic.error_nothing_to_save"));

  const pages: PdfPageInput[] = [];
  for (const idx of indices) {
    const canvas = await renderPageCanvas(idx);
    const jpegBlob = await canvasToBlob(canvas, "image/jpeg", 0.92);
    pages.push({
      jpegBytes: new Uint8Array(await jpegBlob.arrayBuffer()),
      width: canvas.width,
      height: canvas.height,
    });
  }
  const pdf = buildPdfBlob(pages);
  return saveExportBlob(pdf, `${bookExportSlug(book.title)}.pdf`, "PDF", "pdf");
}

/** Switch a panel to an earlier or later take, lazy-loading from the gallery if needed. */
export async function switchPanelTake(
  panelId: string,
  deltaOrIndex: number,
  mode?: "delta" | "index",
): Promise<void> {
  const page = comic.activePage;
  const filename = comic.selectPanelTake(panelId, deltaOrIndex, mode);
  if (!filename) return;
  if (comic.getTakeUrl(filename, page.id)) return;
  try {
    const bytes = await loadGalleryImageDisplay(filename);
    const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "image/webp" }));
    comic.cacheTakeUrl(filename, url, page.id);
    if (comic.activePage.id === page.id && comic.activePage.images[panelId] === filename) {
      comic.setResult(
        panelId,
        { status: "done", url, galleryFilename: filename, issues: null, error: undefined },
        page.id,
      );
    }
  } catch {
    // Take file was deleted from the gallery: prune it and fall back cleanly to any remaining take.
    comic.pruneDeletedTake(panelId, filename, page.id);
    const remaining = page.imageHistory[panelId] ?? [];
    if (remaining.length > 0) {
      await switchPanelTake(panelId, remaining.length - 1, "index");
    }
  }
}

/** Bring back saved panel images for a single page if not yet cached in runtime. */
export async function restoreImagesForPage(pageId: string): Promise<void> {
  const book = comic.books.find((b) => b.pages.some((p) => p.id === pageId));
  const page = book?.pages.find((p) => p.id === pageId);
  if (!page) return;

  const rtResults = comic.resultsForPage(page.id);
  for (const [id, filename] of Object.entries(page.images)) {
    if (!page.panels[id] || rtResults[id]?.url) continue;
    try {
      const bytes = await loadGalleryImageDisplay(filename);
      const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "image/webp" }));
      comic.cacheTakeUrl(filename, url, page.id);
      comic.setResult(id, { status: "done", url, galleryFilename: filename, issues: null }, page.id);
    } catch {
      /* the file was deleted from the gallery: the panel just starts empty */
    }
  }
  if (page.pageImage && !comic.pageUrlForPage(page.id)) {
    try {
      const bytes = await loadGalleryImageDisplay(page.pageImage);
      comic.setPage(URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "image/webp" })), page.pageImage, page.id);
    } catch {
      /* gone */
    }
  }
}

/** Bring back panel images for the active page and page thumbnails for the book strip. */
export async function restoreImages(): Promise<void> {
  await restoreImagesForPage(comic.activePage.id);
  for (const page of comic.activeBook.pages) {
    if (page.id === comic.activePage.id) continue;
    if (page.pageImage && !comic.pageUrlForPage(page.id)) {
      try {
        const bytes = await loadGalleryImageDisplay(page.pageImage);
        comic.setPage(URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "image/webp" })), page.pageImage, page.id);
      } catch {
        /* gone */
      }
    }
  }
}

// ---------------------------------------------------------------------------
// Story: Prompt Assistant (with cross-page context)
// ---------------------------------------------------------------------------

function previousPageSummaries(
  book: ComicBook = comic.activeBook,
  pageIndex: number = comic.activePageIndex,
): PreviousPageSummary[] {
  const start = Math.max(0, pageIndex - 2);
  const out: PreviousPageSummary[] = [];
  for (let i = start; i < pageIndex; i++) {
    const p = book.pages[i];
    if (!p) continue;
    const summary = summarizePageForContext(comic.orderedPanelsForPage(p, book.rightToLeft), book.cast, p.settingOverride);
    if (summary) out.push({ pageNumber: i + 1, summary });
  }
  return out;
}

export async function checkStory(): Promise<void> {
  const ordered = comic.enabledPanels;
  if (ordered.length === 0) return;
  comic.storyChecking = true;
  comic.storyError = null;
  try {
    const brief = storyBrief(
      ordered,
      { style: comic.style, setting: comic.effectiveSetting, cast: comic.cast },
      previousPageSummaries(),
    );
    const reply = await callExternalLlm(STORY_CHECK_SYSTEM, brief, 800);
    comic.storyIssues = parseStoryIssues(reply, ordered.length);
  } catch (e) {
    comic.storyError = messageOf(e);
  } finally {
    comic.storyChecking = false;
  }
}

/**
 * Ask the Prompt Assistant (the backend Enhance uses) to write panels. On a
 * NovelAI V5 model it writes them by the V5 Enhance rules, with the same
 * working notes, and adds dialogue for NovelAI to letter; on anything else it
 * writes tag prompts.
 */
async function askForPanels(ordered: ComicPanel[], instruction: string): Promise<Map<number, PanelDraft>> {
  const effectiveSetting = comic.effectiveSetting;
  const brief = storyBrief(
    ordered,
    { style: comic.style, setting: effectiveSetting, cast: comic.cast },
    previousPageSummaries(),
  );
  const said = ordered.flatMap((p) => [p.beat, p.prompt, p.dialogue ?? ""]).filter((t) => t.trim());
  const rating = assertAllowedRequest([...said, effectiveSetting, ...comic.cast.map((c) => c.tags)]);
  const variant = generation.isNovelAi ? naiV5Variant(generation.checkpoint) : null;
  const user = instruction ? `${brief}\n\n${instruction}` : brief;
  if (!variant) return parseDraftPrompts(await callExternalLlm(DRAFT_SYSTEM, user, 1500), ordered.length);
  const notes = await promptAssistant.naiWorkingNotes(variant);
  const system = naiSystemWithSkill(
    naiComicSystemPrompt({
      variant,
      rating,
      minorRule: generation.naiEnhanceMinorRule,
      language: resolveNaiLanguage(generation.naiEnhanceLanguage, said.join("\n")),
    }),
    notes,
  );
  const budget = Math.min(8000, Math.max(NAI_MAX_TOKENS, 500 + 500 * ordered.length));
  return parseDraftPrompts(await callExternalLlm(system, user, budget), ordered.length);
}

function applyDraft(panel: ComicPanel, draft: PanelDraft, undo: Record<string, { prompt: string; dialogue: string }>) {
  undo[panel.id] = { prompt: panel.prompt, dialogue: panel.dialogue ?? "" };
  comic.updatePanel(panel.id, {
    prompt: draft.prompt,
    ...(draft.dialogue ? { dialogue: draft.dialogue.join("\n") } : {}),
  });
}

/** Write each panel's prompt (and on V5 its dialogue) from its beat. Panels without a beat keep theirs. */
export async function draftFromBeats(): Promise<void> {
  const ordered = comic.enabledPanels;
  if (!ordered.some((p) => p.beat.trim())) throw new Error(locale.t("comic.error_no_beats"));
  comic.drafting = true;
  comic.storyError = null;
  try {
    const drafts = await askForPanels(ordered, "");
    if (drafts.size === 0) throw new Error(locale.t("comic.error_draft_unreadable"));
    const undo: Record<string, { prompt: string; dialogue: string }> = {};
    ordered.forEach((p, i) => {
      const draft = drafts.get(i + 1);
      if (draft && p.beat.trim()) applyDraft(p, draft, undo);
    });
    comic.draftUndo = undo;
  } finally {
    comic.drafting = false;
  }
}

/** Rewrite one panel's prompt (and on V5 its dialogue) with the rest of the page as context. */
export async function enhancePanel(id: string): Promise<void> {
  const ordered = comic.orderedPanels;
  const index = ordered.findIndex((p) => p.id === id);
  const panel = ordered[index];
  if (!panel || comic.enhancingId) return;
  if (!panel.beat.trim() && !panel.prompt.trim()) throw new Error(locale.t("comic.error_nothing_to_enhance"));
  comic.enhancingId = id;
  try {
    const draft = (await askForPanels(ordered, enhancePanelInstruction(index + 1))).get(index + 1);
    if (!draft) throw new Error(locale.t("comic.error_draft_unreadable"));
    const undo: Record<string, { prompt: string; dialogue: string }> = {};
    applyDraft(comic.panels[id] ?? panel, draft, undo);
    comic.draftUndo = undo;
  } finally {
    comic.enhancingId = null;
  }
}

export function undoDraft() {
  const undo = comic.draftUndo;
  if (!undo) return;
  for (const [id, before] of Object.entries(undo)) comic.updatePanel(id, before);
  comic.draftUndo = null;
}
