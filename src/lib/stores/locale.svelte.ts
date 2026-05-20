import { ipcStore } from "../utils/ipc.js";
import { triggerSync } from "../utils/syncTrigger.js";
import en from "../locales/en.js";
import es from "../locales/es.js";
import ja from "../locales/ja.js";
import fr from "../locales/fr.js";
import ko from "../locales/ko.js";
import zh from "../locales/zh.js";
import zhTw from "../locales/zh-tw.js";
import de from "../locales/de.js";
import pt from "../locales/pt.js";
import ru from "../locales/ru.js";
import it from "../locales/it.js";

const STORE_KEY = "locale-settings";

export type Locale = "en" | "es" | "ja" | "fr" | "ko" | "zh" | "zh-tw" | "de" | "pt" | "ru" | "it";

const translations: Record<Locale, Record<string, string>> = {
  en, es, ja, fr, ko, zh, "zh-tw": zhTw, de, pt, ru, it,
};

export const LOCALE_OPTIONS: { value: Locale; label: string }[] = [
  { value: "en", label: "English" },
  { value: "es", label: "Español" },
  { value: "ja", label: "日本語" },
  { value: "fr", label: "Français" },
  { value: "de", label: "Deutsch" },
  { value: "ko", label: "한국어" },
  { value: "zh", label: "简体中文" },
  { value: "zh-tw", label: "繁體中文" },
  { value: "pt", label: "Português" },
  { value: "ru", label: "Русский" },
  { value: "it", label: "Italiano" },
];

class LocaleStore {
  current = $state<Locale>("en");
  private hasStoredPreference = false;

  /** Detect system language and set locale if we support it, otherwise keep English.
   *  Only applies when no user preference has been saved yet. */
  detectSystemLocale(): void {
    if (this.hasStoredPreference) return;
    const nav = globalThis.navigator;
    if (!nav?.languages && !nav?.language) return;
    const candidates = nav.languages ? [...nav.languages] : [nav.language];
    for (const tag of candidates) {
      const lower = tag.toLowerCase();
      // Exact match (e.g. "zh-tw")
      if (lower in translations) {
        this.current = lower as Locale;
        return;
      }
      // Base language match (e.g. "zh-CN" → "zh", "pt-BR" → "pt")
      const base = lower.split("-")[0];
      if (base in translations) {
        this.current = base as Locale;
        return;
      }
    }
  }

  /** Look up a translation key, with optional {var} interpolation. */
  t(key: string, vars?: Record<string, string | number>): string {
    let text = translations[this.current][key] ?? translations.en[key] ?? key;
    if (vars) {
      for (const [k, v] of Object.entries(vars)) {
        text = text.replaceAll(`{${k}}`, String(v));
      }
    }
    return text;
  }

  async loadSettings(): Promise<void> {
    try {
      const data = await ipcStore.get<{ locale?: Locale }>(STORE_KEY);
      if (data?.locale && translations[data.locale]) {
        this.current = data.locale;
        this.hasStoredPreference = true;
      }
    } catch {
      // First launch — no persisted locale yet.
    }
  }

  async saveSettings(): Promise<void> {
    await ipcStore.set(STORE_KEY, { locale: this.current });
    triggerSync();
  }

  applyServerPrefs(localeValue: string): void {
    try {
      if (localeValue && translations[localeValue as Locale]) {
        this.current = localeValue as Locale;
        this.hasStoredPreference = true;
        this.saveSettings().catch(() => {});
      }
    } catch (e) {
      console.error("Failed to apply server prefs (locale):", e);
    }
  }
}

export const locale = new LocaleStore();
