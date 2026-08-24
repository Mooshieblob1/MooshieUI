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
  setLlmBaseUrl,
  listExternalLlmModels,
  connectLlmOauth,
  callExternalLlm,
} from "../utils/api.js";
import { ipcListen } from "../utils/ipc.js";
import {
  H3_MAX_TOKENS,
  h3RetryInstruction,
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
  naiUserPrompt,
} from "../utils/naiPrompt.js";
import type { NaiPromptContext } from "../utils/naiPrompt.js";
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
import type {
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
  composeModalOpen = $state(false);

  /**
   * External provider settings as Rust reports them. The API key never crosses
   * this boundary — only `api_key_configured` does — so nothing here is secret.
   */
  provider = $state<LlmProviderState | null>(null);
  /** Model ids from the provider's `/models` endpoint, empty until fetched. */
  externalModels = $state<string[]>([]);
  /** A provider mutation or model listing is in flight. */
  providerBusy = $state(false);

  /** True once at least one model is installed. */
  get hasInstalledModel(): boolean {
    return !!this.status && this.status.installed_models.length > 0;
  }

  /** True when the assistant can run: a local model is installed, or an external
   *  OpenAI-compatible endpoint is configured. Gates the enhance/compose actions. */
  get isAvailable(): boolean {
    return this.hasInstalledModel || !!this.status?.external_enabled;
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
        st.installed_models[0] ?? hw.recommended_model_id ?? null;
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

  async download(modelId: string, variantKey: string): Promise<void> {
    this.isDownloading = true;
    this.downloadProgress = null;
    const unlisten = await ipcListen("llm:download_progress", (event: any) => {
      const p = event.payload as DownloadProgress;
      this.downloadProgress = p.done ? null : p;
    });
    try {
      await downloadLlmModel(modelId, variantKey);
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

  async saveBaseUrl(baseUrl: string): Promise<void> {
    this.externalModels = [];
    await this.mutateProvider(() => setLlmBaseUrl(baseUrl));
  }

  /** Desktop only: the PKCE loopback listener binds on the user's own machine. */
  async connectOauth(id: string): Promise<void> {
    await this.mutateProvider(() => connectLlmOauth(id));
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

  /** Returns the cleaned/enhanced prompt string. Caller applies it. */
  async enhance(
    prompt: string,
    family: string,
    opts?: PromptAssistantOpts,
  ): Promise<string> {
    this.isGenerating = true;
    try {
      return await this.withStageListener(() =>
        enhancePrompt(prompt, family, opts),
      );
    } finally {
      this.isGenerating = false;
    }
  }

  /**
   * Which model an H3 skill was authored by, so one user's notes cannot leak
   * across a provider or model switch. `"local"` covers the bundled
   * llama-server, whose active model is the only thing that distinguishes it.
   */
  private get llmBackendId(): string {
    const p = this.provider;
    if (p?.enabled) return `${p.provider}/${p.model || "default"}`;
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
        ),
      );
      // Cached even when empty: that is a verdict on this model, not a miss.
      // A thrown error is not, so it deliberately skips this line.
      saveH3Skill(key, skill);
      return skill || null;
    } catch (e) {
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
  ): Promise<H3RewriteResult> {
    const usesFirstFrame =
      ctx.taskType === "i2va" || ctx.taskType === "fl2va";
    const image = (usesFirstFrame && firstFrameFilename) || null;
    const idle = detectH3IdleIntent(prompt);
    const validate = idle ? validateH3IdleResponse : validateH3Response;

    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        const skill = await this.ensureH3Skill(ctx, !!image);
        const system = h3SystemWithSkill(
          idle
            ? h3IdleRewriteSystemPrompt(ctx, !!image)
            : h3RewriteSystemPrompt(ctx, !!image),
          skill,
        );
        const user = idle ? h3IdleUserPrompt(prompt, ctx) : prompt;
        const first = (
          await callExternalLlm(system, user, H3_MAX_TOKENS, image)
        ).trim();
        const check = validate(first, ctx);
        if (check.ok || !check.rule) {
          return { text: first, ok: check.ok, rule: check.rule, idle };
        }
        const second = (
          await callExternalLlm(
            system,
            h3RetryInstruction(check.rule, first),
            H3_MAX_TOKENS,
            image,
          )
        ).trim();
        const recheck = validate(second, ctx);
        if (recheck.ok) return { text: second, ok: true, rule: null, idle };
        // Prefer whichever attempt produced something; the second can come back
        // empty when the model gives up on the correction.
        return {
          text: second || first,
          ok: false,
          rule: recheck.rule ?? check.rule,
          idle,
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
  private async ensureNaiSkill(ctx: NaiPromptContext): Promise<string | null> {
    const key = naiSkillKey(this.llmBackendId, ctx.variant);
    const cached = loadNaiSkill(key);
    if (cached !== null) return cached || null;
    try {
      const skill = sanitizeNaiSkill(
        await callExternalLlm(
          naiSkillAuthoringSystem(),
          naiSkillAuthoringUser(ctx),
          NAI_SKILL_MAX_TOKENS,
        ),
      );
      // Cached even when empty: that is a verdict on this model, not a miss.
      // A thrown error is not, so it deliberately skips this line.
      saveNaiSkill(key, skill);
      return skill || null;
    } catch (e) {
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
  ): Promise<NaiRewriteResult> {
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        const skill = await this.ensureNaiSkill(ctx);
        const system = naiSystemWithSkill(naiRewriteSystemPrompt(ctx), skill);
        const user = naiUserPrompt(prompt, ctx);

        const first = normalizeNaiResponse(
          parseNaiResponse(await callExternalLlm(system, user, NAI_MAX_TOKENS)),
        );
        const problems = validateNaiResponse(first, ctx);
        if (problems.length === 0) return { parsed: first, problems: [] };

        const second = normalizeNaiResponse(
          parseNaiResponse(
            await callExternalLlm(
              system,
              `${user}

${naiRetryInstruction(problems)}`,
              NAI_MAX_TOKENS,
            ),
          ),
        );
        const recheck = validateNaiResponse(second, ctx);
        if (recheck.length === 0) return { parsed: second, problems: [] };
        // Prefer whichever attempt produced a base prompt; the second can come
        // back empty when the model gives up on the correction.
        return second.base.trim()
          ? { parsed: second, problems: recheck }
          : { parsed: first, problems };
      });
    } finally {
      this.isGenerating = false;
    }
  }

  /** Returns the composed prompt string. Caller applies it. */
  async compose(
    description: string,
    family: string,
    opts?: PromptAssistantOpts,
  ): Promise<string> {
    this.isGenerating = true;
    try {
      return await this.withStageListener(() =>
        composePrompt(description, family, opts),
      );
    } finally {
      this.isGenerating = false;
    }
  }
}

export const promptAssistant = new PromptAssistantStore();
