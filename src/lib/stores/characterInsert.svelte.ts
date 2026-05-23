import type { AnimadexCharacter } from "../animadex/types.js";
import {
  analyzePromptForCharacter,
  applyCharacterInsert,
  buildCharacterInsertText,
  type CharacterInsertMode,
  type CharacterTagLevel,
  type PromptCharacterAnalysis,
  previewGirlCountAfterAdd as computeGirlCountAfterAdd,
} from "../animadex/characterInsert.js";
import { generation } from "./generation.svelte.js";

export type CharacterInsertStep = "pick_tags" | "pick_action" | "duplicate";

export type CharacterInsertPending = {
  character: AnimadexCharacter;
  step: CharacterInsertStep;
  analysis: PromptCharacterAnalysis;
  tagLevel?: CharacterTagLevel;
};

class CharacterInsertStore {
  pending = $state<CharacterInsertPending | null>(null);

  request(character: AnimadexCharacter): void {
    const analysis = analyzePromptForCharacter(generation.positivePrompt, character);
    if (analysis.isDuplicate) {
      this.pending = { character, analysis, step: "duplicate" };
      return;
    }
    this.pending = { character, analysis, step: "pick_tags" };
  }

  chooseTagLevel(level: CharacterTagLevel): void {
    const p = this.pending;
    if (!p || p.step !== "pick_tags") return;
    if (p.analysis.needsActionChoice) {
      this.pending = { ...p, tagLevel: level, step: "pick_action" };
      return;
    }
    this.apply(level, "add");
  }

  apply(level: CharacterTagLevel, mode: CharacterInsertMode): void {
    const p = this.pending;
    if (!p) return;
    const resolvedLevel = level ?? p.tagLevel;
    if (!resolvedLevel) return;
    generation.positivePrompt = applyCharacterInsert(
      generation.positivePrompt,
      p.character,
      resolvedLevel,
      mode,
    );
    generation.saveSettings();
    this.pending = null;
  }

  dismiss(): void {
    this.pending = null;
  }

  previewInsert(level: CharacterTagLevel): string {
    const p = this.pending;
    if (!p) return "";
    return buildCharacterInsertText(p.character, level);
  }

  previewGirlCountAfterAdd(): string {
    const p = this.pending;
    if (!p) return "2girls";
    return computeGirlCountAfterAdd(p.analysis);
  }
}

export const characterInsert = new CharacterInsertStore();
