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
  h3IdleRewriteSystemPrompt,
  validateH3IdleResponse,
} from "../utils/h3Idle.js";
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
   * Rewrite prose into MiniMax H3's trained prompt format.
   *
   * Deliberately bypasses `enhance()`: that path is danbooru-tag machinery
   * (candidate retrieval, tag repair, reconciliation) and would mangle H3 prose.
   * This sends the format's own system prompt straight through instead.
   *
   * Small local models miss the format on the first try often enough to be the
   * normal case, so a failed check buys exactly one retry that quotes the
   * violated rule back. A second failure still returns the text, flagged, so the
   * caller can show it beside the manual template rather than claim success.
   */
  async enhanceForH3(
    prompt: string,
    ctx: H3PromptContext,
    idle = false,
  ): Promise<H3RewriteResult> {
    this.isGenerating = true;
    try {
      return await this.withStageListener(async () => {
        const system = idle
          ? h3IdleRewriteSystemPrompt(ctx)
          : h3RewriteSystemPrompt(ctx);
        const validate = idle ? validateH3IdleResponse : validateH3Response;
        const first = (
          await callExternalLlm(system, prompt, H3_MAX_TOKENS)
        ).trim();
        const check = validate(first, ctx);
        if (check.ok || !check.rule) {
          return { text: first, ok: check.ok, rule: check.rule };
        }
        const second = (
          await callExternalLlm(
            system,
            h3RetryInstruction(check.rule, first),
            H3_MAX_TOKENS,
          )
        ).trim();
        const recheck = validate(second, ctx);
        if (recheck.ok) return { text: second, ok: true, rule: null };
        // Prefer whichever attempt produced something; the second can come back
        // empty when the model gives up on the correction.
        return {
          text: second || first,
          ok: false,
          rule: recheck.rule ?? check.rule,
        };
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
