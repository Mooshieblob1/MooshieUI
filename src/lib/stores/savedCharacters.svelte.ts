import { triggerSync } from "../utils/syncTrigger.js";
import { userScopedKey } from "../utils/ipc.js";
import { MODEL_FAMILY_LABELS, type ModelFamily } from "../utils/modelFamily.js";
import { novelAiMaxCharacters } from "../utils/novelaiModels.js";
import type { CharacterPromptStyle } from "../utils/characterExtract.js";
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
  /**
   * The same picture at a size NovelAI can read as a Precise Reference, as an
   * image data URL, or null. Made alongside the thumbnail.
   */
  reference: string | null;
  /**
   * The card this one is a variant of (another outfit or look of the same
   * character), or null for a character of its own. Always a top-level card:
   * variants do not nest.
   */
  parentId: string | null;
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

/** Architectures whose prompts are written as Danbooru-style tags. */
const TAG_ARCHITECTURES = new Set<string>([NOVELAI_ARCHITECTURE, "sdxl", "illustrious", "pony", "sd15", "anima"]);

/** How a character is written for `architecture`: tags, or natural language. */
export function architecturePromptStyle(architecture: string): CharacterPromptStyle {
  return TAG_ARCHITECTURES.has(architecture) ? "tags" : "natural";
}

/** Every architecture a card can be copied to, NovelAI first. */
export function characterArchitectures(): string[] {
  return [NOVELAI_ARCHITECTURE, ...Object.keys(MODEL_FAMILY_LABELS)];
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
    reference: typeof c.reference === "string" && c.reference.startsWith("data:image/") ? c.reference : null,
    parentId: typeof c.parentId === "string" && c.parentId ? c.parentId : null,
    createdAt: typeof c.createdAt === "number" ? c.createdAt : now,
    updatedAt: typeof c.updatedAt === "number" ? c.updatedAt : now,
  };
}

function sanitizeList(raw: unknown): SavedCharacter[] {
  if (!Array.isArray(raw)) return [];
  const list = raw.map(sanitize).filter((c): c is SavedCharacter => c !== null);
  // A variant whose parent is gone, or is itself a variant, or sits under
  // another architecture, stands on its own rather than vanishing from view.
  const tops = new Map(list.filter((c) => !c.parentId).map((c) => [c.id, c.architecture]));
  return list.map((c) =>
    c.parentId && tops.get(c.parentId) !== c.architecture ? { ...c, parentId: null } : c,
  );
}

/** The JSON file Export writes and Import reads. */
export const CHARACTER_EXPORT_FORMAT = "mooshieui.characters";

/** A new card in `architecture`, before it is added. */
export interface NewCharacter {
  name: string;
  prompt: string;
  negative: string;
  thumbnail?: string | null;
  reference?: string | null;
  parentId?: string | null;
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
   * Save characters under `architecture` as new cards. Duplicates of saved
   * cards are sorted out before this by the Characters tab's review; a name
   * already taken there gets a number so two cards never share one.
   * Returns how many were added.
   */
  saveExtracted(extracted: NewCharacter[], architecture: string): number {
    const now = Date.now();
    const next = [...this.characters];
    const taken = new Set(
      next.filter((c) => c.architecture === architecture).map((c) => c.name.toLowerCase()),
    );
    let added = 0;
    for (const item of extracted) {
      let name = item.name.trim();
      for (let n = 2; taken.has(name.toLowerCase()); n++) name = `${item.name.trim()} ${n}`;
      taken.add(name.toLowerCase());
      next.push({
        id: newId(),
        name,
        prompt: item.prompt,
        negative: item.negative,
        architecture,
        thumbnail: item.thumbnail ?? null,
        reference: item.reference ?? null,
        parentId: item.parentId ?? null,
        createdAt: now,
        updatedAt: now,
      });
      added++;
    }
    if (added > 0) {
      this.characters = next;
      this.saveSettings();
    }
    return added;
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

  /**
   * Set a card's thumbnail and its reference-sized copy, or clear both (null).
   * Changes nothing else about it.
   */
  setThumbnail(id: string, thumbnail: string | null, reference: string | null = null): void {
    this.characters = this.characters.map((c) =>
      c.id === id ? { ...c, thumbnail, reference: thumbnail ? reference : null } : c,
    );
    this.saveSettings();
  }

  /** Delete a card. Its variants stay, as characters of their own. */
  remove(id: string): void {
    this.characters = this.characters
      .filter((c) => c.id !== id)
      .map((c) => (c.parentId === id ? { ...c, parentId: null } : c));
    this.saveSettings();
  }

  /** Variants of a top-level card, oldest first. */
  variantsOf(id: string): SavedCharacter[] {
    return this.characters
      .filter((c) => c.parentId === id)
      .sort((a, b) => a.createdAt - b.createdAt);
  }

  /**
   * Start a variant of `character` as a copy with its own name, filed under
   * the top-level card (a variant of a variant is a variant of its parent).
   * Returns the new card's id.
   */
  addVariant(character: SavedCharacter, label: string): string | null {
    const parentId = character.parentId ?? character.id;
    const parent = this.characters.find((c) => c.id === parentId) ?? character;
    const before = new Set(this.characters.map((c) => c.id));
    const added = this.saveExtracted(
      [
        {
          name: `${parent.name} (${label})`,
          prompt: character.prompt,
          negative: character.negative,
          thumbnail: character.thumbnail,
          reference: character.reference,
          parentId,
        },
      ],
      character.architecture,
    );
    if (!added) return null;
    return this.characters.find((c) => !before.has(c.id))?.id ?? null;
  }

  /**
   * File an existing card as a variant of `parentId`, or back as a character
   * of its own (null). Its own variants move up with it.
   */
  setParent(id: string, parentId: string | null): void {
    const parent = parentId ? this.characters.find((c) => c.id === parentId) : null;
    const target = parent?.parentId ?? parent?.id ?? null;
    if (target === id) return;
    this.characters = this.characters.map((c) => {
      if (c.id === id) return { ...c, parentId: target };
      if (target && c.parentId === id) return { ...c, parentId: target };
      return c;
    });
    this.saveSettings();
  }

  /** Every saved card as an Export file. */
  exportJson(): string {
    return JSON.stringify(
      { format: CHARACTER_EXPORT_FORMAT, version: 1, characters: this.characters },
      null,
      2,
    );
  }

  /**
   * Add the cards in an Export file. A card already saved with the same
   * architecture, name and prompt is skipped; anything else is added as new,
   * with variants kept under their imported parent. Null when the file is not
   * an Export file at all.
   */
  importJson(text: string): { added: number; skipped: number } | null {
    let data: unknown;
    try {
      data = JSON.parse(text);
    } catch {
      return null;
    }
    const raw =
      data && typeof data === "object" && Array.isArray((data as { characters?: unknown }).characters)
        ? (data as { characters: unknown[] }).characters
        : Array.isArray(data)
          ? data
          : null;
    if (!raw) return null;
    const incoming = sanitizeList(raw);
    const key = (c: SavedCharacter) =>
      `${c.architecture}\u0000${c.name.trim().toLowerCase()}\u0000${c.prompt.trim().toLowerCase()}`;
    const existing = new Map(this.characters.map((c) => [key(c), c.id]));
    const takenIds = new Set(this.characters.map((c) => c.id));
    // Imported id to the id it ends up with, so variants find their parent
    // whether it was added or was already here.
    const idMap = new Map<string, string>();
    const added: SavedCharacter[] = [];
    let skipped = 0;
    const now = Date.now();
    for (const c of incoming) {
      const match = existing.get(key(c));
      if (match) {
        idMap.set(c.id, match);
        skipped++;
        continue;
      }
      const id = takenIds.has(c.id) ? newId() : c.id;
      takenIds.add(id);
      idMap.set(c.id, id);
      existing.set(key(c), id);
      added.push({ ...c, id, updatedAt: now });
    }
    if (added.length > 0) {
      const remapped = added.map((c) => ({ ...c, parentId: c.parentId ? (idMap.get(c.parentId) ?? null) : null }));
      this.characters = sanitizeList([...this.characters, ...remapped]);
      this.saveSettings();
    }
    return { added: added.length, skipped };
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
