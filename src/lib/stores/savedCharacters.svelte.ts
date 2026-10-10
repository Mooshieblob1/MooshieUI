import { triggerSync } from "../utils/syncTrigger.js";
import { userScopedKey } from "../utils/ipc.js";
import { MODEL_FAMILY_LABELS, type ModelFamily } from "../utils/modelFamily.js";
import { novelAiMaxCharacters } from "../utils/novelaiModels.js";
import type { ExtractedCharacter } from "../utils/characterExtract.js";
import { createNovelAiCharacter, generation } from "./generation.svelte.js";

const STORAGE_KEY = "mooshieui.savedCharacters.v1";

/** Every NovelAI model shares one architecture bucket; their tags carry over. */
export const NOVELAI_ARCHITECTURE = "novelai";

/**
 * A character pulled out of a prompt and kept for reuse. Each one belongs to
 * the architecture it was saved under, because a tag prompt written for SDXL
 * means little to Flux and the reverse.
 */
export interface SavedCharacter {
  id: string;
  name: string;
  /** The text inserted back into a prompt. */
  prompt: string;
  /** What to avoid for this character, inserted into the negative prompt (UC). */
  negative: string;
  /** A `ModelFamily` value, or `NOVELAI_ARCHITECTURE`. */
  architecture: string;
  createdAt: number;
  updatedAt: number;
}

/** Where `insert` put the character. */
export type CharacterInsertResult = "prompt" | "novelai_character" | "duplicate";

function newId(): string {
  return crypto.randomUUID?.() || `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

/** Append `text` to a comma-separated prompt, dropping a trailing comma first. */
function joinPrompt(current: string, text: string): string {
  const base = current.trim().replace(/,+$/, "").trimEnd();
  return base ? `${base}, ${text}` : text;
}

/** Display name of an architecture bucket. */
export function architectureLabel(architecture: string): string {
  if (architecture === NOVELAI_ARCHITECTURE) return "NovelAI";
  return MODEL_FAMILY_LABELS[architecture as ModelFamily] ?? architecture;
}

function sanitize(raw: unknown): SavedCharacter | null {
  if (!raw || typeof raw !== "object") return null;
  const c = raw as Record<string, unknown>;
  if (typeof c.name !== "string" || typeof c.prompt !== "string" || typeof c.architecture !== "string") {
    return null;
  }
  if (!c.name.trim() || !c.architecture) return null;
  const now = Date.now();
  return {
    id: typeof c.id === "string" && c.id ? c.id : newId(),
    name: c.name,
    prompt: c.prompt,
    negative: typeof c.negative === "string" ? c.negative : "",
    architecture: c.architecture,
    createdAt: typeof c.createdAt === "number" ? c.createdAt : now,
    updatedAt: typeof c.updatedAt === "number" ? c.updatedAt : now,
  };
}

function sanitizeList(raw: unknown): SavedCharacter[] {
  if (!Array.isArray(raw)) return [];
  return raw.map(sanitize).filter((c): c is SavedCharacter => c !== null);
}

class SavedCharactersStore {
  characters = $state<SavedCharacter[]>([]);

  constructor() {
    this.loadSettings();
  }

  /**
   * Architecture of the selected model, or null when it is not known yet (no
   * model picked, or detection has not resolved), since a character saved
   * there could never be shown again.
   */
  get currentArchitecture(): string | null {
    if (generation.isNovelAi) return NOVELAI_ARCHITECTURE;
    const family = generation.modelFamily;
    return family && family !== "unknown" ? family : null;
  }

  /** Characters saved for the selected model's architecture, newest first. */
  get currentCharacters(): SavedCharacter[] {
    const arch = this.currentArchitecture;
    if (!arch) return [];
    return this.characters
      .filter((c) => c.architecture === arch)
      .sort((a, b) => b.updatedAt - a.updatedAt);
  }

  /**
   * Save extracted characters under `architecture`. A name already saved there
   * (ignoring case) has its prompt replaced instead of being added twice.
   */
  saveExtracted(
    extracted: ExtractedCharacter[],
    architecture: string,
  ): { added: number; updated: number } {
    const now = Date.now();
    let added = 0;
    let updated = 0;
    const next = [...this.characters];
    for (const item of extracted) {
      const key = item.name.toLowerCase();
      const index = next.findIndex(
        (c) => c.architecture === architecture && c.name.toLowerCase() === key,
      );
      if (index !== -1) {
        if (next[index].prompt !== item.prompt || next[index].negative !== item.negative) {
          next[index] = {
            ...next[index],
            prompt: item.prompt,
            negative: item.negative,
            updatedAt: now,
          };
          updated++;
        }
        continue;
      }
      next.push({
        id: newId(),
        name: item.name,
        prompt: item.prompt,
        negative: item.negative,
        architecture,
        createdAt: now,
        updatedAt: now,
      });
      added++;
    }
    if (added > 0 || updated > 0) {
      this.characters = next;
      this.saveSettings();
    }
    return { added, updated };
  }

  /** Empty name or prompt keeps the old one; an empty negative clears it. */
  update(id: string, patch: { name?: string; prompt?: string; negative?: string }): void {
    const name = patch.name?.trim();
    const prompt = patch.prompt?.trim();
    const negative = patch.negative?.trim();
    this.characters = this.characters.map((c) =>
      c.id === id
        ? {
            ...c,
            ...(name ? { name } : {}),
            ...(prompt ? { prompt } : {}),
            ...(negative !== undefined ? { negative } : {}),
            updatedAt: Date.now(),
          }
        : c,
    );
    this.saveSettings();
  }

  remove(id: string): void {
    this.characters = this.characters.filter((c) => c.id !== id);
    this.saveSettings();
  }

  /**
   * Put a saved character into the current prompt. NovelAI models with
   * character prompts get it as its own character box (filling an empty one
   * first) while there is room, with its UC in that box's undesired content;
   * everything else appends it to the positive prompt and its UC to the
   * negative prompt.
   */
  insert(character: SavedCharacter): CharacterInsertResult {
    const text = character.prompt.trim();
    const negative = character.negative.trim();
    const lower = text.toLowerCase();
    if (generation.positivePrompt.toLowerCase().includes(lower)) return "duplicate";

    if (generation.isNovelAi && generation.novelAiModel?.v4Prompt) {
      const boxes = generation.novelaiSettings.characters;
      if (boxes.some((b) => b.prompt.trim().toLowerCase() === lower)) return "duplicate";
      // Models without per-character UC take it in the main negative prompt.
      const boxNegative = generation.novelAiModel.characterNegatives;
      const box = { prompt: text, ...(boxNegative ? { negative_prompt: negative } : {}) };
      const empty = boxes.findIndex((b) => b.prompt.trim() === "");
      const room = boxes.length < novelAiMaxCharacters(generation.checkpoint);
      if (empty !== -1) {
        generation.updateNovelAiCharacter(empty, { ...box, enabled: true });
      } else if (room) {
        generation.updateNovelAiSettings({
          characters: [...boxes, { ...createNovelAiCharacter(), ...box }],
        });
      }
      if (empty !== -1 || room) {
        if (!boxNegative) this.appendNegative(negative);
        return "novelai_character";
      }
    }

    generation.positivePrompt = joinPrompt(generation.positivePrompt, text);
    this.appendNegative(negative);
    generation.saveSettings();
    return "prompt";
  }

  /** Add to the main negative prompt unless it is empty or already there. */
  private appendNegative(negative: string): void {
    if (!negative) return;
    if (generation.negativePrompt.toLowerCase().includes(negative.toLowerCase())) return;
    generation.negativePrompt = joinPrompt(generation.negativePrompt, negative);
    generation.saveSettings();
  }

  loadSettings(): void {
    try {
      const raw = localStorage.getItem(userScopedKey(STORAGE_KEY));
      if (!raw) return;
      this.characters = sanitizeList(JSON.parse(raw)?.characters);
    } catch (e) {
      console.error("Failed to load saved characters:", e);
    }
  }

  saveSettings(): void {
    try {
      localStorage.setItem(
        userScopedKey(STORAGE_KEY),
        JSON.stringify({ characters: this.characters }),
      );
      triggerSync();
    } catch (e) {
      console.error("Failed to save saved characters:", e);
    }
  }

  collectPrefs(): unknown {
    return { characters: this.characters };
  }

  applyServerPrefs(data: any): void {
    try {
      if (!Array.isArray(data?.characters)) return;
      this.characters = sanitizeList(data.characters);
      localStorage.setItem(
        userScopedKey(STORAGE_KEY),
        JSON.stringify({ characters: this.characters }),
      );
    } catch (e) {
      console.error("Failed to apply server prefs (characters):", e);
    }
  }
}

export const savedCharacters = new SavedCharactersStore();
