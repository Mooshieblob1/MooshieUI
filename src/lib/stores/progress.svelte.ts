import type { GenerationParams } from "../types/index.js";
import { locale } from "./locale.svelte.js";

export interface QueuedPrompt {
  promptId: string;
  mode: "txt2img" | "img2img" | "inpainting";
  wasUpscaled: boolean;
  params: GenerationParams;
  enqueuedAt: number;
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

  /** Timestamp when the current active generation started (for elapsed time display). */
  generationStartTime = $state<number | null>(null);

  /** Live-updating elapsed seconds (ticked for real-time display during long generations like style transfer). */
  elapsedSeconds = $state(0);

  private _elapsedInterval: ReturnType<typeof setInterval> | null = null;

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

  get hasReliableStepProgress(): boolean {
    if (this.totalSteps <= 0) return false;
    // Style transfer pipelines (RFInversion + SamplerCustomAdvanced) can run
    // for a while without regular per-step progress events, which makes the
    // UI appear frozen at 1/N even though generation is still working.
    if (this.activePrompt?.params?.style_transfer_enabled && this.currentStep <= 1) {
      return false;
    }
    return true;
  }

  /** Elapsed time in ms since the current generation became active (for issue 252 style speed feedback). */
  get elapsedMs(): number | null {
    if (!this.generationStartTime) return null;
    return Date.now() - this.generationStartTime;
  }

  private _startElapsedTimer() {
    this._stopElapsedTimer();
    this.elapsedSeconds = 0;
    this._elapsedInterval = setInterval(() => {
      if (this.generationStartTime) {
        this.elapsedSeconds = Math.floor((Date.now() - this.generationStartTime) / 1000);
      }
    }, 1000);
  }

  private _stopElapsedTimer() {
    if (this._elapsedInterval) {
      clearInterval(this._elapsedInterval);
      this._elapsedInterval = null;
    }
    this.elapsedSeconds = 0;
  }

  get displayImage() {
    return this.previewImage ?? this.lastOutputImage;
  }

  /** Non-empty while a regional inpaint chain is between ComfyUI steps. */
  regionalChainStatus: string | null = null;

  setRegionalChainStatus(status: string | null) {
    this.regionalChainStatus = status;
  }

  get phaseLabel(): string {
    if (this.regionalChainStatus) return this.regionalChainStatus;
    if (!this.isGenerating) return "";
    const queuedSuffix = this.queueCount - 1;
    // Show queue position if waiting behind other users
    if (this.queuePosition != null && this.queuePosition > 0 && this.totalSteps === 0) {
      const pos = this.queuePosition;
      const own = this.queueCount;
      if (own > 1) {
        return locale.t("progress.queue_position_own", {
          pos: String(pos + 1),
          count: String(own - 1),
        });
      }
      return locale.t("progress.queue_position", { pos: String(pos + 1) });
    }
    if (this.totalSteps === 0) {
      // Position 0 (or unknown) means this prompt is not waiting behind
      // another prompt in the queue; avoid misleading "In queue..." labels.
      return this.queueCount > 1
        ? locale.t("progress.queued", { count: String(this.queueCount) })
        : locale.t("progress.preparing");
    }
    if (this.activePrompt?.params?.style_transfer_enabled && !this.hasReliableStepProgress) {
      return this.queueCount > 1
        ? locale.t("progress.applying_style_reference_queued", { count: String(queuedSuffix) })
        : locale.t("progress.applying_style_reference");
    }
    if (this.wasUpscaled && this.samplingPass >= 2) {
      return this.queueCount > 1
        ? locale.t("progress.upscaling_queued", { count: String(queuedSuffix) })
        : locale.t("progress.upscaling");
    }
    return this.queueCount > 1
      ? locale.t("progress.generating_queued", { count: String(queuedSuffix) })
      : locale.t("progress.generating");
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

  /** Add a new prompt to the queue.
   *
   * Idempotent by promptId: if the prompt is already tracked (e.g. inserted by
   * `restoreFromSnapshot` after an SSE `queue_update` event raced the HTTP
   * response), the existing entry is upgraded with the real params/mode instead
   * of appending a duplicate. Without this, the same prompt_id can appear twice
   * in `pendingPrompts`, making the queue display show 2 when only 1 image is
   * being generated.
   */
  enqueue(
    promptId: string,
    wasUpscaled: boolean = false,
    mode: "txt2img" | "img2img" | "inpainting" = "txt2img",
    params: GenerationParams | null = null,
  ) {
    const existingIdx = this.pendingPrompts.findIndex((p) => p.promptId === promptId);
    if (existingIdx >= 0) {
      const existing = this.pendingPrompts[existingIdx];
      const next = [...this.pendingPrompts];
      next[existingIdx] = {
        promptId,
        mode,
        wasUpscaled,
        params: params!,
        // Preserve enqueuedAt from the existing entry (set by restoreFromSnapshot
        // or a prior enqueue). If unset, stamp it now so the reconciler's 30s
        // activity guard has a valid baseline.
        enqueuedAt: existing.enqueuedAt ?? Date.now(),
      };
      this.pendingPrompts = next;
      return;
    }
    this.pendingPrompts = [
      ...this.pendingPrompts,
      {
        promptId,
        mode,
        wasUpscaled,
        params: params!,
        enqueuedAt: Date.now(),
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
      this.generationStartTime = Date.now();
      this._startElapsedTimer();
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
      this.generationStartTime = null;
      this._stopElapsedTimer();

      if (this.previewImage && item) {
        this.setLastOutputForMode(item.mode, this.previewImage);
      }
      this.previewImage = null;
    }

    return item;
  }

  /** Remove a specific prompt from the queue (e.g. on error). */
  removePrompt(promptId: string) {
    const wasActive = this.activePromptId === promptId;
    this.pendingPrompts = this.pendingPrompts.filter((p) => p.promptId !== promptId);

    if (wasActive || this.pendingPrompts.length === 0) {
      this.activePromptId = null;
      this.currentStep = 0;
      this.totalSteps = 0;
      this.currentNode = null;
      this.previewImage = null;
      this.samplingPass = 0;
      this._lastProgressNode = null;
      this.generationStartTime = null;
      this._stopElapsedTimer();
    }

    if (this.pendingPrompts.length === 0) {
      this.queuePosition = null;
      this.queueTotal = 0;
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
    this.generationStartTime = null;
    this._stopElapsedTimer();
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

  /** Temp output filenames keyed by prompt_id (for regional inpaint chain, etc.). */
  private promptOutputTemps = new Map<string, string>();
  private promptOutputWaiters = new Map<string, Array<(filename: string) => void>>();

  /** Return a registered temp output filename, if any. */
  getPromptOutputTemp(promptId: string): string | undefined {
    return this.promptOutputTemps.get(promptId);
  }

  /** Register a ComfyUI output temp file as soon as the output_image event arrives. */
  registerPromptOutput(promptId: string, tempFilename: string): void {
    const id = promptId.trim();
    const filename = tempFilename.trim();
    if (!id || !filename) return;
    this.promptOutputTemps.set(id, filename);
    const waiters = this.promptOutputWaiters.get(id);
    if (waiters) {
      for (const resolve of waiters) resolve(filename);
      this.promptOutputWaiters.delete(id);
    }
  }

  /** Wait until a temp output filename is registered for a prompt. */
  waitForPromptOutput(promptId: string, timeoutMs = 600_000): Promise<string> {
    const existing = this.promptOutputTemps.get(promptId);
    if (existing) return Promise.resolve(existing);

    return new Promise((resolve, reject) => {
      const resolveWith = (filename: string) => {
        clearTimeout(timer);
        this.promptOutputTemps.set(promptId, filename);
        resolve(filename);
      };

      const timer = setTimeout(() => {
        const waiters = this.promptOutputWaiters.get(promptId);
        if (waiters) {
          this.promptOutputWaiters.set(
            promptId,
            waiters.filter((entry) => entry !== resolveWith),
          );
        }
        reject(new Error(`No output image found for prompt ${promptId}`));
      }, timeoutMs);

      const waiters = this.promptOutputWaiters.get(promptId) ?? [];
      waiters.push(resolveWith);
      this.promptOutputWaiters.set(promptId, waiters);
    });
  }

  clearPromptOutput(promptId: string): void {
    this.promptOutputTemps.delete(promptId);
    this.promptOutputWaiters.delete(promptId);
  }

  /**
   * Restore pending queue entries from a snapshot delivered on SSE reconnect.
   * Only adds entries that aren't already tracked (idempotent).
   */
  restoreFromSnapshot(promptIds: string[]) {
    const now = Date.now();
    for (const pid of promptIds) {
      if (!this.pendingPrompts.some((p) => p.promptId === pid)) {
        this.pendingPrompts = [
          ...this.pendingPrompts,
          {
            promptId: pid,
            mode: "txt2img",
            wasUpscaled: false,
            params: null as any,
            // Stamp enqueuedAt so the reconciler's 30s activity guard has a
            // baseline for prompts restored from an SSE snapshot.
            enqueuedAt: now,
          },
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
