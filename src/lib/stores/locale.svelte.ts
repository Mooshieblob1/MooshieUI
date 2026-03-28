import { load } from "@tauri-apps/plugin-store";
import en from "../locales/en.js";
import es from "../locales/es.js";

const STORE_KEY = "locale-settings";

export type Locale = "en" | "es";

const translations: Record<Locale, Record<string, string>> = { en, es };

export const LOCALE_OPTIONS: { value: Locale; label: string }[] = [
  { value: "en", label: "English" },
  { value: "es", label: "Español" },
];

class LocaleStore {
  current = $state<Locale>("en");

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
      const store = await load("settings.json", { autoSave: true });
      const data = await store.get<{ locale?: Locale }>(STORE_KEY);
      if (data?.locale && translations[data.locale]) {
        this.current = data.locale;
      }
    } catch {
      // First launch — no persisted locale yet.
    }
  }

  async saveSettings(): Promise<void> {
    const store = await load("settings.json", { autoSave: true });
    await store.set(STORE_KEY, { locale: this.current });
  }
}

export const locale = new LocaleStore();
