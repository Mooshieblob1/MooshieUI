import type { GenerationParams } from "../types/index.js";

export interface QueuedPrompt {
  promptId: string;
  mode: "txt2img" | "img2img" | "inpainting";
  wasUpscaled: boolean;
  params: GenerationParams;
}

class ProgressStore {
  /** All prompts submitted to ComfyUI that haven't completed yet. */
  pendingPrompts = $state<QueuedPrompt[]>([]);

  /** The prompt ComfyUI is currently executing (set from executing events). */
  activePromptId = $state<string | null>(null);

  currentStep = $state(0);
  totalSteps = $state(0);
  currentNode = $state<string | null>(null);
  previewImage = $state<string | null>(null);
  lastOutputImage = $state<string | null>(null);
  modeLastOutput = $state<{
    txt2img: string | null;
    img2img: string | null;
    inpainting: string | null;
  }>({
    txt2img: null,
    img2img: null,
    inpainting: null,
  });

  /** Which sampling pass we're on: 0 = not started, 1 = initial, 2 = upscale */
  samplingPass = $state(0);
  /** Tracks the last node that had progress, to detect pass changes */
  private _lastProgressNode: string | null = null;

  /** Persists wasUpscaled from the last completed prompt (for PreviewImage overlay). */
  private _lastCompletedWasUpscaled = $state(false);

  /** The seed used by the most recently completed generation. */
  lastCompletedSeed = $state<number | null>(null);

  /** Global queue position for this user's next prompt (0 = generating now). */
  queuePosition = $state<number | null>(null);
  /** Total prompts in the global queue across all users. */
  queueTotal = $state(0);

  /**
   * Server-wide generation activity broadcast to all users.
   * null when the server is idle. Set from `mooshie:server_progress` events.
   */
  serverProgress = $state<{ value: number; max: number } | null>(null);

  // --- Derived getters ---

  get isGenerating(): boolean {
    return this.pendingPrompts.length > 0;
  }

  get queueCount(): number {
    return this.pendingPrompts.length;
  }

  get activePrompt(): QueuedPrompt | undefined {
    return this.pendingPrompts.find((p) => p.promptId === this.activePromptId);
  }

  get currentPromptId(): string | null {
    return this.activePromptId;
  }

  get currentMode(): "txt2img" | "img2img" | "inpainting" {
    return this.activePrompt?.mode ?? "txt2img";
  }

  get wasUpscaled(): boolean {
    return this.activePrompt?.wasUpscaled ?? this._lastCompletedWasUpscaled;
  }

  get lastParams(): GenerationParams | null {
    return this.activePrompt?.params ?? null;
  }

  get percentage() {
    return this.totalSteps > 0
      ? (this.currentStep / this.totalSteps) * 100
      : 0;
  }

  get displayImage() {
    return this.previewImage ?? this.lastOutputImage;
  }

  get phaseLabel(): string {
    if (!this.isGenerating) return "";
    // Show queue position if waiting behind other users
    if (this.queuePosition != null && this.queuePosition > 0 && this.totalSteps === 0) {
      const pos = this.queuePosition;
      const own = this.queueCount;
      if (own > 1) return `Queue position #${pos + 1} (+${own - 1} of yours)`;
      return `Queue position #${pos + 1}`;
    }
    if (this.totalSteps === 0) {
      // If at position 0 but other users' prompts exist in the global queue,
      // ComfyUI may still be working on their prompt — show "In queue"
      if (this.queueTotal > this.queueCount) {
        return this.queueCount > 1 ? `In queue (${this.queueCount})` : "In queue...";
      }
      return this.queueCount > 1 ? `Queued (${this.queueCount})` : "Preparing...";
    }
    if (this.wasUpscaled && this.samplingPass >= 2) {
      return this.queueCount > 1 ? `Upscaling... (+${this.queueCount - 1} queued)` : "Upscaling...";
    }
    return this.queueCount > 1 ? `Generating... (+${this.queueCount - 1} queued)` : "Generating...";
  }

  setActiveMode(mode: "txt2img" | "img2img" | "inpainting") {
    this.lastOutputImage = this.modeLastOutput[mode];
  }

  setLastOutputForMode(mode: "txt2img" | "img2img" | "inpainting", image: string | null) {
    this.modeLastOutput = {
      ...this.modeLastOutput,
      [mode]: image,
    };
    this.lastOutputImage = image;
  }

  /** Replace a blob URL in lastOutputImage/modeLastOutput (used by embedTempMetadata). */
  replaceOutputUrl(oldUrl: string, newUrl: string) {
    if (this.lastOutputImage === oldUrl) {
      this.lastOutputImage = newUrl;
    }
    let changed = false;
    for (const [mode, url] of Object.entries(this.modeLastOutput)) {
      if (url === oldUrl) {
        (this.modeLastOutput as Record<string, string | null>)[mode] = newUrl;
        changed = true;
      }
    }
    if (changed) {
      this.modeLastOutput = { ...this.modeLastOutput };
    }
  }

  /** Called when a progress event arrives — detects pass transitions */
  updateProgress(step: number, max: number, node: string | null) {
    if (node && node !== this._lastProgressNode) {
      this._lastProgressNode = node;
      this.samplingPass += 1;
    }
    this.currentStep = step;
    this.totalSteps = max;
  }

  /** Called when a mooshie:queue_update event arrives with position info. */
  updateQueuePosition(promptId: string, position: number, total: number) {
    // Only update if this prompt is one of ours
    if (this.pendingPrompts.some((p) => p.promptId === promptId)) {
      // Use the lowest position among our pending prompts (the one closest to executing)
      if (this.queuePosition === null || position < this.queuePosition) {
        this.queuePosition = position;
      }
      this.queueTotal = total;
    }
  }

  /** Reset queue position tracking (called before a new batch of updates). */
  resetQueuePosition() {
    this.queuePosition = null;
    this.queueTotal = 0;
  }

  /** Add a new prompt to the queue. */
  enqueue(
    promptId: string,
    wasUpscaled: boolean = false,
    mode: "txt2img" | "img2img" | "inpainting" = "txt2img",
    params: GenerationParams | null = null,
  ) {
    this.pendingPrompts = [
      ...this.pendingPrompts,
      {
        promptId,
        mode,
        wasUpscaled,
        params: params!,
      },
    ];
  }

  /** Called when an executing event identifies which prompt is active. */
  setActivePrompt(promptId: string) {
    if (this.activePromptId !== promptId) {
      this.activePromptId = promptId;
      this.currentStep = 0;
      this.totalSteps = 0;
      this.previewImage = null;
      this.samplingPass = 0;
      this._lastProgressNode = null;
    }
  }

  /** Called when a prompt completes — removes it from the queue and returns its metadata. */
  completePrompt(promptId: string): QueuedPrompt | undefined {
    const item = this.pendingPrompts.find((p) => p.promptId === promptId);

    if (item) {
      this._lastCompletedWasUpscaled = item.wasUpscaled;
      if (item.params != null && item.params.seed != null && item.params.seed >= 0) {
        this.lastCompletedSeed = item.params.seed;
      }
    }

    this.pendingPrompts = this.pendingPrompts.filter((p) => p.promptId !== promptId);

    if (this.activePromptId === promptId) {
      this.activePromptId = null;
      this.currentStep = 0;
      this.totalSteps = 0;
      this.currentNode = null;
      this.samplingPass = 0;
      this._lastProgressNode = null;

      if (this.previewImage && item) {
        this.setLastOutputForMode(item.mode, this.previewImage);
      }
      this.previewImage = null;
    }

    return item;
  }

  /** Remove a specific prompt from the queue (e.g. on error). */
  removePrompt(promptId: string) {
    this.pendingPrompts = this.pendingPrompts.filter((p) => p.promptId !== promptId);
    if (this.activePromptId === promptId) {
      this.activePromptId = null;
    }
  }

  /** Cancel everything — interrupt + clear queue. */
  cancelAll() {
    this.pendingPrompts = [];
    this.activePromptId = null;
    this.currentStep = 0;
    this.totalSteps = 0;
    this.currentNode = null;
    this.previewImage = null;
    this.samplingPass = 0;
    this._lastProgressNode = null;
    this.queuePosition = null;
    this.queueTotal = 0;
  }

  /** Update server-wide progress (from mooshie:server_progress events). */
  updateServerProgress(value: number, max: number) {
    if (max <= 0) {
      this.serverProgress = null;
    } else {
      this.serverProgress = { value, max };
    }
  }

  /** Clear server-wide progress indicator (server went idle). */
  clearServerProgress() {
    this.serverProgress = null;
  }

  /**
   * Restore pending queue entries from a snapshot delivered on SSE reconnect.
   * Only adds entries that aren't already tracked (idempotent).
   */
  restoreFromSnapshot(promptIds: string[]) {
    for (const pid of promptIds) {
      if (!this.pendingPrompts.some((p) => p.promptId === pid)) {
        this.pendingPrompts = [
          ...this.pendingPrompts,
          { promptId: pid, mode: "txt2img", wasUpscaled: false, params: null as any },
        ];
      }
    }
  }

  // --- Backward-compat aliases ---

  /** @deprecated Use enqueue() instead. */
  startGeneration(
    promptId: string,
    upscaled: boolean = false,
    mode: "txt2img" | "img2img" | "inpainting" = "txt2img",
    params: GenerationParams | null = null,
  ) {
    this.enqueue(promptId, upscaled, mode, params);
  }

  /** @deprecated Use cancelAll() instead. */
  reset() {
    this.cancelAll();
  }
}

export const progress = new ProgressStore();
