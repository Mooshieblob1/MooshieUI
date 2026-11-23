import {
  detectLlmHardware,
  listLlmCatalog,
  llmStatus,
  downloadLlmModel,
  deleteLlmModel,
  unloadLlm,
  enhancePrompt,
  composePrompt,
  getLlmProvider,
  setLlmProvider,
  setLlmApiKey,
  setLlmModel,
  setLlmFallbackModels,
  setLlmBaseUrl,
  listExternalLlmModels,
  connectLlmOauth,
  setLlmXaiClient,
  callExternalLlm,
  isLlmCancelled,
  describeImage,
} from "../utils/api.js";
import { ipcListen } from "../utils/ipc.js";
import { enhancerSessions } from "./enhancerSessions.svelte.js";
import { CHAT_CLAUSE, splitReply } from "../utils/enhanceChat.js";
import {
  buildHistory,
  estimateTokens,
  withImageMarker,
  withSessionClause,
  LOCAL_CONTEXT_TOKENS,
  REMOTE_CONTEXT_TOKENS,
  RUST_SYSTEM_RESERVE_TOKENS,
  type EnhancerFlow,
  type HistoryBudget,
} from "../utils/enhancerSession.js";
import { cleanMusicWritingResponse, musicWritingProblems, musicWritingRequest } from "../utils/yue2Skill.js";
import { musicEditRequest, validateMusicEdit, type MusicEditContext, type MusicEditProposal } from "../utils/musicEdit.js";
import { musicReviewRequest, validateMusicReviewExplanation, type MusicReviewRecord } from "../utils/musicReview.js";
import type { MusicResult } from "../types/music.js";
import type { MusicReferenceContext, MusicReferenceDraft } from "../types/music.js";
import { referenceStyleRequest, validateReferenceStyle } from "../utils/musicReference.js";
import { savedStyleRequest, type SavedMusicStyle } from "../utils/musicStyleProfiles.js";
import type { AudioStyleTarget } from "../utils/musicAudioStyle.js";
import type { MusicWritingContext, MusicWritingTask } from "../utils/yue2Skill.js";
import {
  H3_MAX_TOKENS,
  h3RetryInstruction,
  h3EditUserPrompt,
  h3ReferenceNote,
  h3ReferenceManifest,
  h3RewriteSystemPrompt,
  validateH3Response,
} from "../utils/h3Prompt.js";
import type { H3PromptContext, H3RewriteResult } from "../utils/h3Prompt.js";
import {
  detectH3IdleIntent,
  h3IdleRewriteSystemPrompt,
  h3IdleUserPrompt,
  validateH3IdleResponse,
} from "../utils/h3Idle.js";
import {
  NAI_MAX_TOKENS,
  naiRewriteSystemPrompt,
  naiRewriteSystemPromptParts,
  naiUserPrompt,
} from "../utils/naiPrompt.js";
import type { NaiPromptContext, NaiVariant } from "../utils/naiPrompt.js";
import { naiRecipeDirective } from "../utils/naiRecipes.js";
import {
  RECIPE_AUTHOR_MAX_TOKENS,
  RECIPE_SILENT_ADAPT_MAX_TOKENS,
  generalSilentRecipeAdaptSystemPrompt,
  generalSilentRecipeAdaptUserPrompt,
  naiSilentRecipeAdaptSystemPrompt,
  naiSilentRecipeAdaptUserPrompt,
  parseGeneralAdaptedPrompt,
  parseRecipeDraft,
  recipeCreateUserPrompt,
  recipeDistillSystemPrompt,
  recipeRefineSystemPrompt,
  recipeRefineUserPrompt,
  type EnhanceRecipe,
  type EnhanceRecipeDraft,
} from "../utils/enhanceRecipes.js";
import { assertAllowedRequest, naiRequestTexts, promptRequestTexts } from "../utils/contentSafety.js";
import {
  naiRetryInstruction,
  normalizeNaiResponse,
  parseNaiResponse,
  validateNaiResponse,
} from "../utils/naiParse.js";
import type { NaiRewriteResult } from "../utils/naiParse.js";
import {
  NAI_SKILL_MAX_TOKENS,
  loadNaiSkill,
  naiSkillAuthoringSystem,
  naiSkillAuthoringUser,
  naiSkillKey,
  naiSystemWithSkill,
  sanitizeNaiSkill,
  saveNaiSkill,
} from "../utils/naiSkill.js";
import {
  H3_SKILL_MAX_TOKENS,
  h3SkillAuthoringSystem,
  h3SkillAuthoringUser,
  h3SkillKey,
  h3SystemWithSkill,
  loadH3Skill,
  sanitizeH3Skill,
  saveH3Skill,
} from "../utils/h3Skill.js";
import {
  CHARACTER_EXTRACT_RETRY,
  CHARACTER_UPDATE_RETRY,
  CHARACTER_CONVERT_RETRY,
  CHARACTER_NAMING_RETRY,
  CHARACTER_SAMPLING,
  applyCharacterNames,
  characterNamingRequest,
  characterConvertRequest,
  characterExtractRequest,
  characterUpdateRequest,
  groundExtracted,
  parseCharacterUpdate,
  parseExtractedCharacters,
  type CharacterPromptStyle,
  type CharacterUpdate,
  type ExtractedCharacter,
  type SavedCharacterHint,
} from "../utils/characterExtract.js";
import {
  CHARACTER_DETECT_RETRY,
  characterDetectRequest,
  parseDetectedCharacters,
  type DetectCandidate,
  type DetectedCharacter,
  type DetectTags,
} from "../utils/characterDetect.js";
import {
  CHARACTER_MERGE_RETRY,
  characterMergeRequest,
  parseCharacterMerge,
  type CharacterMergeInput,
  type CharacterMergeResult,
} from "../utils/characterMerge.js";
import {
  MEMORY_REMEMBER_RETRY,
  memoryBudgetFor,
  memoryContext,
  parseOtherFacts,
  parseRemember,
  rememberRequest,
  splitLearnedMemory,
  writeMemoryRequest,
  type LearnedMemory,
  type LinkedCharacter,
  type CharacterMemoryFile,
  type MemoryBudget,
  type MemoryChange,
} from "../utils/characterMemory.js";
import { expressionRequest, parseExpression, type PortraitPromptStyle } from "../utils/portraitExpression.js";

/** A memory request's suggestions: changes to this character's files, and facts for the other characters it read. */
export interface MemorySuggestions {
  changes: MemoryChange[];
  others: LearnedMemory[];
}
import type {
  LlmDeviceCode,
  LlmHardware,
  LlmCatalogEntry,
  LlmProviderState,
  LlmStatus,
  PromptAssistantOpts,
} from "../types/index.js";

interface DownloadProgress {
  filename: string;
  downloaded: number;
  total: number;
  done: boolean;
}

class PromptAssistantStore {
  hardware = $state<LlmHardware | null>(null);
  catalog = $state<LlmCatalogEntry[]>([]);
  status = $state<LlmStatus | null>(null);
  /** Auto-recommended model id from hardware (pre-selected in the modal). */
  recommendedModelId = $state<string | null>(null);
  /** User's current selection in the setup modal. */
  selectedModelId = $state<string | null>(null);

  isGenerating = $state(false);
  isDownloading = $state(false);
  downloadProgress = $state<DownloadProgress | null>(null);
  /** "loading_model" | "generating" | null */
  stage = $state<string | null>(null);

  setupModalOpen = $state(false);

  /**
   * External provider settings as Rust reports them. The API key never crosses
   * this boundary — only `api_key_configured` does — so nothing here is secret.
   */
  provider = $state<LlmProviderState | null>(null);
  /** Model ids from the provider's `/models` endpoint, empty until fetched. */
  externalModels = $state<string[]>([]);
  /** A provider mutation or model listing is in flight. */
  providerBusy = $state(false);
  oauthBusy = $state(false);
  /**
   * The code the xAI device sign-in is waiting on, or null when none is live.
   *
   * Held in the store rather than in the panel because the sign-in call does not
   * resolve until the user approves it, so the code has to arrive out of band
   * and stay on screen for as long as that takes.
   */
  deviceCode = $state<LlmDeviceCode | null>(null);

  /** True once at least one model that can serve as the assistant is
   *  installed. A captioner only describes images, so it does not count. */
  get hasInstalledModel(): boolean {
    return this.assistantModels.length > 0;
  }

  /** Installed models that can serve as the assistant (not captioners). */
  get assistantModels(): string[] {
    return (this.status?.installed_models ?? []).filter(
      (id) => this.catalog.find((e) => e.id === id)?.purpose !== "captioner",
    );
  }

  /** Installed natural-language local models capable of reading character memory. */
  get naturalLanguageModels(): string[] {
    return this.assistantModels.filter(
      (id) => this.catalog.find((e) => e.id === id)?.purpose !== "tag_upsampler",
    );
  }

  /** Whether at least one bundled natural-language instruct model is installed. */
  get hasNaturalLanguageModel(): boolean {
    return this.naturalLanguageModels.length > 0;
  }

  /** Active or selected bundled natural-language model id, if any. */
  get activeLocalModelId(): string | null {
    const nl = this.naturalLanguageModels;
    const active = this.status?.active_model;
    if (active && nl.includes(active)) return active;
    if (this.selectedModelId && nl.includes(this.selectedModelId)) return this.selectedModelId;
    return nl[0] ?? null;
  }

  /** Catalog models that can see images, for Describe in Interrogate. */
  get visionCatalog(): LlmCatalogEntry[] {
    return this.catalog.filter((e) => e.variants.length > 0 && e.variants.every((v) => !!v.mmproj));
  }

  /** Whether a bundled model that sees images is installed. */
  get hasVisionModel(): boolean {
    const installed = this.status?.installed_models ?? [];
    return this.visionCatalog.some((e) => installed.includes(e.id));
  }

  /** Active route for `Describe image` (external vision, bundled JoyCaption/VLM, or none). */
  get describeTarget():
    | { kind: "external" }
    | { kind: "local"; modelId: string; name: string }
    | { kind: "none" }
    | null {
    const st = this.status;
    if (!st) return null;
    if (st.external_enabled && st.external_vision !== false) {
      return { kind: "external" };
    }
    const localId = st.describe_model ?? null;
    if (localId) {
      const entry = this.catalog.find((e) => e.id === localId);
      return { kind: "local", modelId: localId, name: entry?.name ?? localId };
    }
    return { kind: "none" };
  }

  /** True when the assistant can run: a local model is installed, or an external
   *  OpenAI-compatible endpoint is configured. Gates the enhance/compose actions. */
  get isAvailable(): boolean {
    return this.hasInstalledModel || !!this.status?.external_enabled;
  }

  /**
   * Whether the assistant reads and suggests character memory files: enabled
   * for external providers and bundled natural-language local models (with
   * tighter context budgets and review-before-save on local models).
   */
  get canUseMemory(): boolean {
    return this.usesExternal || this.hasNaturalLanguageModel;
  }

  /** Character memory context budget for the active backend and model. */
  get memoryBudget(): MemoryBudget {
    return memoryBudgetFor(this.usesExternal, this.activeLocalModelId);
  }

  /** Launch-time bootstrap: detect hardware, load catalog + status, pre-select. */
  async init(): Promise<void> {
    try {
      const [hw, cat, st] = await Promise.all([
        detectLlmHardware(),
        listLlmCatalog(),
        llmStatus(),
      ]);
      this.hardware = hw;
      this.catalog = [...cat];
      this.status = st;
      this.recommendedModelId = hw.recommended_model_id;
      // Pre-select: installed model > recommended.
      this.selectedModelId =
        this.assistantModels[0] ?? hw.recommended_model_id ?? null;
    } catch (e) {
      console.warn("[promptAssistant] init failed", e);
    }
    // Kept out of the batch above: the provider commands are moderator-gated in
    // browser mode, so a regular web client's rejection must not take hardware,
    // catalog and status down with it.
    await this.loadProvider();
  }

  async refreshStatus(): Promise<void> {
    try {
      this.status = await llmStatus();
    } catch (e) {
      console.warn("[promptAssistant] status refresh failed", e);
    }
  }

  /** Default variant key for a model id given current hardware. */
  defaultVariantKey(modelId: string): string {
    const entry = this.catalog.find((e) => e.id === modelId);
    if (!entry) return "gguf:Q4_K_M";
    const vram = this.hardware?.total_vram_mb ?? 0;
    // Largest GGUF that fits, else smallest GGUF.
    const fitting = entry.variants
      .filter((v) => v.format === "gguf" && v.vram_mb <= vram)
      .sort((a, b) => b.vram_mb - a.vram_mb);
    const chosen = fitting[0] ?? entry.variants.find((v) => v.format === "gguf");
    return chosen?.quant ? `gguf:${chosen.quant}` : "gguf:Q4_K_M";
  }

  async download(modelId: string, variantKey: string, select = true): Promise<void> {
    this.isDownloading = true;
    this.downloadProgress = null;
    const unlisten = await ipcListen("llm:download_progress", (event: any) => {
      const p = event.payload as DownloadProgress;
      this.downloadProgress = p.done ? null : p;
    });
    try {
      await downloadLlmModel(modelId, variantKey, select);
      await this.refreshStatus();
    } finally {
      unlisten();
      this.isDownloading = false;
      this.downloadProgress = null;
    }
  }

  async deleteModel(modelId: string): Promise<void> {
    await deleteLlmModel(modelId);
    await this.refreshStatus();
  }

  async unload(): Promise<void> {
    await unloadLlm();
    await this.refreshStatus();
  }

  /**
   * Adopt a provider snapshot returned by any of the setters. `enabled` mirrors
   * `llm_external_enabled`, which gates `isAvailable`, so it is folded into the
   * cached status rather than costing another round trip.
   */
  private applyProvider(next: LlmProviderState): void {
    this.provider = next;
    if (this.status && this.status.external_enabled !== next.enabled) {
      this.status = { ...this.status, external_enabled: next.enabled };
    }
  }

  /** Non-fatal: the provider commands are moderator-gated in browser mode. */
  async loadProvider(): Promise<void> {
    try {
      this.applyProvider(await getLlmProvider());
    } catch (e) {
      console.warn("[promptAssistant] provider load failed", e);
    }
  }

  private async mutateProvider(
    fn: () => Promise<LlmProviderState>,
  ): Promise<void> {
    this.providerBusy = true;
    try {
      this.applyProvider(await fn());
    } finally {
      this.providerBusy = false;
    }
  }

  /** Switching providers clears the stored key server-side; drop the stale list. */
  async selectProvider(id: string): Promise<void> {
    this.externalModels = [];
    await this.mutateProvider(() => setLlmProvider(id));
  }

  /** An empty key clears it and disables the external path. */
  async saveApiKey(apiKey: string): Promise<void> {
    await this.mutateProvider(() => setLlmApiKey(apiKey));
  }

  async saveModel(model: string): Promise<void> {
    await this.mutateProvider(() => setLlmModel(model));
  }

  async saveFallbackModels(models: string[]): Promise<void> {
    await this.mutateProvider(() => setLlmFallbackModels(models));
  }

  async saveBaseUrl(baseUrl: string): Promise<void> {
    this.externalModels = [];
    await this.mutateProvider(() => setLlmBaseUrl(baseUrl));
  }

  /**
   * Sign in to a provider. Resolves only once the user finishes on the
   * provider's page, so this stays in flight for as long as they take.
   *
   * OpenRouter and Nous are desktop only, because their PKCE redirect lands on a
   * loopback listener bound wherever Rust runs. `xai-oauth` uses the device
   * grant instead, which has no redirect, so it also works in browser mode and
   * publishes a code the user has to type; that arrives on `llm:device_code`
   * while this call is still blocked, and is cleared however the flow ends.
   */
  async connectOauth(id: string): Promise<void> {
    this.deviceCode = null;
    const unlisten = await ipcListen("llm:device_code", (event: any) => {
      this.deviceCode = event.payload as LlmDeviceCode;
    });
    this.oauthBusy = true;
    try {
      await this.mutateProvider(() => connectLlmOauth(id));
    } finally {
      unlisten();
      this.deviceCode = null;
      this.oauthBusy = false;
    }
  }

  /**
   * Store the xAI OAuth client id, and a scope override when one is given.
   *
   * Not a secret, and not part of the session: signing out leaves it in place,
   * because it describes how this install identifies itself rather than who is
   * signed in.
   */
  async saveXaiClient(clientId: string, scope: string): Promise<void> {
    await this.mutateProvider(() => setLlmXaiClient(clientId, scope));
  }

  /** Turns the model field into a picker. Silent on failure — many self-hosted
   *  endpoints implement chat completions without `/models`. */
  async refreshExternalModels(): Promise<void> {
    this.providerBusy = true;
    try {
      this.externalModels = [...(await listExternalLlmModels())];
    } catch (e) {
      console.warn("[promptAssistant] model listing failed", e);
      this.externalModels = [];
    } finally {
      this.providerBusy = false;
    }
  }

  private async withStageListener<T>(fn: () => Promise<T>): Promise<T> {
    const unlisten = await ipcListen("llm:stage", (event: any) => {
      this.stage = event.payload as string;
    });
    try {
      return await fn();
    } finally {
      unlisten();
      this.stage = null;
    }
  }

  /** Describe an interrogated image in words (see `describeImage`). */
  async describe(imageToken: string, length: "short" | "detailed"): Promise<string> {
    if (this.isGenerating) throw new Error("busy_generation");
    this.isGenerating = true;
    const probePending = !!this.status?.external_enabled && this.status.external_vision == null;
    try {
      return await this.withStageListener(() => describeImage(imageToken, length));
    } finally {
      this.isGenerating = false;
      if (probePending) void this.refreshStatus();
    }
  }

  /** Inject YuE2's writing skill through the existing desktop/server LLM route. */
  async writeForMusic(task: MusicWritingTask, context: MusicWritingContext): Promise<string> {
    if (this.isGenerating) throw new Error("busy_generation");
    const request = musicWritingRequest(task, context);
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        let prompt = request.prompt;
        for (let attempt = 0; attempt < 2; attempt++) {
          const text = cleanMusicWritingResponse(await callExternalLlm(request.system, prompt, request.maxTokens));
          const problems = musicWritingProblems(text, task, context.maxDuration);
          if (problems.length === 0) return text;
          prompt = `${request.prompt}\n\nThe previous attempt did not satisfy the writing contract. Write a fresh corrected result:\n${problems.join("\n")}`;
        }
        throw new Error("invalid_music_writing");
      });
    } finally {
      this.isGenerating = false;
    }
  }

  async styleFromReference(reference: MusicReferenceContext, context: MusicWritingContext, isCurrent: () => boolean = () => true): Promise<MusicReferenceDraft> {
    const request = referenceStyleRequest(reference, context);
    return this.requestStyleDraft(request, context.maxDuration, isCurrent);
  }

  async adaptSavedMusicStyle(saved: SavedMusicStyle, target: AudioStyleTarget, isCurrent: () => boolean): Promise<MusicReferenceDraft> {
    return this.requestStyleDraft(savedStyleRequest(saved, target), target.max_duration, isCurrent);
  }

  private async requestStyleDraft(request: { system: string; prompt: string; maxTokens: number }, duration: number, isCurrent: () => boolean): Promise<MusicReferenceDraft> {
    if (this.isGenerating) throw new Error("busy_generation");
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        let prompt = request.prompt;
        for (let attempt = 0; attempt < 2; attempt++) {
          if (!isCurrent()) throw new Error("music_reference_cancelled");
          const text = await callExternalLlm(request.system, prompt, request.maxTokens);
          if (!isCurrent()) throw new Error("music_reference_cancelled");
          try { return validateReferenceStyle(text, duration); }
          catch (error) {
            if (String(error).includes("music_reference_unknown") || attempt === 1) throw error;
            prompt = request.prompt + "\nReturn a corrected JSON object with status, style and estimates, following the original constraints. If uncertain about the recording, return status unknown.";
          }
        }
        throw new Error("invalid_music_reference");
      });
    } finally { this.isGenerating = false; }
  }

  async editForMusic(context: MusicEditContext, isCurrent: () => boolean = () => true): Promise<MusicEditProposal> {
    if (this.isGenerating) throw new Error("busy_generation");
    const request = musicEditRequest(context);
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        let prompt = request.prompt;
        for (let attempt = 0; attempt < 2; attempt++) {
          if (!isCurrent()) throw new Error("music_edit_cancelled");
          const text = await callExternalLlm(request.system, prompt, request.maxTokens);
          if (!isCurrent()) throw new Error("music_edit_cancelled");
          try { return validateMusicEdit(text, context); }
          catch (error) {
            if (attempt === 1) throw error;
            prompt = `${request.prompt}\n\nValidation rejected the previous proposal. Return a complete corrected JSON object satisfying the original constraints:\n${String(error).slice(0, 3000)}`;
          }
        }
        throw new Error("invalid_music_edit");
      });
    } finally { this.isGenerating = false; }
  }

  async reviewForMusic(record: MusicReviewRecord, song: MusicResult, language: string, isCurrent: () => boolean = () => true) {
    if (this.isGenerating) throw new Error("busy_generation");
    const request = musicReviewRequest(record, song, language);
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        let prompt = request.prompt;
        for (let attempt = 0; attempt < 2; attempt++) {
          if (!isCurrent()) throw new Error("music_review_cancelled");
          const text = await callExternalLlm(request.system, prompt, request.maxTokens);
          if (!isCurrent()) throw new Error("music_review_cancelled");
          try { return validateMusicReviewExplanation(text, record.report); }
          catch (error) {
            if (attempt === 1) throw error;
            prompt = `${request.prompt}\nReturn a corrected JSON object. Every issue must reference an existing evidence_id. Do not add prose outside JSON.`;
          }
        }
        throw new Error("invalid_music_review");
      });
    } finally { this.isGenerating = false; }
  }

  /**
   * Pull the characters out of a prompt and its negative prompt (UC) for the
   * Characters tab. One retry when
   * the reply is not the JSON asked for; an empty list means the model found
   * no characters. `saved` lists the cards already saved, so the model can
   * say which extracted character is one of them.
   */
  async extractCharacters(
    prompt: string,
    negative: string,
    saved: SavedCharacterHint[] = [],
  ): Promise<ExtractedCharacter[]> {
    if (this.isGenerating) throw new Error("busy_generation");
    assertAllowedRequest([prompt]);
    const request = characterExtractRequest(prompt, negative, saved);
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        let user = request.prompt;
        for (let attempt = 0; attempt < 2; attempt++) {
          const parsed = parseExtractedCharacters(
            await callExternalLlm(request.system, user, request.maxTokens, null, null, [], undefined, CHARACTER_SAMPLING),
          );
          // Held to the prompt: what the prompt does not say is dropped.
          if (parsed) return groundExtracted(parsed, prompt, negative);
          user = `${request.prompt}\n\n${CHARACTER_EXTRACT_RETRY}`;
        }
        throw new Error("invalid_character_extract");
      });
    } finally {
      this.isGenerating = false;
    }
  }

  /**
   * Name characters already read from NovelAI character boxes (and say which
   * saved card each one is, if any). The prompts themselves are never sent
   * back changed. One retry when the reply is not the JSON asked for.
   */
  async nameCharacters(
    characters: ExtractedCharacter[],
    saved: SavedCharacterHint[] = [],
  ): Promise<ExtractedCharacter[]> {
    if (this.isGenerating) throw new Error("busy_generation");
    assertAllowedRequest(characters.map((c) => c.prompt));
    const request = characterNamingRequest(characters, saved);
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        let user = request.prompt;
        for (let attempt = 0; attempt < 2; attempt++) {
          const named = applyCharacterNames(
            await callExternalLlm(request.system, user, request.maxTokens, null, null, [], undefined, CHARACTER_SAMPLING),
            characters,
          );
          if (named) return named;
          user = `${request.prompt}\n\n${CHARACTER_NAMING_RETRY}`;
        }
        throw new Error("invalid_character_extract");
      });
    } finally {
      this.isGenerating = false;
    }
  }

  /**
   * Find which of `candidates` (saved characters) are in an interrogated
   * image, from the tagger's tags, for the interrogator's suggestions. One
   * retry when the reply is not the JSON asked for; an empty list means none
   * matched.
   */
  async detectCharacters(tags: DetectTags, candidates: DetectCandidate[]): Promise<DetectedCharacter[]> {
    if (this.isGenerating) throw new Error("busy_generation");
    const request = characterDetectRequest(tags, candidates);
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        let user = request.prompt;
        for (let attempt = 0; attempt < 2; attempt++) {
          const parsed = parseDetectedCharacters(
            await callExternalLlm(request.system, user, request.maxTokens),
            tags,
            candidates,
          );
          if (parsed) return parsed;
          user = `${request.prompt}\n\n${CHARACTER_DETECT_RETRY}`;
        }
        throw new Error("invalid_character_detect");
      });
    } finally {
      this.isGenerating = false;
    }
  }

  /**
   * Refresh one saved character from the current prompt and UC for the
   * Characters tab's Update. "missing" when the prompt does not contain that
   * character. One retry when the reply is not the JSON asked for.
   */
  async updateCharacter(
    character: { name: string; prompt: string; negative: string },
    prompt: string,
    negative: string,
    memory: readonly CharacterMemoryFile[] | null = null,
  ): Promise<CharacterUpdate | "missing"> {
    if (this.isGenerating) throw new Error("busy_generation");
    const files = memory && this.canUseMemory ? memory : null;
    assertAllowedRequest([character.prompt, prompt], [files ? memoryContext(files, 0) : ""]);
    const request = characterUpdateRequest(character, prompt, negative, files);
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        let user = request.prompt;
        for (let attempt = 0; attempt < 2; attempt++) {
          const parsed = parseCharacterUpdate(
            await callExternalLlm(request.system, user, request.maxTokens, null, null, [], undefined, CHARACTER_SAMPLING),
            character.negative,
            files,
          );
          if (parsed) return parsed;
          user = `${request.prompt}\n\n${CHARACTER_UPDATE_RETRY}`;
        }
        throw new Error("invalid_character_update");
      });
    } finally {
      this.isGenerating = false;
    }
  }

  /**
   * Rewrite a saved character for another architecture's prompt style, for
   * the Characters tab's Copy to. One retry when the reply is not the JSON
   * asked for.
   */
  async convertCharacter(
    character: { name: string; prompt: string; negative: string },
    from: string,
    to: string,
    style: CharacterPromptStyle,
  ): Promise<CharacterUpdate> {
    if (this.isGenerating) throw new Error("busy_generation");
    assertAllowedRequest([character.prompt]);
    const request = characterConvertRequest(character, from, to, style);
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        let user = request.prompt;
        for (let attempt = 0; attempt < 2; attempt++) {
          const parsed = parseCharacterUpdate(
            await callExternalLlm(request.system, user, request.maxTokens, null, null, [], undefined, CHARACTER_SAMPLING),
            character.negative,
          );
          if (parsed && parsed !== "missing") return parsed;
          user = `${request.prompt}\n\n${CHARACTER_CONVERT_RETRY}`;
        }
        throw new Error("invalid_character_convert");
      });
    } finally {
      this.isGenerating = false;
    }
  }

  /**
   * Work a saved character into the current prompt for the Characters tab's
   * Use. The caller reviews the result before writing anything. One retry when
   * the reply is not the JSON asked for.
   */
  async mergeCharacter(input: CharacterMergeInput): Promise<CharacterMergeResult> {
    if (this.isGenerating) throw new Error("busy_generation");
    if (!this.canUseMemory) input = { ...input, memory: "" };
    assertAllowedRequest([input.prompt, input.character.prompt, ...(input.box?.boxes ?? [])], [input.memory ?? ""]);
    const request = characterMergeRequest(input);
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        let user = request.prompt;
        for (let attempt = 0; attempt < 2; attempt++) {
          const parsed = parseCharacterMerge(
            await callExternalLlm(request.system, user, request.maxTokens),
            input.box !== null,
            input.negative,
          );
          if (parsed) return parsed;
          user = `${request.prompt}\n\n${CHARACTER_MERGE_RETRY}`;
        }
        throw new Error("invalid_character_merge");
      });
    } finally {
      this.isGenerating = false;
    }
  }

  /**
   * File something the user wants remembered into a character's memory
   * files. Returns the suggested changes for the user to review; an empty
   * list means it is already there. One retry when the reply is not the JSON
   * asked for.
   */
  async rememberForCharacter(
    character: { name: string; prompt: string; negative: string },
    files: readonly CharacterMemoryFile[],
    fact: string,
    linked: readonly LinkedCharacter[] = [],
  ): Promise<MemorySuggestions> {
    if (!fact.trim()) throw new Error("invalid_character_memory");
    return this.memoryChanges(
      [fact],
      [character.prompt, memoryContext(files, 0), ...linked.map((c) => `${c.prompt}\n${c.notes}`)],
      rememberRequest(character, files, fact, linked),
      files,
    );
  }

  /**
   * Memory files written by the external provider from canon, the card's
   * prompt and `brief` (what the user wants covered, may be empty), as
   * changes to review.
   */
  async writeCharacterMemory(
    character: { name: string; prompt: string; negative: string },
    files: readonly CharacterMemoryFile[],
    brief: string,
    linked: readonly LinkedCharacter[] = [],
  ): Promise<MemorySuggestions> {
    return this.memoryChanges(
      [brief],
      [character.prompt, memoryContext(files, 0), ...linked.map((c) => `${c.prompt}\n${c.notes}`)],
      writeMemoryRequest(character, files, brief, linked),
      files,
    );
  }

  /**
   * The facial expression for a portrait of a saved character, picked by the
   * external provider from their memory notes or canon.
   */
  async portraitExpression(
    character: { name: string; prompt: string },
    notes: string,
    style: PortraitPromptStyle,
  ): Promise<string> {
    if (this.isGenerating) throw new Error("busy_generation");
    if (!this.canUseMemory) throw new Error("memory_needs_external");
    // Only a face is asked for, so the card is background: it can make the
    // request a minor one, never a sexual one.
    assertAllowedRequest([], [character.prompt, notes]);
    const request = expressionRequest(character, notes, style);
    this.isGenerating = true;
    try {
      const face = parseExpression(await callExternalLlm(request.system, request.prompt, request.maxTokens), style);
      if (!face) throw new Error("invalid_expression");
      return face;
    } finally {
      this.isGenerating = false;
    }
  }

  /**
   * Send a memory request and parse its suggested changes, and the matching
   * facts for other characters, retrying once. `texts` is what the user typed
   * for it; `background` is the card, its files and linked characters, which
   * can make the request a minor one but never a sexual one (see
   * `assertAllowedRequest`).
   */
  private async memoryChanges(
    texts: string[],
    background: string[],
    request: { system: string; prompt: string; maxTokens: number },
    files: readonly CharacterMemoryFile[],
  ): Promise<MemorySuggestions> {
    if (this.isGenerating) throw new Error("busy_generation");
    if (!this.canUseMemory) throw new Error("memory_needs_external");
    assertAllowedRequest(texts, background);
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        let user = request.prompt;
        for (let attempt = 0; attempt < 2; attempt++) {
          const reply = await callExternalLlm(request.system, user, request.maxTokens);
          const changes = parseRemember(reply, files);
          if (changes) return { changes, others: parseOtherFacts(reply) };
          user = `${request.prompt}\n\n${MEMORY_REMEMBER_RETRY}`;
        }
        throw new Error("invalid_character_memory");
      });
    } finally {
      this.isGenerating = false;
    }
  }

  /**
   * The general (non-V5) Enhance: three requests behind one box.
   *
   * - `existing` with no `text` polishes the current prompt (the old Enhance).
   * - `existing` with `text` applies `text` to it as a requested change.
   * - No `existing` writes a prompt from `text` (what Compose used to do).
   *
   * Returns the prompt and the user turn to record in the session; the caller
   * applies the prompt. All three share the `enhance` session, because to the
   * user it is one conversation in one modal.
   */
  async rewrite(req: {
    text: string;
    existing: string | null;
    family: string;
    opts: PromptAssistantOpts;
    images: string[];
    /** Aborting it stops the request on the backend too (see `runPromptAssistant`). */
    signal?: AbortSignal;
    /** From the Enhance chat: the answer may carry a reply, or be one alone. */
    chat?: boolean;
    /** Generation tab owning this request, so history reads/writes stay scoped to it. */
    tabId?: string;
  }): Promise<{ prompt: string; reply: string; sessionUser: string; learned: LearnedMemory[] }> {
    const text = req.text.trim();
    const existing = req.existing?.trim() || null;
    const images = req.images;
    // The same refusal the V5 rewrite makes, before anything is sent. Earlier
    // turns count because they go out as history with this one.
    assertAllowedRequest(
      promptRequestTexts(text, existing, enhancerSessions.turns("enhance", req.tabId).map((turn) => turn.user)),
    );
    this.isGenerating = true;
    try {
      const history = this.sessionHistory(
        "enhance",
        {
          systemTokens:
            RUST_SYSTEM_RESERVE_TOKENS +
            estimateTokens(req.opts.character_notes ?? "") +
            estimateTokens(req.opts.recipes ?? ""),
          userText: existing ? `${existing}\n${text}` : text,
          maxOutputTokens: req.chat ? 640 : 384,
        },
        req.tabId,
      );
      const opts = req.chat ? { ...req.opts, chat: true } : req.opts;
      const answer = await this.withStageListener(() =>
        existing
          ? enhancePrompt(
              existing,
              req.family,
              { ...opts, instruction: text || undefined },
              history.messages,
              images,
              req.signal,
            )
          : composePrompt(text, req.family, opts, history.messages, images, req.signal),
      );
      // The backend puts the chat reply first and has already split it off
      // before repairing the prompt; this only separates the two again.
      const { reply, prompt: answerPrompt } = req.chat
        ? splitReply(answer)
        : { reply: "", prompt: answer };
      // Facts to save to a saved character's memory come after the prompt
      // when character notes went out (see `character_notes`).
      const { prompt, learned } = splitLearnedMemory(answerPrompt);
      // The pasted current prompt is left out, like the V5 session: the next
      // turn sends whatever the prompt box holds by then.
      const sessionUser = withImageMarker(text || existing || "", images.length, "reference");
      return { prompt, reply, sessionUser, learned: req.opts.character_notes ? learned : [] };
    } finally {
      this.isGenerating = false;
    }
  }

  /** Context the history has to share: the local model's `-c`, or the API allowance. */
  private get historyContextTokens(): number {
    if (!this.usesExternal) return LOCAL_CONTEXT_TOKENS;
    // With "fall back to a local model" on, the same request can land on the
    // bundled server when the provider is down, so it has to fit there too.
    return this.provider?.fallback_local && this.hasInstalledModel
      ? LOCAL_CONTEXT_TOKENS
      : REMOTE_CONTEXT_TOKENS;
  }

  /**
   * Whether requests go to an external provider. A plain user of a shared
   * server cannot read the provider settings, so `provider` stays null for
   * them; the status, which everyone can read, answers then.
   */
  get usesExternal(): boolean {
    return this.provider ? this.provider.enabled : !!this.status?.external_enabled;
  }

  /** The flow's saved session, trimmed to fit, with the drop count published for the UI. */
  private sessionHistory(flow: EnhancerFlow, budget: Omit<HistoryBudget, "contextTokens">, tabId?: string) {
    const built = buildHistory(enhancerSessions.turns(flow, tabId), {
      ...budget,
      contextTokens: this.historyContextTokens,
    });
    enhancerSessions.noteDropped(flow, built.droppedTurns, tabId);
    return built;
  }

  /**
   * Which model an H3 skill was authored by, so one user's notes cannot leak
   * across a provider or model switch. `"local"` covers the bundled
   * llama-server, whose active model is the only thing that distinguishes it.
   */
  private get llmBackendId(): string {
    const p = this.provider;
    if (p?.enabled) return `${p.provider}/${p.model || "default"}`;
    // Provider settings unreadable (see `usesExternal`): still not the local model.
    if (this.usesExternal) return "external/default";
    return `local/${this.status?.active_model ?? this.status?.installed_models[0] ?? "default"}`;
  }

  /**
   * The current model's own notes on how to write an H3 prompt, authored once
   * per backend and cached from then on.
   *
   * One extra round trip the first time a given model is asked for a given
   * shape of the job, and none after that. Every failure returns `null` and the
   * rewrite proceeds on the format spec alone, which is what every rewrite did
   * before this existed. A model that answers with something unusable has that
   * recorded too, so it is not asked again on every prompt.
   */
  private async ensureH3Skill(
    ctx: H3PromptContext,
    hasImage: boolean,
    signal?: AbortSignal,
  ): Promise<string | null> {
    const key = h3SkillKey(this.llmBackendId, ctx, hasImage);
    const cached = loadH3Skill(key);
    if (cached !== null) return cached || null;
    try {
      const skill = sanitizeH3Skill(
        await callExternalLlm(
          h3SkillAuthoringSystem(),
          h3SkillAuthoringUser(ctx, hasImage),
          H3_SKILL_MAX_TOKENS,
          null,
          null,
          [],
          signal,
        ),
      );
      // Cached even when empty: that is a verdict on this model, not a miss.
      // A thrown error is not, so it deliberately skips this line.
      saveH3Skill(key, skill);
      return skill || null;
    } catch (e) {
      // Cancelled: stop here rather than carry on to the rewrite itself.
      if (isLlmCancelled(e)) throw e;
      console.warn("[promptAssistant] H3 skill authoring failed", e);
      return null;
    }
  }

  /**
   * Rewrite prose into MiniMax H3's trained prompt format.
   *
   * Deliberately bypasses `enhance()`: that path is danbooru-tag machinery
   * (candidate retrieval, tag repair, reconciliation) and would mangle H3 prose.
   * This sends the format's own system prompt straight through instead.
   *
   * `firstFrameFilename` is the frame uploaded to ComfyUI's input folder. When
   * the task type actually conditions on it and the model can see it, the
   * rewrite is written from the frame rather than from the user's text alone -
   * which is the difference between a description of this video and a
   * description of a video like it. The gate lives here rather than at the call
   * site so a frame left over from an earlier setup cannot follow the user into
   * a task that ignores it.
   *
   * Small local models miss the format on the first try often enough to be the
   * normal case, so a failed check buys exactly one retry that quotes the
   * violated rule back. A second failure still returns the text, flagged, so the
   * caller can show it beside the manual template rather than claim success.
   *
   * A prompt that asks for a Live2D-style idle loop swaps in a stricter contract
   * on both ends - extra rules in the system turn, extra checks on the way back.
   * The trigger is the user's own wording, so nothing about this mode exists
   * until somebody asks for it by name.
   */
  async enhanceForH3(
    prompt: string,
    ctx: H3PromptContext,
    firstFrameFilename?: string | null,
    extra: {
      /** The current prompt, when the request is a change to it. */
      existing?: string | null;
      /** Extra images from the Enhance modal: base64 plus optional label. */
      references?: { base64: string; label: string }[];
      /** Aborting it stops the request on the backend too. */
      signal?: AbortSignal;
      /** From the Enhance chat: the answer may carry a reply, or be one alone. */
      chat?: boolean;
      /** Generation tab owning this request, so history reads/writes stay scoped to it. */
      tabId?: string;
    } = {},
  ): Promise<H3RewriteResult & { sessionUser: string; reply: string }> {
    const usesFirstFrame =
      ctx.taskType === "i2va" || ctx.taskType === "fl2va";
    const image = (usesFirstFrame && firstFrameFilename) || null;
    // Only an external provider sees images, so the bundled model is not told
    // about any. The cap mirrors `MAX_VISION_IMAGES` in Rust: the first frame
    // takes a slot first, so the manifest names only images that arrive.
    const refs = this.usesExternal
      ? (extra.references ?? []).slice(0, image ? 3 : 4)
      : [];
    const text = prompt.trim();
    const existing = extra.existing?.trim() || "";
    // An empty box with the current prompt ticked is the old in-place
    // Enhance: rewrite that prompt into the format.
    const idea = text || existing;
    const idle = detectH3IdleIntent(idea);
    const validate = idle ? validateH3IdleResponse : validateH3Response;
    // Before the skill authoring call too, as in the V5 rewrite.
    assertAllowedRequest(
      promptRequestTexts(text, existing, enhancerSessions.turns("h3", extra.tabId).map((turn) => turn.user)),
    );

    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        const skill = await this.ensureH3Skill(ctx, !!image, extra.signal);
        const baseSystem =
          h3SystemWithSkill(
            idle
              ? h3IdleRewriteSystemPrompt(ctx, !!image)
              : h3RewriteSystemPrompt(ctx, !!image),
            skill,
          ) +
          (refs.length ? `\n\n${h3ReferenceNote(refs.length, !!image)}` : "") +
          (extra.chat ? `\n\n${CHAT_CLAUSE}` : "");
        const maxTokens = H3_MAX_TOKENS + (extra.chat ? 256 : 0);
        const request = idle ? h3IdleUserPrompt(idea, ctx) : idea;
        const user =
          (existing && text ? h3EditUserPrompt(existing, request) : request) +
          h3ReferenceManifest(refs.map((r) => r.label));
        const history = this.sessionHistory(
          "h3",
          {
            systemTokens: estimateTokens(withSessionClause(baseSystem, 1)),
            userText: user,
            maxOutputTokens: maxTokens,
          },
          extra.tabId,
        );
        const system = withSessionClause(baseSystem, history.sentTurns);
        const sessionUser = withImageMarker(
          withImageMarker(idea, image ? 1 : 0, "first-frame"),
          refs.length,
          "reference",
        );
        const imageData = refs.map((r) => r.base64);
        const split = (raw: string) =>
          extra.chat ? splitReply(raw) : { reply: "", prompt: raw.trim() };
        const firstAnswer = split(
          await callExternalLlm(
            system,
            user,
            maxTokens,
            image,
            imageData,
            history.messages,
            extra.signal,
          ),
        );
        const first = firstAnswer.prompt;
        const reply = firstAnswer.reply;
        // A reply with no prompt is an answer to a question: nothing to check.
        if (!first && reply) {
          return { text: "", ok: true, rule: null, idle, sessionUser, reply };
        }
        const check = validate(first, ctx);
        if (check.ok || !check.rule) {
          return { text: first, ok: check.ok, rule: check.rule, idle, sessionUser, reply };
        }
        const second = split(
          await callExternalLlm(
            system,
            h3RetryInstruction(check.rule, first),
            maxTokens,
            image,
            imageData,
            history.messages,
            extra.signal,
          ),
        ).prompt;
        const recheck = validate(second, ctx);
        if (recheck.ok) return { text: second, ok: true, rule: null, idle, sessionUser, reply };
        // Prefer whichever attempt produced something; the second can come back
        // empty when the model gives up on the correction.
        return {
          text: second || first,
          ok: false,
          rule: recheck.rule ?? check.rule,
          idle,
          sessionUser,
          reply,
        };
      });
    } finally {
      this.isGenerating = false;
    }
  }

  /**
   * The current model's own notes on how to write a V5 prompt, authored once
   * per backend and variant and cached from then on.
   *
   * Same bargain as `ensureH3Skill`: one extra round trip the first time, none
   * after that, and every failure falls back to running on the specification
   * alone. A model that answers with something unusable has that recorded too,
   * so it is not asked again before every rewrite.
   */
  /**
   * The working notes the backend wrote for itself for V5 prompts, the same
   * ones Enhance uses, so other V5 writers (the comic page) follow them too.
   */
  naiWorkingNotes(variant: NaiVariant, signal?: AbortSignal): Promise<string | null> {
    return this.ensureNaiSkill({ variant }, signal);
  }

  private async ensureNaiSkill(ctx: Pick<NaiPromptContext, "variant">, signal?: AbortSignal): Promise<string | null> {
    const key = naiSkillKey(this.llmBackendId, ctx.variant);
    const cached = loadNaiSkill(key);
    if (cached !== null) return cached || null;
    try {
      const skill = sanitizeNaiSkill(
        await callExternalLlm(
          naiSkillAuthoringSystem(),
          naiSkillAuthoringUser(ctx),
          NAI_SKILL_MAX_TOKENS,
          null,
          null,
          [],
          signal,
        ),
      );
      // Cached even when empty: that is a verdict on this model, not a miss.
      // A thrown error is not, so it deliberately skips this line.
      saveNaiSkill(key, skill);
      return skill || null;
    } catch (e) {
      if (isLlmCancelled(e)) throw e;
      console.warn("[promptAssistant] NAI skill authoring failed", e);
      return null;
    }
  }

  /**
   * Rewrite an idea into a NovelAI Diffusion V5 prompt.
   *
   * Bypasses `enhance()` for the same reason `enhanceForH3` does: that path is
   * danbooru-tag machinery, and V5 wants natural language plus its own emphasis
   * syntax and character boxes. Feeding V5 the tag soup that path produces makes
   * the image worse than the raw prompt the user typed.
   *
   * The result is never applied here. It comes back parsed into fields for the
   * review modal, because a V5 rewrite touches the prompt, the undesired content
   * and every character box at once, and overwriting hand-tuned character work
   * without asking is not recoverable by a single undo.
   *
   * Validator problems buy exactly one retry that quotes them back. Whatever the
   * second attempt returns is handed over anyway, with the surviving problems
   * attached, so the modal can warn rather than swallow the work.
   */
  async enhanceForNai(
    prompt: string,
    ctx: NaiPromptContext,
    referenceData: string[] = [],
    signal?: AbortSignal,
    /** From the Enhance chat: the answer may carry a reply, or be one alone. */
    chat = false,
    tabId?: string,
  ): Promise<NaiRewriteResult & { raw: string; sessionUser: string; reply: string }> {
    // Before anything is sent, the skill authoring call included. The rating
    // also picks the vocabulary block, so an SFW request never carries the
    // explicit tag list.
    const pastUsers = enhancerSessions.turns("nai", tabId).map((turn) => turn.user);
    ctx = {
      ...ctx,
      rating: assertAllowedRequest(
        naiRequestTexts(prompt, ctx.existing, pastUsers),
      ),
    };
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        const skill = await this.ensureNaiSkill(ctx, signal);
        const combineRecipes = (texts: string[]) =>
          [naiRecipeDirective(texts), ctx.recipes].filter(Boolean).join("\n\n");
        // Budgeted against every recorded turn, the most the session can send.
        const sessionCtx = {
          ...ctx,
          inSession: true,
          recipes: combineRecipes([
            prompt,
            ...pastUsers,
          ]),
        };
        // The chat rules go last, after the output contract they make an exception to.
        const chatClause = chat ? `\n\n${CHAT_CLAUSE}` : "";
        const maxTokens = NAI_MAX_TOKENS + (chat ? 256 : 0);
        const sessionParts = naiRewriteSystemPromptParts(sessionCtx);
        const sessionSystem =
          naiSystemWithSkill(
            [sessionParts.cached, sessionParts.dynamic].filter(Boolean).join("\n\n"),
            skill,
          ) + chatClause;
        const sessionUserTurn = naiUserPrompt(prompt, sessionCtx);
        const history = this.sessionHistory(
          "nai",
          {
            systemTokens: estimateTokens(sessionSystem),
            userText: sessionUserTurn,
            maxOutputTokens: maxTokens,
          },
          tabId,
        );
        // With history the turn is a revision of the newest answer; without it,
        // the ordinary rewrite from this text alone.
        const user =
          history.sentTurns > 0 ? sessionUserTurn : naiUserPrompt(prompt, { ...ctx, inSession: false });
        // The SESSION block only when history actually goes out: telling the
        // model about earlier turns it cannot see would be the opposite lie.
        const singleParts =
          history.sentTurns > 0
            ? sessionParts
            : naiRewriteSystemPromptParts({
                ...ctx,
                inSession: false,
                recipes: combineRecipes([prompt]),
              });
        const system =
          history.sentTurns > 0
            ? sessionSystem
            : naiSystemWithSkill(
                [singleParts.cached, singleParts.dynamic].filter(Boolean).join("\n\n"),
                skill,
              ) + chatClause;
        const systemCachePrefix = singleParts.cached;
        // Saved without the pasted current prompt: the next turn sends that fresh.
        const sessionUser = withImageMarker(prompt, referenceData.length, "reference");

        // Skill authoring above deliberately runs without the images: it asks
        // the model how it writes V5 prompts in general, and the answer is
        // cached per backend and variant, so feeding one user's references into
        // it would bias every later rewrite.
        const split = (raw: string) => (chat ? splitReply(raw) : { reply: "", prompt: raw });
        const firstAnswer = split(
          await callExternalLlm(
            system,
            user,
            maxTokens,
            null,
            referenceData,
            history.messages,
            signal,
            undefined,
            systemCachePrefix,
          ),
        );
        const firstRaw = firstAnswer.prompt;
        const reply = firstAnswer.reply;
        // A reply with no prompt answers a question: there are no fields to
        // parse, and parsing nothing would only produce a validator problem.
        if (!firstRaw.trim() && reply) {
          return { parsed: parseNaiResponse(""), problems: [], raw: "", sessionUser, reply };
        }
        const first = normalizeNaiResponse(parseNaiResponse(firstRaw));
        const problems = validateNaiResponse(first);
        if (problems.length === 0) {
          return { parsed: first, problems: [], raw: firstRaw, sessionUser, reply };
        }

        // The first answer goes back as a turn of its own, so the correction
        // is an edit of that text and not a second attempt from nothing, which
        // tends to fix the named problem and break something that was fine.
        const secondRaw = split(await callExternalLlm(
          system,
          naiRetryInstruction(problems),
          maxTokens,
          null,
          referenceData,
          [
            // The local context has no room for the session and the first
            // answer both, and the first answer already reflects the session.
            ...(this.historyContextTokens === LOCAL_CONTEXT_TOKENS ? [] : history.messages),
            { role: "user", content: user },
            { role: "assistant", content: firstRaw },
          ],
          signal,
          undefined,
          systemCachePrefix,
        )).prompt;
        const second = normalizeNaiResponse(parseNaiResponse(secondRaw));
        const recheck = validateNaiResponse(second);
        if (recheck.length === 0) {
          return { parsed: second, problems: [], raw: secondRaw, sessionUser, reply };
        }
        // Neither passed: keep the one with fewer problems, the correction on a
        // tie. The second can also come back empty when the model gives up.
        return second.base.trim() && recheck.length <= problems.length
          ? { parsed: second, problems: recheck, raw: secondRaw, sessionUser, reply }
          : { parsed: first, problems, raw: firstRaw, sessionUser, reply };
      });
    } finally {
      this.isGenerating = false;
    }
  }

  /** Author or distill a Concept Recipe from a plain-English description, chat, and/or reference images. */
  async createEnhanceRecipe(
    instruction: string,
    exchanges: { user: string; reply: string; prompt: string }[] = [],
    signal?: AbortSignal,
    referenceData: string[] = [],
  ): Promise<Partial<EnhanceRecipeDraft> | null> {
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        const raw = await callExternalLlm(
          recipeDistillSystemPrompt(),
          recipeCreateUserPrompt(instruction, exchanges),
          RECIPE_AUTHOR_MAX_TOKENS,
          null,
          referenceData.length > 0 ? referenceData : null,
          [],
          signal,
        );
        return parseRecipeDraft(raw);
      });
    } finally {
      this.isGenerating = false;
    }
  }

  /** Distill an Enhance chat sequence into a reusable Concept Recipe draft. */
  async distillEnhanceRecipe(
    exchanges: { user: string; reply: string; prompt: string }[],
    signal?: AbortSignal,
  ): Promise<Partial<EnhanceRecipeDraft> | null> {
    return this.createEnhanceRecipe("", exchanges, signal);
  }

  /** Refine an existing Concept Recipe based on user feedback about drift or failure. */
  async refineEnhanceRecipe(
    recipe: EnhanceRecipe,
    feedback: string,
    signal?: AbortSignal,
    referenceData: string[] = [],
  ): Promise<Partial<EnhanceRecipeDraft> | null> {
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        const raw = await callExternalLlm(
          recipeRefineSystemPrompt(),
          recipeRefineUserPrompt(recipe, feedback),
          RECIPE_AUTHOR_MAX_TOKENS,
          null,
          referenceData.length > 0 ? referenceData : null,
          [],
          signal,
        );
        return parseRecipeDraft(raw);
      });
    } finally {
      this.isGenerating = false;
    }
  }

  /**
   * Silently adapt an outgoing NovelAI V5 prompt to active Concept Recipes at
   * Generate time (without Enhance Chat history).
   */
  async adaptNaiWithRecipes(
    fields: { base: string; uc: string; characters: string[]; characterUcs: string[] },
    variant: NaiVariant,
    recipes: EnhanceRecipe[],
    signal?: AbortSignal,
  ): Promise<{ base: string; uc: string; characters: string[]; characterUcs: (string | null)[] } | null> {
    assertAllowedRequest(naiRequestTexts("", { base: fields.base, characters: fields.characters }, []));
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        const raw = await callExternalLlm(
          naiSilentRecipeAdaptSystemPrompt(recipes, variant),
          naiSilentRecipeAdaptUserPrompt(fields),
          RECIPE_SILENT_ADAPT_MAX_TOKENS,
          null,
          null,
          [],
          signal,
        );
        const parsed = normalizeNaiResponse(parseNaiResponse(raw));
        if (!parsed.base.trim()) return null;
        let base = parsed.base;
        const textMatch = fields.base.match(/\n[ \t]*Text:\s*[\s\S]*$/i);
        if (textMatch && !/^\s*Text:/im.test(base)) {
          base = `${base.trimEnd()}\n${textMatch[0]}`;
        }
        return {
          base,
          uc: parsed.uc,
          characters: parsed.characters,
          characterUcs: parsed.characterUcs,
        };
      });
    } finally {
      this.isGenerating = false;
    }
  }

  /**
   * Silently adapt an outgoing General (non-V5) prompt to active Concept
   * Recipes at Generate time (without Enhance Chat history).
   */
  async adaptGeneralWithRecipes(
    positive: string,
    negative: string,
    family: string,
    recipes: EnhanceRecipe[],
    signal?: AbortSignal,
  ): Promise<{ positive: string; negative: string } | null> {
    assertAllowedRequest(promptRequestTexts(positive, null, []));
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        const raw = await callExternalLlm(
          generalSilentRecipeAdaptSystemPrompt(recipes, family),
          generalSilentRecipeAdaptUserPrompt(positive, negative),
          RECIPE_SILENT_ADAPT_MAX_TOKENS,
          null,
          null,
          [],
          signal,
        );
        return parseGeneralAdaptedPrompt(raw);
      });
    } finally {
      this.isGenerating = false;
    }
  }
}

export const promptAssistant = new PromptAssistantStore();
