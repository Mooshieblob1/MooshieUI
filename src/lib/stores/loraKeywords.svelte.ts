import { getLoraCivitaiInfo, type LoraCivitaiInfo } from "../utils/api.js";

/**
 * Trigger words / keywords of the LoRAs that are switched on, for the prompt
 * autocomplete to suggest first.
 *
 * Leaf store: it does not read `generation`. App.svelte pushes the enabled LoRA
 * filenames in through `setActive()` (see the cross-store rule in CLAUDE.md).
 *
 * Keywords come from the same `get_lora_civitai_info` data the LoRA gallery
 * shows as trigger-word chips: CivitAI `trainedWords` first, then the
 * safetensors modelspec trigger phrase. That command hashes the whole file and
 * may call CivitAI, so the LoRA gallery's localStorage cache is read first and
 * each LoRA is fetched at most once per session, one at a time.
 */

/** Shared with LoraGallery.svelte, which owns and writes this cache. */
const GALLERY_CACHE_KEY = "mooshieui.lora.civitai.cache.v3";

export interface LoraKeyword {
  /** The keyword exactly as it is inserted into the prompt. */
  word: string;
  /** LoRA filename the keyword belongs to. */
  lora: string;
  /** Human readable LoRA name for the dropdown tooltip. */
  label: string;
}

interface LoraKeywordInfo {
  label: string;
  words: string[];
}

function normalize(text: string): string {
  return text.toLowerCase().replace(/_/g, " ").replace(/\s+/g, " ").trim();
}

function fileLabel(filename: string): string {
  const base = filename.split(/[\\/]/).pop() ?? filename;
  return base.replace(/\.(safetensors|ckpt|pt|pth|bin)$/i, "");
}

/** Same word list the LoRA gallery shows as chips. */
export function keywordsFromInfo(info: LoraCivitaiInfo): string[] {
  const raw = info.civitai_trigger_words?.length
    ? info.civitai_trigger_words
    : (info.modelspec_trigger_phrase ?? "").split(",");
  const seen = new Set<string>();
  const words: string[] = [];
  for (const entry of raw) {
    const word = entry?.trim();
    if (!word) continue;
    const key = normalize(word);
    if (seen.has(key)) continue;
    seen.add(key);
    words.push(word);
  }
  return words;
}

function hasKeywordData(info: LoraCivitaiInfo): boolean {
  return Boolean(info.civitai_trigger_words?.length || info.modelspec_trigger_phrase?.trim());
}

function readGalleryCache(filename: string): LoraCivitaiInfo | null {
  try {
    const raw = localStorage.getItem(GALLERY_CACHE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Record<string, { data?: LoraCivitaiInfo }>;
    return parsed[filename]?.data ?? null;
  } catch {
    return null;
  }
}

class LoraKeywordsStore {
  /** Enabled LoRA filenames, in panel order. */
  private active = $state.raw<string[]>([]);
  /** Keyword data per LoRA filename (empty `words` when it has none). */
  private byLora = $state.raw<Record<string, LoraKeywordInfo>>({});

  private activeKey = "";
  private queued = new Set<string>();
  private queue: string[] = [];
  private running = false;

  /** Set the enabled LoRAs. Cheap to call often; only changes do work. */
  setActive(filenames: string[]) {
    const key = filenames.join("\n");
    if (key === this.activeKey) return;
    this.activeKey = key;
    this.active = filenames;
    for (const name of filenames) {
      if (name in this.byLora || this.queued.has(name)) continue;
      const cached = readGalleryCache(name);
      if (cached && hasKeywordData(cached)) {
        this.ingest(name, cached);
        continue;
      }
      this.queued.add(name);
      this.queue.push(name);
    }
    void this.drain();
  }

  /** Record fresh info for a LoRA (the gallery calls this after each fetch). */
  ingest(filename: string, info: LoraCivitaiInfo) {
    const label = info.civitai_name || info.modelspec_title || fileLabel(filename);
    this.byLora = { ...this.byLora, [filename]: { label, words: keywordsFromInfo(info) } };
  }

  private async drain() {
    if (this.running) return;
    this.running = true;
    try {
      while (this.queue.length > 0) {
        const name = this.queue.shift()!;
        try {
          const info = await getLoraCivitaiInfo(name);
          // The gallery may have filled it in while this one was hashing.
          if (!(name in this.byLora) || !this.byLora[name].words.length) this.ingest(name, info);
        } catch (e) {
          // Missing file, no ComfyUI path, or no Model Hub access: remember the
          // miss so it isn't retried on every LoRA toggle.
          console.warn(`loraKeywords: no info for ${name}`, e);
          if (!(name in this.byLora)) {
            this.byLora = { ...this.byLora, [name]: { label: fileLabel(name), words: [] } };
          }
        } finally {
          this.queued.delete(name);
        }
      }
    } finally {
      this.running = false;
    }
  }

  /** All keywords of the enabled LoRAs, deduplicated, in panel order. */
  get keywords(): LoraKeyword[] {
    const seen = new Set<string>();
    const out: LoraKeyword[] = [];
    for (const lora of this.active) {
      const info = this.byLora[lora];
      if (!info) continue;
      for (const word of info.words) {
        const key = normalize(word);
        if (seen.has(key)) continue;
        seen.add(key);
        out.push({ word, lora, label: info.label });
      }
    }
    return out;
  }

  /**
   * Keywords matching a typed fragment: a keyword (or one of its words) that
   * starts with the query first, then, from three characters on, any keyword
   * that contains it.
   */
  match(query: string, limit: number): LoraKeyword[] {
    const q = normalize(query);
    if (!q || limit <= 0) return [];
    const starts: LoraKeyword[] = [];
    const contains: LoraKeyword[] = [];
    for (const kw of this.keywords) {
      const k = normalize(kw.word);
      if (k.startsWith(q) || k.includes(` ${q}`)) starts.push(kw);
      else if (q.length >= 3 && k.includes(q)) contains.push(kw);
    }
    return [...starts, ...contains].slice(0, limit);
  }
}

export const loraKeywords = new LoraKeywordsStore();

export function normalizeKeyword(text: string): string {
  return normalize(text);
}
