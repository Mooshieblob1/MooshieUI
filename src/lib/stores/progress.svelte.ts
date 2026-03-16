class ProgressStore {
  isGenerating = $state(false);
  currentPromptId = $state<string | null>(null);
  currentStep = $state(0);
  totalSteps = $state(0);
  currentNode = $state<string | null>(null);
  previewImage = $state<string | null>(null);
  lastOutputImage = $state<string | null>(null);

  get percentage() {
    return this.totalSteps > 0
      ? (this.currentStep / this.totalSteps) * 100
      : 0;
  }

  get displayImage() {
    return this.previewImage ?? this.lastOutputImage;
  }

  reset() {
    this.isGenerating = false;
    this.currentPromptId = null;
    this.currentStep = 0;
    this.totalSteps = 0;
    this.currentNode = null;
    // Keep last preview as the output image
    if (this.previewImage) {
      this.lastOutputImage = this.previewImage;
    }
    this.previewImage = null;
  }

  startGeneration(promptId: string) {
    this.isGenerating = true;
    this.currentPromptId = promptId;
    this.currentStep = 0;
    this.totalSteps = 0;
    this.previewImage = null;
  }
}

export const progress = new ProgressStore();
