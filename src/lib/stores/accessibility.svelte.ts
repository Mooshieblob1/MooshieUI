import { triggerSync } from "../utils/syncTrigger.js";
import { userScopedKey } from "../utils/ipc.js";

const ACCESSIBILITY_SETTINGS_KEY = "mooshieui.accessibility.v1";
/** Where the dyslexia-friendly font toggle lived before it joined this store. */
const LEGACY_DYSLEXIC_FONT_KEY = "mooshieui.dyslexicFont";

export type VisionSimulatorMode = "none" | "protanopia" | "deuteranopia" | "tritanopia";

class AccessibilityStore {
  visionSimulatorMode = $state<VisionSimulatorMode>("none");
  showInfoTips = $state(true);
  dyslexicFont = $state(false);

  constructor() {
    this.loadSettings();
  }

  loadSettings() {
    try {
      const raw = localStorage.getItem(userScopedKey(ACCESSIBILITY_SETTINGS_KEY));
      const parsed = raw ? JSON.parse(raw) : {};
      if (parsed.dyslexicFont !== undefined) {
        this.dyslexicFont = !!parsed.dyslexicFont;
      } else {
        this.dyslexicFont = localStorage.getItem(LEGACY_DYSLEXIC_FONT_KEY) === "true";
      }
      if (parsed.visionSimulatorMode) {
        this.visionSimulatorMode = parsed.visionSimulatorMode;
      }
      if (parsed.showInfoTips !== undefined) {
        this.showInfoTips = parsed.showInfoTips;
      }
    } catch (e) {
      console.error("Failed to load accessibility settings:", e);
    }
  }

  saveSettings() {
    try {
      localStorage.setItem(userScopedKey(ACCESSIBILITY_SETTINGS_KEY), JSON.stringify({
        visionSimulatorMode: this.visionSimulatorMode,
        showInfoTips: this.showInfoTips,
        dyslexicFont: this.dyslexicFont,
      }));
      triggerSync();
    } catch (e) {
      console.error("Failed to save accessibility settings:", e);
    }
  }

  collectPrefs(): unknown {
    return {
      visionSimulatorMode: this.visionSimulatorMode,
      showInfoTips: this.showInfoTips,
      dyslexicFont: this.dyslexicFont,
    };
  }

  applyServerPrefs(data: any): void {
    try {
      if (data?.visionSimulatorMode) this.visionSimulatorMode = data.visionSimulatorMode;
      if (data?.showInfoTips !== undefined) this.showInfoTips = data.showInfoTips;
      if (data?.dyslexicFont !== undefined) this.dyslexicFont = !!data.dyslexicFont;
      this.saveSettings();
    } catch (e) {
      console.error("Failed to apply server prefs (accessibility):", e);
    }
  }

  setDyslexicFont(enabled: boolean) {
    this.dyslexicFont = enabled;
    this.saveSettings();
  }

  setVisionSimulatorMode(mode: VisionSimulatorMode) {
    this.visionSimulatorMode = mode;
    this.saveSettings();
  }
}

export const accessibility = new AccessibilityStore();
