import { generation } from "../stores/generation.svelte.js";
import type { NovelAiSettings } from "../stores/generation.svelte.js";
import { readImageMetadataBytes, readImageMetadataPath } from "./api.js";
import { gallery } from "../stores/gallery.svelte.js";
import { locale } from "../stores/locale.svelte.js";
import { readPngMetadataClientSide } from "./pngMetadata.js";
import { isBrowserMode } from "./ipc.js";
import { parseSegmentDetailPrompt } from "./promptSegmentDetail.js";
import type { NovelAiCharacter } from "../types/index.js";
import { NOVELAI_MAX_CHARACTERS, isNovelAiModel } from "./novelaiModels.js";
import { novelaiImport } from "../stores/novelaiImport.svelte.js";
import type {
  NovelAiImportSelection,
  NovelAiImportSource,
} from "../stores/novelaiImport.svelte.js";

/** The newest NovelAI model that still has vibe transfer and precise reference. */
export const NOVELAI_REFERENCE_FALLBACK_MODEL = "nai-diffusion-4-5-full";

/** Section IDs that accept metadata drops */
export type DroppableSectionId =
  | "prompts"
  | "sampler"
  | "dimensions"
  | "model"
  | "upscaleHistory"
  | "facefix";

const DROPPABLE_SECTIONS = new Set<string>([
  "prompts", "sampler", "dimensions", "model", "upscaleHistory", "facefix",
]);

export function isDroppableSection(sectionId: string): boolean {
  return DROPPABLE_SECTIONS.has(sectionId);
}

/** Human-readable label for what was imported */
function sectionLabel(sectionId: DroppableSectionId | "all"): string {
  switch (sectionId) {
    case "prompts": return "prompts";
    case "sampler": return "sampler settings";
    case "dimensions": return "dimensions";
    case "model": return "model settings";
    case "upscaleHistory": return "upscale settings";
    case "facefix": return "face fix settings";
    case "all": return "all parameters";
  }
}

/** Build a set of quality tags from the user's custom quality tag settings.
 *  This ensures imported prompts have the user's current custom tags stripped to avoid duplication. */
function buildAutoQualityTagSet(): Set<string> {
  const all = [
    generation.customAnimaPositiveQuality,
    generation.customAnimaNegativeQuality,
    generation.customIllustriousPositiveQuality,
    generation.customIllustriousNegativeQuality,
  ].join(", ");
  return new Set(
    all.split(",").map((t) => t.trim().toLowerCase()).filter(Boolean)
  );
}

/**
 * Strip unsupported SwarmUI inline syntax tags from a prompt string.
 * Matches patterns like `<random:...>`, `<preset:...>`, `<wildcard:...>`, etc.
 * LoRA tags `<lora:name:strength>` are also stripped since MooshieUI handles LoRAs separately.
 * Handles URL-encoded values and nested `//` parameters.
 */
const SWARMUI_TAG_RE = /<[a-zA-Z_-]+:[^>]*>/g;

/**
 * Inline syntaxes MooshieUI supports natively — preserved on import so they
 * keep working (`<segment:...>` refinement, `<from/to/range:...>` scheduling,
 * `<region:...>` regional prompts). `<fromto[...]:...>` never matched the
 * strip regex (the `[` breaks the name pattern) and survives on its own.
 */
const SUPPORTED_TAG_RE = /^<(?:segment|from|to|range|region):/i;

function stripSwarmUITags(prompt: string): string {
  return prompt
    .replace(SWARMUI_TAG_RE, (tag) => (SUPPORTED_TAG_RE.test(tag) ? tag : ""))
    .replace(/,\s*,/g, ",")
    .replace(/^\s*,\s*/, "")
    .replace(/\s*,\s*$/, "")
    .trim();
}

function appendCommaSeparatedPromptParts(text: string, parts: string[]): void {
  for (const part of text.split(",")) {
    const trimmed = part.trim();
    if (trimmed) parts.push(trimmed);
  }
}

function splitQualityTagCandidates(prompt: string): string[] {
  const ranges = parseSegmentDetailPrompt(prompt).ranges;
  if (ranges.length === 0) {
    return prompt.split(",").map((t) => t.trim()).filter(Boolean);
  }

  const parts: string[] = [];
  let cursor = 0;
  for (const range of ranges) {
    appendCommaSeparatedPromptParts(prompt.slice(cursor, range.start), parts);
    const segment = prompt.slice(range.start, range.end).trim();
    if (segment) parts.push(segment);
    cursor = range.end;
  }
  appendCommaSeparatedPromptParts(prompt.slice(cursor), parts);
  return parts;
}

/** Remove auto-applied quality tags and SwarmUI syntax from a prompt string. */
function stripQualityTags(prompt: string): string {
  const autoTags = buildAutoQualityTagSet();
  const cleaned = stripSwarmUITags(prompt);
  const tags = splitQualityTagCandidates(cleaned);
  const filtered = tags.filter((t) => !autoTags.has(t.toLowerCase()));
  return filtered.join(", ");
}

/**
 * The prompt text an import writes into the panel.
 *
 * `clean` is the import dialog's "Clean Imports" switch, and matches what every
 * other import path has always done: drop the quality tags this app appends on
 * its own so they are not baked in twice, and drop inline syntax no backend
 * here understands. Turning it off imports the prompt exactly as the image
 * carries it.
 */
function promptForImport(text: string, clean: boolean): string {
  return clean ? stripQualityTags(text) : text.trim();
}

function applyPositivePrompt(meta: Record<string, string>, clean = true): boolean {
  if (meta.positive_prompt === undefined) return false;
  generation.positivePrompt = promptForImport(meta.positive_prompt, clean);
  generation.extraPositiveBoxes = [];
  return true;
}

function applyNegativePrompt(meta: Record<string, string>, clean = true): boolean {
  if (meta.negative_prompt === undefined) return false;
  generation.negativePrompt = promptForImport(meta.negative_prompt, clean);
  generation.extraNegativeBoxes = [];
  return true;
}

function applyPrompts(meta: Record<string, string>): boolean {
  // Both sides are applied: `||` would short-circuit past the negative.
  const positive = applyPositivePrompt(meta);
  const negative = applyNegativePrompt(meta);
  return positive || negative;
}

/**
 * Was this image made by NovelAI?
 *
 * Set by our own writer for anything generated against the NovelAI backend,
 * and by the Rust reader for an image copied straight off novelai.net, so both
 * routes into the panel land here.
 */
export function isNovelAiMetadata(meta: Record<string, string>): boolean {
  return meta.mooshie_backend === "novelai";
}

/** Local alias, so the call sites inside this module read as they did. */
const isNovelAiMeta = isNovelAiMetadata;

/** Does this image carry a character list the Characters box could import? */
export function hasNovelAiCharacters(meta: Record<string, string>): boolean {
  return parseNovelAiCharacters(meta).length > 0;
}

/**
 * Was this image made by NovelAI's img2img endpoint?
 *
 * The metadata records the endpoint but not the source image, so nothing here
 * can reproduce the result: importing the settings starts a fresh generation
 * from whatever the panel is holding. The dialog warns about that.
 */
export function isNovelAiImg2ImgMetadata(meta: Record<string, string>): boolean {
  return (meta.mooshie_novelai_request_type ?? "").toLowerCase().includes("img2img");
}

function metaBool(value: string | undefined): boolean | undefined {
  if (value === undefined) return undefined;
  return value.trim().toLowerCase() === "true";
}

function metaNumber(value: string | undefined): number | undefined {
  if (value === undefined) return undefined;
  const parsed = parseFloat(value);
  return isNaN(parsed) ? undefined : parsed;
}

/**
 * Restore the NovelAI half of an image's settings.
 *
 * NovelAI's sampler and noise schedule are not the top-level `samplerName` and
 * `scheduler`: those stay ComfyUI values for the local post-process pass, and
 * writing a NovelAI sampler into them would leave the local pass asking ComfyUI
 * for a sampler it does not have. Everything NovelAI-specific goes through the
 * one settings patch instead, which persists on write.
 */
function applyNovelAiSettings(meta: Record<string, string>, withCharacters = true): void {
  const patch: Partial<NovelAiSettings> = {};
  if (meta.sampler) patch.sampler = meta.sampler;
  if (meta.scheduler) patch.noise_schedule = meta.scheduler;

  const numbers: [string, keyof NovelAiSettings][] = [
    ["mooshie_novelai_cfg_rescale", "cfg_rescale"],
    ["mooshie_novelai_uncond_scale", "uncond_scale"],
    ["mooshie_novelai_uc_preset", "uc_preset"],
    ["mooshie_novelai_strength", "strength"],
    ["mooshie_novelai_noise", "noise"],
  ];
  for (const [key, field] of numbers) {
    const value = metaNumber(meta[key]);
    if (value !== undefined) (patch as Record<string, unknown>)[field] = value;
  }

  const booleans: [string, keyof NovelAiSettings][] = [
    ["mooshie_novelai_dynamic_thresholding", "dynamic_thresholding"],
    ["mooshie_novelai_variety_plus", "variety_plus"],
    ["mooshie_novelai_transparent_background", "transparent_background"],
    ["mooshie_novelai_use_coords", "use_coords"],
    ["mooshie_novelai_quality_toggle", "quality_toggle"],
    ["mooshie_novelai_legacy_uc", "legacy_uc"],
  ];
  for (const [key, field] of booleans) {
    const value = metaBool(meta[key]);
    if (value !== undefined) (patch as Record<string, unknown>)[field] = value;
  }

  if (withCharacters) {
    const characters = parseNovelAiCharacters(meta);
    if (characters.length > 0) patch.characters = characters;
  }

  generation.updateNovelAiSettings(patch);
}

/** Read the character list an image carries, or an empty list if it has none. */
function parseNovelAiCharacters(meta: Record<string, string>): NovelAiCharacter[] {
  if (!meta.mooshie_novelai_characters) return [];
  try {
    const parsed = JSON.parse(meta.mooshie_novelai_characters);
    if (!Array.isArray(parsed)) return [];
    return parsed.map((c: any) => ({
      prompt: typeof c?.prompt === "string" ? c.prompt : "",
      negative_prompt: typeof c?.negative_prompt === "string" ? c.negative_prompt : "",
      center: {
        x: typeof c?.center?.x === "number" ? c.center.x : 0.5,
        y: typeof c?.center?.y === "number" ? c.center.y : 0.5,
      },
      enabled: c?.enabled !== false,
    }));
  } catch {
    // A character list we cannot read is dropped; the rest still applies.
    return [];
  }
}

/**
 * Import an image's characters, either on top of the panel's own or in place of
 * them.
 *
 * Appending is capped the same way the character panel caps itself, so a busy
 * image dropped onto a busy panel cannot push the list past what the backend
 * accepts.
 */
function applyNovelAiCharacters(meta: Record<string, string>, append: boolean): boolean {
  const incoming = parseNovelAiCharacters(meta);
  if (incoming.length === 0) return false;
  const existing = append ? (generation.novelaiSettings.characters ?? []) : [];
  generation.updateNovelAiSettings({
    characters: [...existing, ...incoming].slice(0, NOVELAI_MAX_CHARACTERS),
  });
  return true;
}

/**
 * Empty the character panel.
 *
 * An image with no characters of its own writes nothing, so the panel keeps
 * whoever was already in it. This is how a drop says "this image has no
 * characters, and neither should the panel".
 */
function clearNovelAiCharacters(): boolean {
  if ((generation.novelaiSettings.characters ?? []).length === 0) return false;
  generation.updateNovelAiSettings({ characters: [] });
  return true;
}

/**
 * Restore the seed.
 *
 * Kept as a string all the way through: `parseInt` rounds the 63-bit seeds
 * NovelAI hands out past 2^53, which silently produces a different image.
 */
function applySeed(meta: Record<string, string>): boolean {
  if (!meta.seed) return false;
  const trimmed = meta.seed.trim();
  if (!/^\d+$/.test(trimmed)) return false;
  generation.seed = trimmed;
  return true;
}

function applySampler(
  meta: Record<string, string>,
  options: { seed?: boolean; characters?: boolean } = {}
): boolean {
  const { seed = true, characters = true } = options;
  let applied = false;
  if (isNovelAiMeta(meta)) {
    applyNovelAiSettings(meta, characters);
    applied = true;
  } else {
    if (meta.sampler) { generation.samplerName = meta.sampler; applied = true; }
    if (meta.scheduler) { generation.scheduler = meta.scheduler; applied = true; }
  }
  if (meta.steps) {
    const v = parseInt(meta.steps, 10);
    if (!isNaN(v)) { generation.steps = v; applied = true; }
  }
  if (meta.cfg) {
    const v = parseFloat(meta.cfg);
    if (!isNaN(v)) { generation.cfg = v; applied = true; }
  }
  if (meta.denoise) {
    const v = parseFloat(meta.denoise);
    if (!isNaN(v)) { generation.denoise = v; applied = true; }
  }
  if (seed && applySeed(meta)) applied = true;
  return applied;
}

function applyDimensions(meta: Record<string, string>): boolean {
  if (!meta.size) return false;
  const match = meta.size.match(/^(\d+)x(\d+)$/);
  if (!match) return false;
  const w = parseInt(match[1], 10);
  const h = parseInt(match[2], 10);
  if (isNaN(w) || isNaN(h)) return false;
  generation.width = w;
  generation.height = h;
  return true;
}

function applyModel(meta: Record<string, string>): boolean {
  let applied = false;
  if (meta.model) { generation.checkpoint = meta.model; applied = true; }
  if (meta.vae) { generation.vae = meta.vae; applied = true; }
  if (meta.loras) {
    try {
      const raw = meta.loras.trim();
      if (raw.startsWith("[")) {
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed)) {
          generation.loras = parsed.map((l: any) => ({
            name: l.name || "",
            strength_model: l.strength_model ?? 1.0,
            strength_clip: l.strength_clip ?? 1.0,
            enabled: true,
          }));
          applied = true;
        }
      } else if (raw) {
        const entries = raw.split(",").map((s) => s.trim()).filter(Boolean);
        generation.loras = entries.map((entry) => {
          const [name, str] = entry.split(":");
          const strength = parseFloat(str) || 1.0;
          return { name: name.trim(), strength_model: strength, strength_clip: strength, enabled: true };
        });
        applied = true;
      }
    } catch {
      // Ignore parse errors for loras
    }
  }
  return applied;
}

function applyUpscale(meta: Record<string, string>): boolean {
  let applied = false;
  
  if (meta.upscale_model) { 
    generation.upscaleModel = meta.upscale_model; 
    applied = true;
    
    // Auto-detect scale from model name (e.g., "OmniSR_X4_DIV2K" → 4x)
    const match = meta.upscale_model.match(/_X(\d+)[_\.]/i) || meta.upscale_model.match(/[_-](\d+)x[_\.]/i);
    if (match) {
      generation.upscaleScale = parseInt(match[1], 10);
    }
  }
  
  if (meta.upscale_scale) {
    const v = parseFloat(meta.upscale_scale);
    if (!isNaN(v)) { generation.upscaleScale = v; applied = true; }
  }
  if (meta.upscale_denoise) {
    const v = parseFloat(meta.upscale_denoise);
    if (!isNaN(v)) { generation.upscaleDenoise = v; applied = true; }
  }
  return applied;
}

/** Apply metadata for a specific section. Returns true if any values were applied. */
export function applyMetadataToSection(
  meta: Record<string, string>,
  sectionId: DroppableSectionId
): boolean {
  switch (sectionId) {
    case "prompts": return applyPrompts(meta);
    case "sampler": return applySampler(meta);
    case "dimensions": return applyDimensions(meta);
    case "model": return applyModel(meta);
    case "upscaleHistory": return applyUpscale(meta);
    case "facefix": return false;
  }
}

/** Apply all applicable metadata. Returns list of section names that were applied. */
export function applyAllMetadata(meta: Record<string, string>): string[] {
  const applied: string[] = [];
  if (applyPrompts(meta)) applied.push("prompts");
  if (applySampler(meta)) applied.push("sampler");
  if (applyDimensions(meta)) applied.push("dimensions");
  if (applyModel(meta)) applied.push("model");
  if (applyUpscale(meta)) applied.push("upscale");
  return applied;
}

/**
 * Point the panel at the model that made the image.
 *
 * Coming from a local checkpoint this has to go through `selectNovelAiModel`,
 * which clears the split-model and LoRA state a local checkpoint leaves behind.
 * That also resets steps and CFG to NovelAI's defaults, so it runs before the
 * rest of the import and the image's own values land on top.
 */
function applyNovelAiModel(meta: Record<string, string>): boolean {
  const model = meta.model;
  if (!model || !isNovelAiModel(model)) return false;
  if (generation.checkpoint === model) return false;
  if (isNovelAiModel(generation.checkpoint)) {
    generation.checkpoint = model;
  } else {
    generation.selectNovelAiModel(model);
  }
  return true;
}

/**
 * Apply exactly what the import dialog's checkboxes asked for.
 *
 * Returns the list of section names that changed, for the confirmation toast.
 * The order matters: the model switch resets steps and CFG, so it goes first,
 * and characters are written after the settings patch that would otherwise
 * carry its own copy of them.
 */
export function applyNovelAiSelection(
  meta: Record<string, string>,
  selection: NovelAiImportSelection,
): string[] {
  const applied: string[] = [];

  if (selection.settings) {
    let settingsApplied = applyNovelAiModel(meta);
    if (applyDimensions(meta)) settingsApplied = true;
    // Characters and the seed are their own checkboxes, so they are held back
    // from the settings pass whatever it would otherwise have written.
    if (applySampler(meta, { seed: false, characters: false })) settingsApplied = true;
    if (settingsApplied) applied.push("settings");
  }

  // Clearing runs first so ticking it alongside Append still ends up as a
  // replace rather than an append onto stale characters.
  let charactersChanged = false;
  if (selection.clearCharacters && clearNovelAiCharacters()) charactersChanged = true;
  if (selection.characters && applyNovelAiCharacters(meta, selection.appendCharacters)) {
    charactersChanged = true;
  }
  if (charactersChanged) applied.push("characters");
  if (selection.prompt && applyPositivePrompt(meta, selection.clean)) applied.push("prompt");
  if (selection.undesired && applyNegativePrompt(meta, selection.clean)) {
    applied.push("undesired");
  }
  if (selection.seed && applySeed(meta)) applied.push("seed");

  if (applied.length > 0) generation.saveSettings();
  return applied;
}

/**
 * Switch to a model that supports vibe transfer and precise reference.
 *
 * V5 dropped both, so picking either from the dialog moves the panel to V4.5
 * Full, which is the newest model that still has them. A model that already
 * supports the feature is left alone.
 */
export function ensureNovelAiReferenceModel(kind: "vibe" | "precise"): boolean {
  const supported =
    kind === "vibe"
      ? generation.supportsNovelAiVibeTransfer
      : generation.supportsNovelAiPreciseReference;
  if (supported) return false;
  if (isNovelAiModel(generation.checkpoint)) {
    generation.checkpoint = NOVELAI_REFERENCE_FALLBACK_MODEL;
    generation.saveSettings();
  } else {
    generation.selectNovelAiModel(NOVELAI_REFERENCE_FALLBACK_MODEL);
  }
  return true;
}

/** Extract PNG bytes from a File or DataTransferItem. */
async function fileToPngBytes(file: File): Promise<number[]> {
  const buffer = await file.arrayBuffer();
  return Array.from(new Uint8Array(buffer));
}

function isImageFile(file: File): boolean {
  if (file.type && file.type.startsWith("image/")) return true;
  // mp4 and avif carry generation parameters too, and neither reports an
  // image/* MIME type: mp4 is video/mp4 and avif is often reported as empty.
  return /\.(png|jpe?g|webp|bmp|gif|mp4|avif)$/i.test(file.name);
}

/** Extract image file from a DragEvent's dataTransfer. */
function getImageFile(dt: DataTransfer): File | null {
  for (const file of Array.from(dt.files)) {
    if (isImageFile(file)) return file;
  }

  for (const item of Array.from(dt.items || [])) {
    if (item.kind !== "file") continue;
    const file = item.getAsFile();
    if (file && isImageFile(file)) return file;
  }
  return null;
}

/** Extract image file from a ClipboardEvent's clipboardData. */
function getClipboardImageFile(e: ClipboardEvent): File | null {
  if (!e.clipboardData) return null;
  for (const item of Array.from(e.clipboardData.items)) {
    if (item.type.startsWith("image/")) {
      const file = item.getAsFile();
      if (file) return file;
    }
  }
  return null;
}

/**
 * Handle a metadata import from a dropped/pasted image file.
 * @param file The image file
 * @param target "all" for preview area, or a DroppableSectionId
 */
export async function handleMetadataImport(
  file: File,
  target: DroppableSectionId | "all"
): Promise<void> {
  try {
    if (isBrowserMode) {
      // Client-side: read metadata directly from the file without server round-trip
      const buf = await file.arrayBuffer();
      const meta = await readPngMetadataClientSide(buf);
      applyParsedMetadata(meta, target, { kind: "file", file });
    } else {
      const bytes = await fileToPngBytes(file);
      await handleMetadataImportBytes(bytes, target, { kind: "file", file });
    }
  } catch (err) {
    console.error("Metadata import failed:", err);
    gallery.showToast(locale.t("metadata.toast.read_failed"), "error");
  }
}

/**
 * Apply parsed metadata to the appropriate section(s) and show toast feedback.
 *
 * A NovelAI image is the one exception: nothing is applied, and the import
 * dialog opens instead so the user picks what to take. Section targeting is
 * dropped along with it, because the dialog covers every section a drop could
 * have aimed at.
 */
function applyParsedMetadata(
  meta: Record<string, string> | null,
  target: DroppableSectionId | "all",
  source: NovelAiImportSource
): void {
  if (!meta || Object.keys(meta).length === 0) {
    gallery.showToast(locale.t("metadata.toast.no_metadata"), "info");
    return;
  }

  if (isNovelAiMetadata(meta)) {
    novelaiImport.open(meta, source);
    return;
  }

  if (target === "all") {
    const applied = applyAllMetadata(meta);
    if (applied.length > 0) {
      gallery.showToast(locale.t("metadata.toast.applied_all", { fields: applied.join(", ") }), "success");
      generation.saveSettings();
    } else {
      gallery.showToast(locale.t("metadata.toast.no_applicable"), "info");
    }
  } else {
    const applied = applyMetadataToSection(meta, target);
    if (applied) {
      gallery.showToast(locale.t("metadata.toast.applied_section", { section: sectionLabel(target) }), "success");
      generation.saveSettings();
    } else {
      gallery.showToast(locale.t("metadata.toast.no_section", { section: sectionLabel(target) }), "info");
    }
  }
}

/**
 * Handle a metadata import from raw image bytes (e.g. from Tauri file read).
 * @param bytes The image file bytes as number[]
 * @param target "all" for preview area, or a DroppableSectionId
 */
export async function handleMetadataImportBytes(
  bytes: number[],
  target: DroppableSectionId | "all",
  source?: NovelAiImportSource
): Promise<void> {
  gallery.showToast(locale.t("metadata.toast.reading"), "info");
  try {
    const meta = await readImageMetadataBytes(bytes);
    applyParsedMetadata(meta, target, source ?? { kind: "bytes", bytes, filename: "image.png" });
  } catch (err) {
    console.error("Metadata import failed:", err);
    gallery.showToast(locale.t("metadata.toast.read_failed"), "error");
  }
}

/**
 * Handle a metadata import from an OS file path (native drops).
 * Sends only the path string over IPC — Rust reads the file directly from disk.
 */
export async function handleMetadataImportPath(
  filePath: string,
  target: DroppableSectionId | "all"
): Promise<void> {
  gallery.showToast(locale.t("metadata.toast.reading"), "info");
  try {
    const meta = await readImageMetadataPath(filePath);
    applyParsedMetadata(meta, target, { kind: "path", path: filePath });
  } catch (err) {
    console.error("Metadata import failed:", err);
    gallery.showToast(locale.t("metadata.toast.read_failed"), "error");
  }
}

export { getImageFile, getClipboardImageFile };
