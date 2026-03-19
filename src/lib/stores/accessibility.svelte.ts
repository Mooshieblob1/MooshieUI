const ACCESSIBILITY_SETTINGS_KEY = "mooshieui.accessibility.v1";

export type VisionSimulatorMode = "none" | "protanopia" | "deuteranopia" | "tritanopia";

class AccessibilityStore {
  visionSimulatorMode = $state<VisionSimulatorMode>("none");

  constructor() {
    this.loadSettings();
  }

  loadSettings() {
    try {
      const raw = localStorage.getItem(ACCESSIBILITY_SETTINGS_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw);
      if (parsed.visionSimulatorMode) {
        this.visionSimulatorMode = parsed.visionSimulatorMode;
      }
    } catch (e) {
      console.error("Failed to load accessibility settings:", e);
    }
  }

  saveSettings() {
    try {
      localStorage.setItem(ACCESSIBILITY_SETTINGS_KEY, JSON.stringify({
        visionSimulatorMode: this.visionSimulatorMode
      }));
    } catch (e) {
      console.error("Failed to save accessibility settings:", e);
    }
  }

  setVisionSimulatorMode(mode: VisionSimulatorMode) {
    this.visionSimulatorMode = mode;
    this.saveSettings();
  }
}

export const accessibility = new AccessibilityStore();
