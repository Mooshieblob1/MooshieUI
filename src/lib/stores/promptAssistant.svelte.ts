import {
  detectLlmHardware,
  listLlmCatalog,
  llmStatus,
  downloadLlmModel,
  deleteLlmModel,
  unloadLlm,
  enhancePrompt,
  composePrompt,
} from "../utils/api.js";
import { ipcListen } from "../utils/ipc.js";
import type {
  LlmHardware,
  LlmCatalogEntry,
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

  /** True once at least one model is installed. */
  get hasInstalledModel(): boolean {
    return !!this.status && this.status.installed_models.length > 0;
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
