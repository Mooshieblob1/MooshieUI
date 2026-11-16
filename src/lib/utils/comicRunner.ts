// Comic page builder: running it. Generates panels through the normal queue,
// checks continuity with the tagger, asks the Prompt Assistant about the story,
// and assembles the page. State lives in stores/comic.svelte.ts.

import { autocomplete } from "../stores/autocomplete.svelte.js";
import { comic } from "../stores/comic.svelte.js";
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
  saveToGalleryBytes,
} from "./api.js";
import {
  DRAFT_SYSTEM,
  ONE_SHOT_MAX_PANELS,
  dialogueLines,
  enhancePanelInstruction,
  STORY_CHECK_SYSTEM,
  continuityIssues,
  coverCrop,
  joinTags,
  oneShotBasePrompt,
  oneShotCentre,
  oneShotPanelPrompt,
  pageAspect,
  pageSize,
  panelGenerationSize,
  panelPixelRect,
  panelPrompt,
  panelSetting,
  parseDraftPrompts,
  parseStoryIssues,
  splitTags,
  storyBrief,
  type ComicPanel,
  type PanelDraft,
} from "./comicLayout.js";
import { assertAllowedRequest } from "./contentSafety.js";
import { resolveNaiLanguage } from "./naiLanguage.js";
import { NAI_MAX_TOKENS, naiComicSystemPrompt } from "./naiPrompt.js";
import { naiSystemWithSkill } from "./naiSkill.js";
import { naiV5Variant } from "./novelaiModels.js";
import { loadOutputImageForGenerationInput } from "./galleryActions.js";
import { submitGeneration } from "./generationSubmit.js";

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
  if (generation.mode === "video") throw new Error(locale.t("comic.error_video_mode"));
  if (!generation.checkpoint) throw new Error(locale.t("generation.error_no_checkpoint"));
}

// ---------------------------------------------------------------------------
// Per-panel generation
// ---------------------------------------------------------------------------

async function generateOne(panel: ComicPanel): Promise<void> {
  const rect = comic.rects.get(panel.id);
  if (!rect) return;
  const page = pageSize(comic.pageShape);
  const size = panelGenerationSize(rect, page, { width: generation.width, height: generation.height });
  // NovelAI letters the panel's dialogue from a Text: block; other models get none.
  const positive = panelPrompt(panel, comic, generation.isNovelAi);
  if (!positive) throw new Error(locale.t("comic.error_empty_prompt"));
  const params = baseParams(positive, panel.negative.trim(), size.width, size.height);
  if (params.novelai) {
    params.novelai = { ...params.novelai, characters: [], use_coords: false, vibes: [], director_references: [] };
  }
  comic.setResult(panel.id, { status: "running", error: undefined, issues: null, checkError: undefined });
  const promptId = await submitGeneration(params);
  const image = await waitForImage(promptId);
  if (!image) {
    comic.setResult(panel.id, { status: "queued" });
    return;
  }
  const { png, galleryFilename } = await imageBytes(image);
  comic.setResult(panel.id, { status: "done", url: objectUrl(png), galleryFilename });
  if (comic.checkContinuity) void checkPanel(panel.id, png, galleryFilename);
}

/** Generate the given panels (default: every enabled panel), one after another. */
export async function generatePanels(ids?: string[]): Promise<void> {
  if (comic.running) return;
  checkGenerationReady();
  const targets = (ids ? comic.orderedPanels.filter((p) => ids.includes(p.id)) : comic.enabledPanels).filter(
    (p) => p.enabled || ids,
  );
  if (targets.length === 0) return;
  comic.running = true;
  comic.stopRequested = false;
  for (const p of targets) comic.setResult(p.id, { status: "queued", error: undefined });
  try {
    for (const panel of targets) {
      if (comic.stopRequested) break;
      try {
        await generateOne(panel);
      } catch (e) {
        comic.setResult(panel.id, { status: "failed", error: messageOf(e) });
      }
    }
  } finally {
    // Panels still marked queued after a stop go back to their last state.
    for (const p of targets) {
      const r = comic.results[p.id];
      if (r?.status === "queued" && !r.url) {
        const { [p.id]: _drop, ...rest } = comic.results;
        comic.results = rest;
      } else if (r?.status === "queued") comic.setResult(p.id, { status: "done" });
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

export async function checkPanel(id: string, png?: Uint8Array, galleryFilename?: string | null): Promise<void> {
  const panel = comic.panels[id];
  const result = comic.results[id];
  if (!panel || !result) return;
  comic.setResult(id, { checking: true, checkError: undefined });
  try {
    const name = galleryFilename ?? result.galleryFilename ?? null;
    let detected: TagResult[] = [];
    let bytes = png;
    if (!bytes && !name && result.url) bytes = new Uint8Array(await (await fetch(result.url)).arrayBuffer());
    const res = name ? await interrogateGalleryImage(name) : await interrogateImage(bytesToBase64(bytes ?? new Uint8Array()));
    detected = [...res.general_tags, ...res.character_tags];
    const expected = [
      ...comic.cast
        .filter((c) => panel.cast.includes(c.id))
        .map((c) => ({ who: c.name || "?", tags: checkable(c.tags) })),
      { who: null, tags: checkable(panelSetting(panel, comic.setting)) },
    ].filter((g) => g.tags.length > 0);
    comic.setResult(id, { checking: false, issues: continuityIssues(expected, detected.map((t) => t.name)) });
  } catch (e) {
    comic.setResult(id, { checking: false, checkError: messageOf(e) });
  }
}

// ---------------------------------------------------------------------------
// One image (NovelAI): every panel as a positioned character prompt
// ---------------------------------------------------------------------------

export async function generateOneImage(): Promise<void> {
  if (comic.running) return;
  checkGenerationReady();
  if (!generation.isNovelAi) throw new Error(locale.t("comic.error_one_image_novelai"));
  const panels = comic.enabledPanels;
  if (panels.length === 0) return;
  if (panels.length > ONE_SHOT_MAX_PANELS) {
    throw new Error(locale.t("comic.error_one_image_panels", { max: String(ONE_SHOT_MAX_PANELS) }));
  }
  const aspect = pageAspect(comic.pageShape);
  const pixels = generation.width * generation.height;
  const snap = (v: number) => Math.max(256, Math.round(v / 64) * 64);
  const width = snap(Math.sqrt(pixels * aspect));
  const height = snap(Math.sqrt(pixels / aspect));

  const dialogue = panels.flatMap((p) => dialogueLines(p.dialogue ?? ""));
  const params = baseParams(oneShotBasePrompt(comic, panels.length, dialogue), "", width, height);
  if (!params.novelai) throw new Error(locale.t("comic.error_one_image_novelai"));
  params.novelai = {
    ...params.novelai,
    use_coords: true,
    vibes: [],
    director_references: [],
    characters: panels.map((p) => ({
      prompt: oneShotPanelPrompt(p, comic.numberOf(p.id), comic),
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
    comic.setPage(objectUrl(png), galleryFilename);
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
// Assembling and saving the page
// ---------------------------------------------------------------------------

async function loadBitmap(url: string): Promise<ImageBitmap> {
  const blob = await (await fetch(url)).blob();
  return createImageBitmap(blob);
}

/** Draw the finished panels onto one page. Panels without an image stay blank. */
export async function assemblePage(): Promise<Blob> {
  const page = pageSize(comic.pageShape);
  const canvas = document.createElement("canvas");
  canvas.width = page.width;
  canvas.height = page.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("Canvas is not available");
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, page.width, page.height);
  const border = Math.max(2, Math.round(Math.max(page.width, page.height) / 600));
  for (const [id, rect] of comic.rects) {
    const panel = comic.panels[id];
    const box = panelPixelRect(rect, page, comic.gutter);
    const url = comic.results[id]?.url;
    if (panel?.enabled && url) {
      const bitmap = await loadBitmap(url);
      const crop = coverCrop(bitmap, box);
      ctx.drawImage(bitmap, crop.x, crop.y, crop.w, crop.h, box.x, box.y, box.w, box.h);
      bitmap.close();
    }
    ctx.strokeStyle = "#000000";
    ctx.lineWidth = border;
    ctx.strokeRect(box.x + border / 2, box.y + border / 2, box.w - border, box.h - border);
  }
  return await new Promise<Blob>((resolve, reject) =>
    canvas.toBlob((b) => (b ? resolve(b) : reject(new Error("Could not encode the page"))), "image/png"),
  );
}

export async function previewPage(): Promise<void> {
  const blob = await assemblePage();
  comic.setPage(URL.createObjectURL(blob), null);
  comic.pageStatus = "done";
}

/** Save the page to the gallery: the assembled page, or the one-image page as it is. */
export async function savePage(): Promise<string> {
  let bytes: Uint8Array;
  if (comic.mode === "one_image") {
    if (!comic.pageUrl) throw new Error(locale.t("comic.error_nothing_to_save"));
    if (comic.pageGalleryFilename) return comic.pageGalleryFilename;
    bytes = new Uint8Array(await (await fetch(comic.pageUrl)).arrayBuffer());
  } else {
    if (!comic.orderedPanels.some((p) => comic.results[p.id]?.url)) throw new Error(locale.t("comic.error_nothing_to_save"));
    const blob = await assemblePage();
    bytes = new Uint8Array(await blob.arrayBuffer());
    comic.setPage(URL.createObjectURL(blob), null);
  }
  const summary = comic.enabledPanels
    .map((p, i) => `Panel ${i + 1}: ${p.beat.trim() || p.prompt.trim()}`)
    .join("\n");
  const saved = await saveToGalleryBytes(
    Array.from(bytes),
    "comic_page.png",
    `comic${Date.now().toString(36)}`,
    "txt2img",
    { positive_prompt: joinTags(comic.style, comic.setting), comic_panels: summary },
  );
  await gallery.addPersistedImage(saved, undefined, true);
  comic.pageGalleryFilename = saved;
  comic.persistSoon();
  return saved;
}

/** Bring back panel images saved before a restart. */
export async function restoreImages(): Promise<void> {
  const entries = Object.entries(comic.savedImages);
  comic.savedImages = {};
  for (const [id, filename] of entries) {
    if (!comic.panels[id] || comic.results[id]?.url) continue;
    try {
      const bytes = await loadGalleryImageDisplay(filename);
      const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "image/webp" }));
      comic.setResult(id, { status: "done", url, galleryFilename: filename, issues: null });
    } catch {
      /* the file was deleted from the gallery: the panel just starts empty */
    }
  }
  const pageFile = comic.savedPageImage;
  comic.savedPageImage = null;
  if (pageFile && !comic.pageUrl) {
    try {
      const bytes = await loadGalleryImageDisplay(pageFile);
      comic.setPage(URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "image/webp" })), pageFile);
      comic.pageStatus = "done";
    } catch {
      /* gone */
    }
  }
}

// ---------------------------------------------------------------------------
// Story: Prompt Assistant
// ---------------------------------------------------------------------------

export async function checkStory(): Promise<void> {
  const ordered = comic.enabledPanels;
  if (ordered.length === 0) return;
  comic.storyChecking = true;
  comic.storyError = null;
  try {
    const reply = await callExternalLlm(STORY_CHECK_SYSTEM, storyBrief(ordered, comic), 800);
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
  const brief = storyBrief(ordered, comic);
  const said = ordered.flatMap((p) => [p.beat, p.prompt, p.dialogue ?? ""]).filter((t) => t.trim());
  const rating = assertAllowedRequest([...said, comic.setting, ...comic.cast.map((c) => c.tags)]);
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
