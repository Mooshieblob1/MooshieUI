/**
 * Sends Enhance chat messages to the prompt assistant and files the answers.
 *
 * The calls live here and not in `enhanceChat`: that is a feature store and
 * may not import the prompt assistant. Each flow builds its request the way
 * the old Enhance modal did (same options, references, memory notes, content
 * checks and validation), with the chat rules switched on so the model can
 * talk back. Every answer that arrives joins the conversation, which is also
 * the session sent back as history, so feedback on an answer the user has not
 * applied ("make her hair longer") revises that answer.
 */

import { connection } from "../stores/connection.svelte.js";
import {
  enhanceChat,
  type ChatAnswerMeta,
  type ChatRequest,
  type EnhanceFlow,
  type NaiEnhanceReference,
} from "../stores/enhanceChat.svelte.js";
import { enhancerSessions } from "../stores/enhancerSessions.svelte.js";
import { enhanceRecipes } from "../stores/enhanceRecipes.svelte.js";
import { gallery } from "../stores/gallery.svelte.js";
import { generation } from "../stores/generation.svelte.js";
import { locale } from "../stores/locale.svelte.js";
import { promptAssistant } from "../stores/promptAssistant.svelte.js";
import { savedCharacters } from "../stores/savedCharacters.svelte.js";
import { isLlmCancelled } from "./api.js";
import {
  buildPendingLearnedFacts,
  characterNotesBlock,
  memoryForRequest,
  parseLearnedLines,
  type LearnedMemory,
  type PendingLearnedFact,
} from "./characterMemory.js";
import { naiRequestTexts, promptRequestTexts, rateRequest, savedCharactersForRequest } from "./contentSafety.js";
import { joinReply } from "./enhanceChat.js";
import { withImageMarker } from "./enhancerSession.js";
import {
  detectRecipeCreationIntent,
  detectRecipeRefineIntent,
  formatRecipeChatSummary,
  matchTargetRecipeForRefine,
  matchTriggeredRecipes,
  renderUserRecipesDirective,
  stripLeakedRecipeSlugs,
  stripNeverUcTags,
  type EnhanceRecipe,
} from "./enhanceRecipes.js";
import { buildH3Context } from "./h3Prompt.js";
import { mapLlmError } from "./llmError.js";
import { extractNaiArtists, restoreNaiArtists } from "./naiArtists.js";
import { resolveNaiLanguage } from "./naiLanguage.js";
import { withOppositeOutfitUc } from "./naiOutfits.js";
import { serializeNaiFields } from "./naiParse.js";
import { NAI_UC_HEAVY, naiV5Variant } from "./novelaiModels.js";
import { naiSavedCharacterDirective } from "./naiSavedCharacters.js";

/**
 * Whether a message can go out: text, an image, or (general and H3) a
 * current prompt to polish with "Edit current prompt" ticked. V5 always needs
 * to be told what to write.
 */
export function canSend(flow: EnhanceFlow): boolean {
  if (enhanceChat.input.trim() !== "" || enhanceChat.references.length > 0) return true;
  return flow !== "nai" && generation.naiEnhanceIncludeExisting && !!generation.positivePrompt?.trim();
}

/** Send what is in the composer. */
export async function sendMessage(): Promise<void> {
  const flow = enhanceChat.flow;
  if (enhanceChat.busy || !canSend(flow)) return;
  const request: ChatRequest = {
    text: enhanceChat.input.trim(),
    references: enhanceChat.references.map((r) => ({ ...r })),
  };
  enhanceChat.input = "";
  enhanceChat.references = [];
  await run(flow, request);
}

/**
 * Ask the newest answer again: the same message, with the canon side switched
 * when `priority` is given. The old answer leaves the conversation first, so
 * the new one is written as if it had never been there.
 */
export async function redoLast(flow: EnhanceFlow, at: number, priority?: "saved" | "canon"): Promise<void> {
  const request = enhanceChat.meta[at]?.request;
  if (!request || enhanceChat.busy) return;
  if (priority) enhanceChat.canonPriority = priority;
  enhanceChat.takeBackLast(flow);
  await run(flow, request);
}

async function run(flow: EnhanceFlow, request: ChatRequest): Promise<void> {
  const tabId = enhanceChat.activeTabId;
  const signal = enhanceChat.beginRequest(flow, request.text, request.references, tabId);
  try {
    if (flow !== "h3" && detectRecipeCreationIntent(request.text)) {
      await runCreateRecipe(flow, request, signal, tabId);
      return;
    }
    if (flow !== "h3" && detectRecipeRefineIntent(request.text)) {
      const target = matchTargetRecipeForRefine(
        request.text,
        enhanceRecipes.recipes,
        new Set(enhanceRecipes.activeIds),
      );
      if (target) {
        await runRefineRecipe(flow, target, request, signal, tabId);
        return;
      }
    }
    if (flow === "general") await runGeneral(request, signal, tabId);
    else if (flow === "h3") await runH3(request, signal, tabId);
    else await runNai(request, signal, tabId);
  } catch (e) {
    // Stopped on purpose: nothing went wrong, so nothing to say.
    if (signal.aborted || isLlmCancelled(e)) return;
    console.error(`Enhance chat (${flow}) failed:`, e);
    enhanceChat.fail(flow, mapLlmError(String(e)), request, tabId);
  } finally {
    enhanceChat.endRequest(signal);
  }
}

async function runCreateRecipe(
  flow: EnhanceFlow,
  request: ChatRequest,
  signal: AbortSignal,
  tabId: string | null,
): Promise<void> {
  const recent = enhanceChat
    .exchanges(flow)
    .slice(-6)
    .map((ex) => ({ user: ex.user, reply: ex.reply, prompt: ex.prompt }));
  const images = request.references.map((r) => r.base64);
  const draft = await promptAssistant.createEnhanceRecipe(request.text, recent, signal, images);
  if (signal.aborted) return;
  if (!draft || !draft.intent) {
    enhanceChat.fail(flow, locale.t("prompt_assistant.recipe_distill_failed"), request, tabId);
    return;
  }
  const created = enhanceRecipes.add({
    name: draft.name || "Untitled recipe",
    slug: draft.slug || "",
    scope: draft.scope ?? "any",
    applyMode: draft.applyMode ?? "ai",
    intent: draft.intent,
    include: draft.include ?? "",
    avoid: draft.avoid ?? "",
    neverUc: draft.neverUc ?? "",
    context: draft.context ?? "",
    triggers: draft.triggers ?? [],
  });
  enhanceRecipes.setActive(created.id, true);
  const userText = withImageMarker(request.text, request.references.length, "reference");
  file(flow, request, userText, formatRecipeChatSummary(created), "", { createdRecipeId: created.id }, signal, tabId);
  gallery.showToast(locale.t("prompt_assistant.recipe_saved"), "success");
}

async function runRefineRecipe(
  flow: EnhanceFlow,
  target: EnhanceRecipe,
  request: ChatRequest,
  signal: AbortSignal,
  tabId: string | null,
): Promise<void> {
  const prevSnapshot: EnhanceRecipe = { ...target, triggers: [...target.triggers] };
  const images = request.references.map((r) => r.base64);
  const patch = await promptAssistant.refineEnhanceRecipe(target, request.text, signal, images);
  if (signal.aborted) return;
  if (!patch || !patch.intent) {
    enhanceChat.fail(flow, locale.t("prompt_assistant.recipe_refine_failed"), request, tabId);
    return;
  }
  const updated = enhanceRecipes.update(target.id, patch) ?? target;
  const userText = withImageMarker(request.text, request.references.length, "reference");
  file(flow, request, userText, formatRecipeChatSummary(updated), "", { updatedRecipeId: updated.id }, signal, tabId);
  gallery.showToast(locale.t("prompt_assistant.recipe_updated_chat"), "success", {
    actionLabel: locale.t("prompt_assistant.undo"),
    onAction: () => {
      enhanceRecipes.update(prevSnapshot.id, {
        name: prevSnapshot.name,
        slug: prevSnapshot.slug,
        scope: prevSnapshot.scope,
        intent: prevSnapshot.intent,
        include: prevSnapshot.include,
        avoid: prevSnapshot.avoid,
        neverUc: prevSnapshot.neverUc,
        context: prevSnapshot.context,
        triggers: prevSnapshot.triggers,
      });
    },
    durationMs: 8000,
  });
}

/**
 * Memory notes of the saved characters a request refers to, by id, when the
 * provider gets memory. `texts` is what the request says; `existing` the
 * prompt it edits, whose saved characters count as referred to.
 */
function memoryNotes(texts: string[], existing: string): Map<string, string> {
  if (!promptAssistant.canUseMemory || !enhanceChat.useMemory) return new Map();
  const picks = memoryForRequest(
    savedCharacters.withMemory(savedCharacters.currentCharacters),
    texts,
    existing,
    promptAssistant.memoryBudget,
  );
  return new Map(picks.map((p) => [p.id, p.notes]));
}

/**
 * Save what an answer worked out about the saved characters it was sent
 * (`sentIds`) into their memory, so the next prompt reads it instead of
 * working it out again. The toast offers Undo.
 */
function saveLearned(learned: LearnedMemory[], sentIds: Set<string>): void {
  if (learned.length === 0 || sentIds.size === 0) return;
  const cards = savedCharacters.currentCharacters.filter((c) => sentIds.has(c.id));
  const result = savedCharacters.learnMemory(learned, cards);
  if (!result) return;
  gallery.showToast(locale.t("characters.memory.learned", { list: result.saved.join(", ") }), "success", {
    actionLabel: locale.t("characters.undo"),
    onAction: result.undo,
    durationMs: 8000,
  });
}

/**
 * Build memory persistence metadata for an answer: external providers save
 * learned facts on Apply (with Undo), whereas bundled local models queue them
 * for explicit user review before saving.
 */
function learnedMeta(
  learned: LearnedMemory[],
  sentIds: Set<string>,
  canApply: boolean,
): Partial<ChatAnswerMeta> {
  if (!canApply || learned.length === 0 || sentIds.size === 0) return {};
  if (promptAssistant.usesExternal) {
    return { onApplied: () => saveLearned(learned, sentIds) };
  }
  const cards = savedCharacters.currentCharacters.filter((c) => sentIds.has(c.id));
  const pending = buildPendingLearnedFacts(learned, cards, (c) => savedCharacters.memoryOwner(c));
  if (pending.length === 0) return {};
  return {
    pendingMemory: pending,
    onSaveMemory: (kept: PendingLearnedFact[]) => {
      saveLearned(
        kept.map((item) => ({ name: item.ownerName, file: item.file, fact: item.fact })),
        sentIds,
      );
    },
  };
}

/** File an answer, or say it came back empty. */
function file(
  flow: EnhanceFlow,
  request: ChatRequest,
  user: string,
  reply: string,
  prompt: string,
  meta: ChatAnswerMeta,
  signal: AbortSignal,
  tabId: string | null,
): void {
  if (signal.aborted) return;
  if (!reply.trim() && !prompt.trim()) {
    enhanceChat.fail(flow, locale.t("prompt_assistant.couldnt_enhance"), request, tabId);
    return;
  }
  // The user side is what the session stores; a message of images alone has
  // only their marker, and an empty one (rewrite the current prompt) the text
  // the flow worked from.
  const at = enhanceChat.addAnswer(flow, user || request.text || "…", joinReply(reply, prompt), { ...meta, request }, tabId);
  if (at === null) enhanceChat.fail(flow, locale.t("prompt_assistant.couldnt_enhance"), request, tabId);
}

async function runGeneral(request: ChatRequest, signal: AbortSignal, tabId: string | null): Promise<void> {
  const { text, references: refs } = request;
  const existing = generation.naiEnhanceIncludeExisting ? (generation.positivePrompt ?? "") : null;
  // Saved characters the request refers to bring their memory notes. The
  // minor check reads the notes too, as it does for the V5 roster. Earlier
  // turns of the session are sent along, so they are rated too.
  const earlierUserTurns = enhancerSessions.turns("enhance", tabId ?? undefined).map((t) => t.user);
  const texts = promptRequestTexts(text, existing, earlierUserTurns);
  const notes = memoryNotes(texts, existing ?? "");
  const noted = savedCharactersForRequest(
    savedCharacters.currentCharacters
      .filter((c) => notes.has(c.id))
      .map((c) => ({ id: c.id, name: c.name, prompt: c.prompt, notes: notes.get(c.id) ?? "" })),
    rateRequest(texts),
    texts,
  );
  const recipeMatchTexts = promptRequestTexts(
    text,
    existing,
    earlierUserTurns.filter((u) => !detectRecipeCreationIntent(u) && !detectRecipeRefineIntent(u)),
  );
  const activeRecipes = matchTriggeredRecipes(
    enhanceRecipes.recipes,
    recipeMatchTexts,
    new Set(enhanceRecipes.activeIds),
  );
  const result = await promptAssistant.rewrite({
    text,
    existing,
    family: generation.modelFamily,
    opts: {
      length: enhanceChat.length,
      include_artists: enhanceChat.includeArtists,
      reference_labels: refs.map((r) => r.label),
      character_notes: characterNotesBlock(noted) || undefined,
      recipes: renderUserRecipesDirective(activeRecipes, "general") || undefined,
    },
    images: refs.map((r) => r.base64),
    signal,
    chat: true,
    tabId: tabId ?? undefined,
  });
  const cleanedPrompt = stripLeakedRecipeSlugs(result.prompt, activeRecipes);
  const sentIds = new Set(noted.map((n) => n.id));
  file("general", request, result.sessionUser, result.reply, cleanedPrompt, {
    appliedRecipes:
      activeRecipes.length > 0
        ? activeRecipes.map((r) => ({ id: r.id, name: r.name, slug: r.slug }))
        : undefined,
    ...learnedMeta(result.learned, sentIds, !!cleanedPrompt.trim()),
  }, signal, tabId);
}

async function runH3(request: ChatRequest, signal: AbortSignal, tabId: string | null): Promise<void> {
  const { text, references } = request;
  const result = await promptAssistant.enhanceForH3(
    text,
    buildH3Context({
      variant: generation.videoVariant,
      frames: generation.videoFrameLength,
      hasFirstFrame: !!generation.videoFirstFrame,
      hasLastFrame: !!generation.videoEffectiveLastFrame,
      referenceImageCount: generation.videoRefImageFilenames.length,
    }),
    generation.videoFirstFrame,
    {
      existing: generation.naiEnhanceIncludeExisting ? (generation.positivePrompt ?? "") : null,
      references: references.map((r) => ({ base64: r.base64, label: r.label })),
      signal,
      chat: true,
      tabId: tabId ?? undefined,
    },
  );
  file("h3", request, result.sessionUser, result.reply, result.text, {
    // A near-miss is still a better start than the prose it replaces, so it is
    // offered with the broken rule named. Idle mode is invisible until it
    // fires, so it is named too, or "the pose never changes" reads as a bug.
    notice: !result.text
      ? null
      : !result.ok
        ? { kind: "warning", text: locale.t("prompt_assistant.h3_format_warning", { rule: result.rule ?? "" }) }
        : result.idle
          ? { kind: "info", text: locale.t("prompt_assistant.h3_idle_applied") }
          : null,
  }, signal, tabId);
}

/**
 * The artist tags "Keep artists" carries through, read off the prompt box.
 * NovelAI writes them bare, so the artist index is loaded first when it can
 * be; without it only `artist:` tags are recognised.
 */
async function keptArtists(): Promise<string[]> {
  const prompt = generation.positivePrompt ?? "";
  if (!generation.naiEnhanceKeepArtists || !prompt.trim()) return [];
  if (connection.artistGalleryManifestUrl) {
    await gallery.loadArtistIndex(connection.artistGalleryManifestUrl);
  }
  return extractNaiArtists(prompt, gallery.artistIndexReady ? gallery.artistTagIndex : null);
}

async function runNai(request: ChatRequest, signal: AbortSignal, tabId: string | null): Promise<void> {
  const { text, references: refs } = request;
  const current = naiV5Variant(generation.checkpoint);
  if (!current) return;
  // Curate it switches the model to V5 Full on apply, so the answer targets
  // Full's budget from the start.
  const curate = enhanceChat.curate;
  const variant = curate ? "full" : current;
  const language = resolveNaiLanguage(generation.naiEnhanceLanguage, text);
  const boxes = generation.novelaiSettings.characters;
  const kept = await keptArtists();
  if (signal.aborted) return;
  // Ticked, the message is applied as an edit to these fields rather than
  // treated as the whole of the idea.
  const existing = generation.naiEnhanceIncludeExisting
    ? {
        base: generation.positivePrompt ?? "",
        uc: generation.negativePrompt ?? "",
        characters: boxes.map((c) => c.prompt ?? ""),
        characterUcs: boxes.map((c) => c.negative_prompt ?? ""),
      }
    : null;
  // Rated here as well as in the store, because only this side has the saved
  // characters themselves: an explicit request leaves out the ones drawn as
  // minors, and refuses when it names one.
  const earlierNaiTurns = enhancerSessions.turns("nai", tabId ?? undefined).map((t) => t.user);
  const texts = naiRequestTexts(text, existing, earlierNaiTurns);
  const notes = memoryNotes(texts, existing ? [existing.base, ...existing.characters].join("\n") : "");
  const roster = savedCharactersForRequest(
    savedCharacters.currentCharacters.map((c) => ({ ...c, notes: notes.get(c.id) ?? "" })),
    rateRequest(texts),
    texts,
  );
  const recipeMatchTexts = naiRequestTexts(
    text,
    existing,
    earlierNaiTurns.filter((u) => !detectRecipeCreationIntent(u) && !detectRecipeRefineIntent(u)),
  );
  const activeRecipes = matchTriggeredRecipes(
    enhanceRecipes.recipes,
    recipeMatchTexts,
    new Set(enhanceRecipes.activeIds),
  );
  const result = await promptAssistant.enhanceForNai(
    text,
    {
      variant,
      // Curate it applies the Heavy preset, so the answer skips what it covers.
      ucPreset: curate ? NAI_UC_HEAVY : generation.novelaiSettings.uc_preset,
      characterCount: boxes.length,
      existing,
      language,
      references: refs.map((r) => r.label),
      keptArtists: kept,
      minorRule: generation.naiEnhanceMinorRule,
      recipes: renderUserRecipesDirective(activeRecipes, "nai") || undefined,
      // NovelAI mode, so these are the characters saved under NovelAI.
      savedCharacters: naiSavedCharacterDirective(
        roster.map((c) => ({
          ...c,
          variantOf: c.parentId ? savedCharacters.characters.find((p) => p.id === c.parentId)?.name : undefined,
        })),
        enhanceChat.canonPriority,
      ),
    },
    refs.map((r: NaiEnhanceReference) => r.base64),
    signal,
    true,
    tabId ?? undefined,
  );
  const parsed = result.parsed;
  let prompt = "";
  if (parsed.base.trim()) {
    const cleanBase = stripLeakedRecipeSlugs(parsed.base, activeRecipes);
    const cleanUc = stripNeverUcTags(stripLeakedRecipeSlugs(parsed.uc, activeRecipes), activeRecipes);
    const cleanChars = parsed.characters.map((c) => stripLeakedRecipeSlugs(c, activeRecipes));
    // Each box's own UC: the answer's CHAR n UC line, or the box's current one
    // when it wrote none. Then the other characters' distinctive garments go
    // in automatically, so clothes cannot jump between similar characters even
    // when the answer forgot to keep them apart.
    const charUcs = withOppositeOutfitUc(
      cleanChars,
      cleanChars.map((_, i) => parsed.characterUcs[i] ?? boxes[i]?.negative_prompt ?? ""),
    ).map((u) => stripNeverUcTags(stripLeakedRecipeSlugs(u, activeRecipes), activeRecipes));
    // Stored cleaned, with the kept artists back in: the conversation shows
    // and replays the prompt Apply writes, not the raw answer.
    prompt = serializeNaiFields({
      base: restoreNaiArtists(cleanBase, kept),
      uc: cleanUc,
      characters: cleanChars,
      characterUcs: charUcs,
    });
  }
  const learned = parseLearnedLines(parsed.memory);
  const sentIds = new Set(roster.filter((c) => c.notes).map((c) => c.id));
  file("nai", request, result.sessionUser, result.reply, prompt, {
    variant,
    language,
    curate,
    note: parsed.note,
    problems: result.problems,
    canon: parsed.canon,
    appliedRecipes:
      activeRecipes.length > 0
        ? activeRecipes.map((r) => ({ id: r.id, name: r.name, slug: r.slug }))
        : undefined,
    ...learnedMeta(learned, sentIds, !!prompt && result.problems.length === 0),
  }, signal, tabId);
}
