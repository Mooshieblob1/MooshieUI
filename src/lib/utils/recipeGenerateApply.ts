/**
 * Silently applies active Enhance Concept Recipes to outgoing `GenerationParams`
 * right before generation submits, without mutating the user's visible prompt
 * boxes. Results from AI adaptation are cached per `(prompt, active recipes)`
 * so re-rolls, batches, and repeat runs are instant, and any non-safety LLM
 * failure falls back cleanly to deterministic tag/UC injection.
 */

import { enhanceRecipes } from "../stores/enhanceRecipes.svelte.js";
import { gallery } from "../stores/gallery.svelte.js";
import { generation } from "../stores/generation.svelte.js";
import { locale } from "../stores/locale.svelte.js";
import { promptAssistant } from "../stores/promptAssistant.svelte.js";
import type { GenerationParams } from "../types/index.js";
import { BLOCKED_MINOR_CONTENT } from "./contentSafety.js";
import {
  applyRecipesDeterministic,
  matchGenerateTimeRecipes,
  recipePromptFingerprint,
  stripLeakedRecipeSlugs,
  stripNeverUcTags,
  type EnhanceRecipe,
} from "./enhanceRecipes.js";
import { mapLlmError } from "./llmError.js";
import { isNovelAiModel, naiV5Variant } from "./novelaiModels.js";

interface CachedAdaptedPrompt {
  positive: string;
  negative: string;
  characters?: { prompt: string; negative_prompt: string }[];
}

const CACHE_LIMIT = 32;
const adaptCache = new Map<string, CachedAdaptedPrompt>();
const toastedFallbacks = new Set<string>();

function readCache(key: string): CachedAdaptedPrompt | undefined {
  const hit = adaptCache.get(key);
  if (!hit) return undefined;
  // Refresh LRU order.
  adaptCache.delete(key);
  adaptCache.set(key, hit);
  return hit;
}

function writeCache(key: string, value: CachedAdaptedPrompt): void {
  if (adaptCache.has(key)) adaptCache.delete(key);
  adaptCache.set(key, value);
  if (adaptCache.size > CACHE_LIMIT) {
    const oldest = adaptCache.keys().next().value;
    if (oldest !== undefined) adaptCache.delete(oldest);
  }
}

function notifyFallbackOnce(fp: string, key: string): void {
  if (toastedFallbacks.has(fp)) return;
  if (toastedFallbacks.size > CACHE_LIMIT) toastedFallbacks.clear();
  toastedFallbacks.add(fp);
  gallery.showToast(locale.t(key), "info");
}

function cloneParamsForRecipes(params: GenerationParams): GenerationParams {
  return {
    ...params,
    novelai: params.novelai
      ? {
          ...params.novelai,
          characters: params.novelai.characters.map((c) => ({
            ...c,
            center: { ...c.center },
          })),
        }
      : undefined,
  };
}

function applyCachedToClone(clone: GenerationParams, cached: CachedAdaptedPrompt): void {
  clone.positive_prompt = cached.positive;
  clone.negative_prompt = cached.negative;
  if (clone.novelai && cached.characters) {
    clone.novelai.characters = clone.novelai.characters.map((existing, i) => {
      const next = cached.characters?.[i];
      return next
        ? {
            ...existing,
            prompt: next.prompt,
            negative_prompt: next.negative_prompt,
          }
        : existing;
    });
  }
}

function applyDeterministicToClone(
  clone: GenerationParams,
  recipes: EnhanceRecipe[],
  isNai: boolean,
): void {
  const res = applyRecipesDeterministic(
    {
      positive: clone.positive_prompt,
      negative: clone.negative_prompt,
      characters: clone.novelai?.characters,
    },
    recipes,
    isNai,
  );
  clone.positive_prompt = res.positive;
  clone.negative_prompt = res.negative;
  if (clone.novelai && res.characters) {
    clone.novelai.characters = res.characters;
  }
}

function stripFinalConflicts(clone: GenerationParams, recipes: EnhanceRecipe[]): void {
  clone.positive_prompt = stripLeakedRecipeSlugs(clone.positive_prompt, recipes);
  clone.negative_prompt = stripNeverUcTags(
    stripLeakedRecipeSlugs(clone.negative_prompt, recipes),
    recipes,
  );
  if (clone.novelai) {
    clone.novelai.characters = clone.novelai.characters.map((c) => ({
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
 * Apply any active (or inline `#slug`-triggered) Concept Recipes to a cloned
 * `GenerationParams` right before submission.
 */
export async function applyActiveRecipesToParams(
  params: GenerationParams,
): Promise<GenerationParams> {
  if (params.mode === "video") return params;

  const texts = [
    params.positive_prompt,
    ...(params.novelai?.characters.map((c) => c.prompt) ?? []),
  ];
  const matched = matchGenerateTimeRecipes(
    enhanceRecipes.recipes,
    texts,
    new Set(enhanceRecipes.activeIds),
  );
  if (matched.length === 0) return params;

  const clone = cloneParamsForRecipes(params);
  const isNai = isNovelAiModel(clone.checkpoint);
  const variant = isNai ? naiV5Variant(clone.checkpoint) : null;
  const flowKey = variant ? `nai-${variant}` : `general:${generation.modelFamily}`;

  // Skip double LLM processing when this exact prompt came straight out of
  // Enhance Chat with the same active recipes.
  const rawBoxFp = recipePromptFingerprint(
    {
      flow: flowKey,
      positive: generation.positivePrompt,
      negative: generation.negativePrompt,
      characters: variant !== null ? generation.novelaiSettings.characters : undefined,
    },
    matched,
  );
  if (enhanceRecipes.isEnhancedFingerprint(rawBoxFp)) {
    stripFinalConflicts(clone, matched);
    return clone;
  }

  const aiRecipes = matched.filter((r) => r.applyMode === "ai");
  const tagRecipes = matched.filter((r) => r.applyMode === "tags");

  if (aiRecipes.length > 0) {
    const aiFp = recipePromptFingerprint(
      {
        flow: flowKey,
        positive: clone.positive_prompt,
        negative: clone.negative_prompt,
        characters: clone.novelai?.characters,
      },
      aiRecipes,
    );
    const cached = readCache(aiFp);
    if (cached) {
      applyCachedToClone(clone, cached);
    } else if (!promptAssistant.isAvailable) {
      applyDeterministicToClone(clone, aiRecipes, isNai);
      notifyFallbackOnce(aiFp, "prompt_assistant.recipe_silent_fallback_no_model");
    } else {
      enhanceRecipes.isApplyingAtGenerate = true;
      try {
        if (variant !== null) {
          const boxes = clone.novelai?.characters ?? [];
          const adapted = await promptAssistant.adaptNaiWithRecipes(
            {
              base: clone.positive_prompt,
              uc: clone.negative_prompt,
              characters: boxes.map((c) => c.prompt),
              characterUcs: boxes.map((c) => c.negative_prompt),
            },
            variant,
            aiRecipes,
          );
          if (adapted) {
            const nextChars = boxes.map((c, i) => ({
              prompt: adapted.characters[i]?.trim() ? adapted.characters[i] : c.prompt,
              negative_prompt: adapted.characterUcs[i] ?? c.negative_prompt,
            }));
            const snapshot: CachedAdaptedPrompt = {
              positive: adapted.base,
              negative: adapted.uc,
              characters: clone.novelai ? nextChars : undefined,
            };
            writeCache(aiFp, snapshot);
            applyCachedToClone(clone, snapshot);
          } else {
            applyDeterministicToClone(clone, aiRecipes, isNai);
            notifyFallbackOnce(aiFp, "prompt_assistant.recipe_silent_fallback");
          }
        } else {
          const adapted = await promptAssistant.adaptGeneralWithRecipes(
            clone.positive_prompt,
            clone.negative_prompt,
            generation.modelFamily,
            aiRecipes,
          );
          if (adapted) {
            const snapshot: CachedAdaptedPrompt = {
              positive: adapted.positive,
              negative: adapted.negative,
            };
            writeCache(aiFp, snapshot);
            applyCachedToClone(clone, snapshot);
          } else {
            applyDeterministicToClone(clone, aiRecipes, isNai);
            notifyFallbackOnce(aiFp, "prompt_assistant.recipe_silent_fallback");
          }
        }
      } catch (e) {
        const msg = String(e);
        if (msg.includes(BLOCKED_MINOR_CONTENT)) {
          throw new Error(mapLlmError(msg));
        }
        console.warn("[enhanceRecipes] silent adapt failed, falling back to tags:", e);
        applyDeterministicToClone(clone, aiRecipes, isNai);
        notifyFallbackOnce(aiFp, "prompt_assistant.recipe_silent_fallback");
      } finally {
        enhanceRecipes.isApplyingAtGenerate = false;
      }
    }
  }

  if (tagRecipes.length > 0) {
    applyDeterministicToClone(clone, tagRecipes, isNai);
  }

  stripFinalConflicts(clone, matched);
  return clone;
}
