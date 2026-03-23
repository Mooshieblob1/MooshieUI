import { generation } from "../stores/generation.svelte.js";
import { readImageMetadataBytes } from "./api.js";
import { gallery } from "../stores/gallery.svelte.js";

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

/** Quality tags that are auto-applied by the app — strip these from imported prompts to avoid duplication. */
const AUTO_QUALITY_TAGS = new Set([
  // Anima positive
  "year 2025", "newest", "masterpiece", "best quality", "score_9", "score_8", "safe", "highres",
  // Anima negative
  "worst quality", "low quality", "score_1", "score_2", "score_3", "blurry", "jpeg artifacts", "sepia",
  // Illustrious positive
  "very aesthetic", "year 2024", "absurdres",
  // Illustrious negative
  "bad quality", "lowres", "artistic error", "bad anatomy", "extra fingers", "text",
  "signature", "watermark", "long body", "bad hands", "cropped", "username",
]);

/** Remove auto-applied quality tags from a prompt string. */
function stripQualityTags(prompt: string): string {
  const tags = prompt.split(",").map((t) => t.trim()).filter(Boolean);
  const filtered = tags.filter((t) => !AUTO_QUALITY_TAGS.has(t.toLowerCase()));
  return filtered.join(", ");
}

function applyPrompts(meta: Record<string, string>): boolean {
  let applied = false;
  if (meta.positive_prompt !== undefined) {
    generation.positivePrompt = stripQualityTags(meta.positive_prompt);
    applied = true;
  }
  if (meta.negative_prompt !== undefined) {
    generation.negativePrompt = stripQualityTags(meta.negative_prompt);
    applied = true;
  }
  return applied;
}

function applySampler(meta: Record<string, string>): boolean {
  let applied = false;
  if (meta.sampler) { generation.samplerName = meta.sampler; applied = true; }
  if (meta.scheduler) { generation.scheduler = meta.scheduler; applied = true; }
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
  if (meta.seed) {
    const v = parseInt(meta.seed, 10);
    if (!isNaN(v)) { generation.seed = v; applied = true; }
  }
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
  if (meta.upscale_model) { generation.upscaleModel = meta.upscale_model; applied = true; }
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

/** Extract PNG bytes from a File or DataTransferItem. */
async function fileToPngBytes(file: File): Promise<number[]> {
  const buffer = await file.arrayBuffer();
  return Array.from(new Uint8Array(buffer));
}

/** Extract image file from a DragEvent's dataTransfer. */
function getImageFile(dt: DataTransfer): File | null {
  for (const file of Array.from(dt.files)) {
    if (file.type.startsWith("image/")) return file;
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
  gallery.showToast("Reading image metadata...", "info");
  try {
    const bytes = await fileToPngBytes(file);
    const meta = await readImageMetadataBytes(bytes);

    if (!meta || Object.keys(meta).length === 0) {
      gallery.showToast("No metadata found in image", "info");
      return;
    }

    if (target === "all") {
      const applied = applyAllMetadata(meta);
      if (applied.length > 0) {
        gallery.showToast(`Applied ${applied.join(", ")} from image metadata`, "success");
        generation.saveSettings();
      } else {
        gallery.showToast("No applicable parameters found in image metadata", "info");
      }
    } else {
      const applied = applyMetadataToSection(meta, target);
      if (applied) {
        gallery.showToast(`Applied ${sectionLabel(target)} from image metadata`, "success");
        generation.saveSettings();
      } else {
        gallery.showToast(`No ${sectionLabel(target)} found in image metadata`, "info");
      }
    }
  } catch (err) {
    console.error("Metadata import failed:", err);
    gallery.showToast("Failed to read image metadata", "error");
  }
}

export { getImageFile, getClipboardImageFile };
