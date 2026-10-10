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
  /** Card thumbnail as a small image data URL, or null for none. */
  thumbnail: string | null;
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
    thumbnail: typeof c.thumbnail === "string" && c.thumbnail.startsWith("data:image/") ? c.thumbnail : null,
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
        thumbnail: null,
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

  /** Set or clear (null) a card's thumbnail. Changes nothing else about it. */
  setThumbnail(id: string, thumbnail: string | null): void {
    this.characters = this.characters.map((c) => (c.id === id ? { ...c, thumbnail } : c));
    this.saveSettings();
  }

  remove(id: string): void {
    this.characters = this.characters.filter((c) => c.id !== id);
    this.saveSettings();
  }

  /** True when the character's text is already in the prompt or a character box. */
  isInPrompt(character: SavedCharacter): boolean {
    const lower = character.prompt.trim().toLowerCase();
    if (generation.positivePrompt.toLowerCase().includes(lower)) return true;
    if (!generation.isNovelAi || !generation.novelAiModel?.v4Prompt) return false;
    return generation.novelaiSettings.characters.some(
      (b) => b.prompt.trim().toLowerCase() === lower,
    );
  }

  /**
   * Where a NovelAI V4+ character box can take the character: the index of an
   * empty box to fill, or -1 for a new one. Null when the character goes into
   * the positive prompt instead (not NovelAI, an older model, or every slot
   * taken).
   */
  boxTarget(): { index: number; boxNegative: boolean } | null {
    if (!generation.isNovelAi || !generation.novelAiModel?.v4Prompt) return null;
    const boxes = generation.novelaiSettings.characters;
    const boxNegative = generation.novelAiModel.characterNegatives;
    const empty = boxes.findIndex((b) => b.prompt.trim() === "");
    if (empty !== -1) return { index: empty, boxNegative };
    if (boxes.length < novelAiMaxCharacters(generation.checkpoint)) {
      return { index: -1, boxNegative };
    }
    return null;
  }

  /**
   * Put a saved character into the current prompt as it is. NovelAI models
   * with character prompts get it as its own character box while there is
   * room, with its UC in that box's undesired content; everything else
   * appends it to the positive prompt and its UC to the negative prompt.
   */
  insert(character: SavedCharacter): CharacterInsertResult {
    if (this.isInPrompt(character)) return "duplicate";
    const text = character.prompt.trim();
    const negative = character.negative.trim();
    const target = this.boxTarget();
    if (target) {
      this.writeBox(target.index, text, target.boxNegative ? negative : "");
      if (!target.boxNegative) this.appendNegative(negative);
      return "novelai_character";
    }
    generation.positivePrompt = joinPrompt(generation.positivePrompt, text);
    this.appendNegative(negative);
    generation.saveSettings();
    return "prompt";
  }

  /**
   * Write a reviewed LLM merge. Only the fields passed are written, so a row
   * the user unticked keeps what is there now. `character` (box mode) goes
   * into the box `boxTarget` picks at apply time.
   */
  applyMerge(fields: {
    prompt?: string;
    negative?: string;
    character?: string;
    characterNegative?: string;
  }): void {
    if (fields.prompt !== undefined) generation.positivePrompt = fields.prompt;
    if (fields.negative !== undefined) generation.negativePrompt = fields.negative;
    const target = fields.character ? this.boxTarget() : null;
    if (target && fields.character) {
      this.writeBox(target.index, fields.character, target.boxNegative ? (fields.characterNegative ?? "") : "");
    } else if (fields.character) {
      // Every slot filled up since the merge ran: fall back to the prompt.
      generation.positivePrompt = joinPrompt(generation.positivePrompt, fields.character);
    }
    generation.saveSettings();
  }

  /** Fill box `index`, or add a new one when it is -1. */
  private writeBox(index: number, prompt: string, negative: string): void {
    const box = { prompt, negative_prompt: negative };
    if (index !== -1) {
      generation.updateNovelAiCharacter(index, { ...box, enabled: true });
    } else {
      generation.updateNovelAiSettings({
        characters: [...generation.novelaiSettings.characters, { ...createNovelAiCharacter(), ...box }],
      });
    }
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
