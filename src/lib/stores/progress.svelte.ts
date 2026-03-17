class ProgressStore {
  isGenerating = $state(false);
  currentPromptId = $state<string | null>(null);
  currentStep = $state(0);
  totalSteps = $state(0);
  currentNode = $state<string | null>(null);
  previewImage = $state<string | null>(null);
  lastOutputImage = $state<string | null>(null);
  wasUpscaled = $state(false);

  /** Which sampling pass we're on: 0 = not started, 1 = initial, 2 = upscale */
  samplingPass = $state(0);
  /** Tracks the last node that had progress, to detect pass changes */
  private _lastProgressNode: string | null = null;

  get percentage() {
    return this.totalSteps > 0
      ? (this.currentStep / this.totalSteps) * 100
      : 0;
  }

  get displayImage() {
    return this.previewImage ?? this.lastOutputImage;
  }

  /** Plain English label for the current phase */
  get phaseLabel(): string {
    if (!this.isGenerating) return "";
    if (this.totalSteps === 0) return "Preparing...";
    if (this.wasUpscaled && this.samplingPass >= 2) return "Upscaling...";
    return "Generating...";
  }

  /** Called when a progress event arrives — detects pass transitions */
  updateProgress(step: number, max: number, node: string | null) {
    // Detect when a new KSampler starts (node changes while progress resets)
    if (node && node !== this._lastProgressNode) {
      this._lastProgressNode = node;
      this.samplingPass += 1;
    }
    this.currentStep = step;
    this.totalSteps = max;
  }

  reset() {
    this.isGenerating = false;
    this.currentPromptId = null;
    this.currentStep = 0;
    this.totalSteps = 0;
    this.currentNode = null;
    this.samplingPass = 0;
    this._lastProgressNode = null;
    // Keep last preview as the output image
    if (this.previewImage) {
      this.lastOutputImage = this.previewImage;
    }
    this.previewImage = null;
  }

  startGeneration(promptId: string, upscaled: boolean = false) {
    this.isGenerating = true;
    this.currentPromptId = promptId;
    this.currentStep = 0;
    this.totalSteps = 0;
    this.previewImage = null;
    this.wasUpscaled = upscaled;
    this.samplingPass = 0;
    this._lastProgressNode = null;
  }
}

export const progress = new ProgressStore();
