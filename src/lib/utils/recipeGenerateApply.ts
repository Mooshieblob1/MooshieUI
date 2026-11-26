/**
 * Silently applies active Enhance Concept Recipes to outgoing `GenerationParams`
 * right before generation submits, without mutating the user's visible prompt
 * boxes. Fingerprinting and AI adaptation run strictly on user prompts + active
 * recipes (before settings/model injections and without model/flow keys), so
 * changing parameters, models, presets, or character coordinates never triggers
 * redundant recipe runs.
 */

import { enhanceRecipes } from "../stores/enhanceRecipes.svelte.js";
import { gallery } from "../stores/gallery.svelte.js";
import {
  generation,
  type GenerationToParamsOptions,
  type PreparedUserPrompts,
} from "../stores/generation.svelte.js";
import { locale } from "../stores/locale.svelte.js";
import { novelai } from "../stores/novelai.svelte.js";
import { promptAssistant } from "../stores/promptAssistant.svelte.js";
import { savedCharacters } from "../stores/savedCharacters.svelte.js";
import type { GenerationParams } from "../types/index.js";
import { resolveDirectorReferenceCharacterTargets } from "./characterPreciseRefLogic.js";
import { BLOCKED_MINOR_CONTENT } from "./contentSafety.js";
import {
  activeRecipeCharacters,
  applyRecipesDeterministic,
  matchGenerateTimeRecipes,
  recipePromptFingerprint,
  stripLeakedRecipeSlugs,
  stripNeverUcTags,
  type EnhanceRecipe,
  type RecipeCharacterSlot,
} from "./enhanceRecipes.js";
import { userScopedKey } from "./ipc.js";
import { mapLlmError } from "./llmError.js";
import { naiV5Variant } from "./novelaiModels.js";

interface CachedAdaptedPrompt {
  positive: string;
  negative: string;
  characters?: { prompt: string; negative_prompt: string }[];
}

const ADAPT_CACHE_KEY = "mooshieui.enhanceRecipes.adaptCache.v1";
const CACHE_LIMIT = 32;
const adaptCache = new Map<string, CachedAdaptedPrompt>();
const pendingAdapts = new Map<string, Promise<CachedAdaptedPrompt | null>>();
const toastedFallbacks = new Set<string>();
let adaptCacheStorageKey: string | null = null;
let activeAdaptCount = 0;

function ensureAdaptCacheLoaded(): void {
  const key = userScopedKey(ADAPT_CACHE_KEY);
  if (key === adaptCacheStorageKey) return;
  adaptCacheStorageKey = key;
  adaptCache.clear();
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return;
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return;
    for (const entry of parsed.slice(-CACHE_LIMIT)) {
      if (
        Array.isArray(entry) &&
        typeof entry[0] === "string" &&
        entry[1] &&
        typeof entry[1] === "object" &&
        typeof entry[1].positive === "string" &&
        typeof entry[1].negative === "string"
      ) {
        const chars = Array.isArray(entry[1].characters)
          ? entry[1].characters.filter(
              (c: unknown): c is { prompt: string; negative_prompt: string } =>
                !!c &&
                typeof c === "object" &&
                typeof (c as { prompt?: unknown }).prompt === "string" &&
                typeof (c as { negative_prompt?: unknown }).negative_prompt === "string",
            )
          : undefined;
        adaptCache.set(entry[0], {
          positive: entry[1].positive,
          negative: entry[1].negative,
          ...(chars ? { characters: chars } : {}),
        });
      }
    }
  } catch {
    /* storage unavailable */
  }
}

function saveAdaptCache(): void {
  try {
    const key = userScopedKey(ADAPT_CACHE_KEY);
    adaptCacheStorageKey = key;
    localStorage.setItem(key, JSON.stringify([...adaptCache.entries()]));
  } catch {
    /* storage unavailable */
  }
}

function readCache(key: string): CachedAdaptedPrompt | undefined {
  ensureAdaptCacheLoaded();
  const hit = adaptCache.get(key);
  if (!hit) return undefined;
  // Refresh LRU order.
  adaptCache.delete(key);
  adaptCache.set(key, hit);
  return hit;
}

function writeCache(key: string, value: CachedAdaptedPrompt): void {
  ensureAdaptCacheLoaded();
  if (adaptCache.has(key)) adaptCache.delete(key);
  adaptCache.set(key, value);
  if (adaptCache.size > CACHE_LIMIT) {
    const oldest = adaptCache.keys().next().value;
    if (oldest !== undefined) adaptCache.delete(oldest);
  }
  saveAdaptCache();
}

function notifyFallbackOnce(fp: string, key: string): void {
  if (toastedFallbacks.has(fp)) return;
  if (toastedFallbacks.size > CACHE_LIMIT) toastedFallbacks.clear();
  toastedFallbacks.add(fp);
  gallery.showToast(locale.t(key), "info");
}

function clonePreparedPrompts(prepared: PreparedUserPrompts): PreparedUserPrompts {
  return {
    ...prepared,
    rawCharacters: prepared.rawCharacters.map((c) => ({ ...c, center: { ...c.center } })),
    characters: prepared.characters.map((c) => ({ ...c, center: { ...c.center } })),
    detailSegments: prepared.detailSegments.map((s) => ({ ...s })),
  };
}

function applyCachedToPrepared(
  target: PreparedUserPrompts,
  cached: CachedAdaptedPrompt,
  activeSlots: RecipeCharacterSlot[],
  isNai: boolean,
): void {
  target.positive = cached.positive;
  target.negative = cached.negative;
  if (isNai && cached.characters) {
    const nextCharacters = target.characters.map((c) => ({ ...c, center: { ...c.center } }));
    for (let k = 0; k < activeSlots.length; k++) {
      const slot = activeSlots[k];
      const next = cached.characters[k];
      if (!next) continue;
      const existing = nextCharacters[slot.index];
      if (!existing) continue;
      nextCharacters[slot.index] = {
        ...existing,
        prompt: next.prompt,
        negative_prompt: next.negative_prompt,
      };
    }
    target.characters = nextCharacters;
  }
}

function applyDeterministicToPrepared(
  target: PreparedUserPrompts,
  recipes: EnhanceRecipe[],
  isNai: boolean,
): void {
  const res = applyRecipesDeterministic(
    {
      positive: target.positive,
      negative: target.negative,
      characters: isNai ? target.characters : undefined,
    },
    recipes,
    isNai,
  );
  target.positive = res.positive;
  target.negative = res.negative;
  if (isNai && res.characters) {
    target.characters = res.characters;
  }
}

function stripFinalConflicts(params: GenerationParams, recipes: EnhanceRecipe[]): void {
  params.positive_prompt = stripLeakedRecipeSlugs(params.positive_prompt, recipes);
  params.negative_prompt = stripNeverUcTags(
    stripLeakedRecipeSlugs(params.negative_prompt, recipes),
    recipes,
  );
  if (params.novelai) {
    params.novelai.characters = params.novelai.characters.map((c) => ({
      ...c,
      prompt: stripLeakedRecipeSlugs(c.prompt, recipes),
      negative_prompt: stripNeverUcTags(
        stripLeakedRecipeSlugs(c.negative_prompt, recipes),
        recipes,
      ),
    }));
  }
}

/**
 * Resolve Auto multi-character Precise Reference targets against the outgoing
 * character boxes (using live store `source_character_id` metadata when the
 * reference list still matches) and enforce the Settings experimental gate.
 */
function applyNovelAiMultiCharPrToParams(params: GenerationParams): GenerationParams {
  const nai = params.novelai;
  if (!nai) return params;

  if (!novelai.experimentalMultiCharPr || !nai.multi_char_pr?.enabled) {
    nai.multi_char_pr = { ...nai.multi_char_pr, enabled: false };
    nai.director_references = nai.director_references.map((r) => ({
      ...r,
      target_character_index: null,
      sheet_x: null,
      sheet_y: null,
      sheet_scale: null,
    }));
    return params;
  }

  const liveRefs = generation.novelaiSettings.director_references;
  const refsMatchStore =
    liveRefs.length === nai.director_references.length &&
    liveRefs.every((r, i) => r.image === nai.director_references[i]?.image);
  const sourceRefs = refsMatchStore ? liveRefs : nai.director_references;

  const targets = resolveDirectorReferenceCharacterTargets(
    sourceRefs,
    nai.characters,
    savedCharacters.characters,
  );
  nai.director_references = nai.director_references.map((r, i) => ({
    ...r,
    target_character_index: targets[i] ?? null,
  }));
  return params;
}

async function runAiAdapt(
  target: PreparedUserPrompts,
  aiRecipes: EnhanceRecipe[],
  isNai: boolean,
  activeSlots: RecipeCharacterSlot[],
): Promise<CachedAdaptedPrompt | null> {
  const variant = isNai ? naiV5Variant(generation.checkpoint) : null;
  if (variant !== null) {
    const adapted = await promptAssistant.adaptNaiWithRecipes(
      {
        base: target.positive,
        uc: target.negative,
        characters: activeSlots.map((c) => c.prompt),
        characterUcs: activeSlots.map((c) => c.negative_prompt),
      },
      variant,
      aiRecipes,
    );
    if (!adapted) return null;
    return {
      positive: adapted.base,
      negative: adapted.uc,
      characters: activeSlots.map((slot, i) => ({
        prompt: adapted.characters[i]?.trim() ? adapted.characters[i] : slot.prompt,
        negative_prompt: adapted.characterUcs[i] ?? slot.negative_prompt,
      })),
    };
  }
  const adapted = await promptAssistant.adaptGeneralWithRecipes(
    target.positive,
    target.negative,
    generation.modelFamily,
    aiRecipes,
  );
  if (!adapted) return null;
  return {
    positive: adapted.positive,
    negative: adapted.negative,
  };
}

/**
 * Prepare user prompts, apply any active (or inline `#slug`-triggered) Concept
 * Recipes strictly based on prompt content, then assemble final `GenerationParams`
 * with current settings/parameters.
 */
export async function buildParamsWithActiveRecipes(
  options: GenerationToParamsOptions = {},
): Promise<GenerationParams> {
  if (generation.mode === "video") return generation.toParams(options);

  const prepared = generation.prepareUserPrompts(options);
  const isNai = generation.isNovelAi;
  const activeChars = isNai ? activeRecipeCharacters(prepared.characters) : [];
  const texts = [prepared.positive, ...activeChars.map((c) => c.prompt)];
  const matched = matchGenerateTimeRecipes(
    enhanceRecipes.recipes,
    texts,
    new Set(enhanceRecipes.activeIds),
  );
  if (matched.length === 0) {
    return applyNovelAiMultiCharPrToParams(
      generation.toParams({ ...options, preparedPrompts: prepared }),
    );
  }

  // Skip double LLM processing when this exact user prompt came straight out
  // of Enhance Chat with the same active recipes.
  const rawBoxFp = recipePromptFingerprint(
    {
      positive: prepared.rawPositive,
      negative: prepared.rawNegative,
      characters: isNai ? prepared.rawCharacters : undefined,
    },
    matched,
  );
  if (enhanceRecipes.isEnhancedFingerprint(rawBoxFp)) {
    const params = generation.toParams({ ...options, preparedPrompts: prepared });
    stripFinalConflicts(params, matched);
    return applyNovelAiMultiCharPrToParams(params);
  }

  const adapted = clonePreparedPrompts(prepared);
  const aiRecipes = matched.filter((r) => r.applyMode === "ai");
  const tagRecipes = matched.filter((r) => r.applyMode === "tags");

  if (aiRecipes.length > 0) {
    const aiFp = recipePromptFingerprint(
      {
        positive: adapted.positive,
        negative: adapted.negative,
        characters: isNai ? adapted.characters : undefined,
      },
      aiRecipes,
    );
    const cached = readCache(aiFp);
    if (cached) {
      applyCachedToPrepared(adapted, cached, activeChars, isNai);
    } else if (!promptAssistant.isAvailable) {
      applyDeterministicToPrepared(adapted, aiRecipes, isNai);
      notifyFallbackOnce(aiFp, "prompt_assistant.recipe_silent_fallback_no_model");
    } else {
      enhanceRecipes.isApplyingAtGenerate = ++activeAdaptCount > 0;
      try {
        let inflight = pendingAdapts.get(aiFp);
        if (!inflight) {
          inflight = runAiAdapt(adapted, aiRecipes, isNai, activeChars).finally(() => {
            pendingAdapts.delete(aiFp);
          });
          pendingAdapts.set(aiFp, inflight);
        }
        const snapshot = await inflight;
        if (snapshot) {
          writeCache(aiFp, snapshot);
          applyCachedToPrepared(adapted, snapshot, activeChars, isNai);
        } else {
          applyDeterministicToPrepared(adapted, aiRecipes, isNai);
          notifyFallbackOnce(aiFp, "prompt_assistant.recipe_silent_fallback");
        }
      } catch (e) {
        const msg = String(e);
        if (msg.includes(BLOCKED_MINOR_CONTENT)) {
          throw new Error(mapLlmError(msg));
        }
        console.warn("[enhanceRecipes] silent adapt failed, falling back to tags:", e);
        applyDeterministicToPrepared(adapted, aiRecipes, isNai);
        notifyFallbackOnce(aiFp, "prompt_assistant.recipe_silent_fallback");
      } finally {
        enhanceRecipes.isApplyingAtGenerate = --activeAdaptCount > 0;
      }
    }
  }

  if (tagRecipes.length > 0) {
    applyDeterministicToPrepared(adapted, tagRecipes, isNai);
  }

  const params = generation.toParams({ ...options, preparedPrompts: adapted });
  stripFinalConflicts(params, matched);
  return applyNovelAiMultiCharPrToParams(params);
}
