export type ThemeMode = "dark" | "light";
export type ThemePalette = "mooshie" | "nord" | "solarized" | "gruvbox" | "catppuccin";

export interface ThemeTone {
  main: string;
  sub: string;
  trim: string;
  background: string;
  text: string;
}

export interface ThemeProfile {
  id: string;
  name: string;
  palette: ThemePalette | "custom";
  dark: ThemeTone;
  light: ThemeTone;
  background_image: string | null;
  background_fade: number;
  logo_image: string | null;
  hide_branding: boolean;
}

export interface ThemeConfigLike {
  theme: string | null | undefined;
  theme_palette?: string | null;
  theme_profile_id?: string | null;
  theme_profiles?: ThemeProfile[] | null;
}

export const THEME_TONE_FIELDS: Array<{ key: keyof ThemeTone; labelKey: string }> = [
  { key: "main", labelKey: "settings.appearance.tone_main" },
  { key: "sub", labelKey: "settings.appearance.tone_sub" },
  { key: "trim", labelKey: "settings.appearance.tone_trim" },
  { key: "background", labelKey: "settings.appearance.tone_background" },
  { key: "text", labelKey: "settings.appearance.tone_text" },
];

export const THEME_PALETTES: Array<{ value: ThemePalette; label: string }> = [
  { value: "mooshie", label: "Mooshie" },
  { value: "nord", label: "Nord" },
  { value: "solarized", label: "Solarized" },
  { value: "gruvbox", label: "Gruvbox" },
  { value: "catppuccin", label: "Catppuccin" },
];

export function normalizeThemeMode(value: string | null | undefined): ThemeMode {
  return value === "light" ? "light" : "dark";
}

export function normalizeThemePalette(value: string | null | undefined): ThemePalette {
  return THEME_PALETTES.some((palette) => palette.value === value)
    ? (value as ThemePalette)
    : "mooshie";
}

export const DEFAULT_THEME_TONE_DARK: ThemeTone = {
  main: "#ffcc00",
  sub: "#404040",
  trim: "#ffd54d",
  background: "#0a0a0a",
  text: "#f5f5f5",
};

export const DEFAULT_THEME_TONE_LIGHT: ThemeTone = {
  main: "#ffcc00",
  sub: "#404040",
  trim: "#ffd54d",
  background: "#f5f5f5",
  text: "#111111",
};

function normalizeThemeTone(
  input?: Partial<ThemeTone> | null,
  defaultTone: ThemeTone = DEFAULT_THEME_TONE_DARK,
): ThemeTone {
  return {
    main: typeof input?.main === "string" && input.main.trim() ? input.main : defaultTone.main,
    sub: typeof input?.sub === "string" && input.sub.trim() ? input.sub : defaultTone.sub,
    trim: typeof input?.trim === "string" && input.trim.trim() ? input.trim : defaultTone.trim,
    background:
      typeof input?.background === "string" && input.background.trim()
        ? input.background
        : defaultTone.background,
    text: typeof input?.text === "string" && input.text.trim() ? input.text : defaultTone.text,
  };
}

function clamp01(value: number): number {
  if (!Number.isFinite(value)) return 0.55;
  return Math.max(0, Math.min(1, value));
}

function mixColor(a: string, b: string, aPercent: number): string {
  const pct = Math.max(0, Math.min(100, aPercent));
  return `color-mix(in srgb, ${a} ${pct}%, ${b})`;
}

const CUSTOM_THEME_CSS_VARS = [
  "--theme-main",
  "--theme-sub",
  "--theme-trim",
  "--theme-background",
  "--theme-text",
  "--theme-accent-foreground",
  "--theme-accent-50",
  "--theme-accent-100",
  "--theme-accent-200",
  "--theme-accent-300",
  "--theme-accent-400",
  "--theme-accent-500",
  "--theme-accent-600",
  "--theme-accent-700",
  "--theme-accent-800",
  "--theme-accent-900",
  "--theme-accent-950",
  "--theme-surface-950",
  "--theme-surface-900",
  "--theme-surface-800",
  "--theme-border-700",
  "--color-neutral-50",
  "--color-neutral-100",
  "--color-neutral-200",
  "--color-neutral-300",
  "--color-neutral-400",
  "--color-neutral-500",
  "--color-neutral-600",
  "--color-neutral-700",
  "--color-neutral-800",
  "--color-neutral-900",
  "--color-neutral-950",
  "--theme-logo-image",
] as const;

let activeThemeLogoUrl: string | null = null;

export function getActiveThemeLogoUrl(): string | null {
  return activeThemeLogoUrl;
}

export function onThemeApplied(callback: () => void): () => void {
  const handler = () => callback();
  window.addEventListener("mooshie-theme-applied", handler);
  return () => window.removeEventListener("mooshie-theme-applied", handler);
}

function notifyThemeApplied(): void {
  window.dispatchEvent(new Event("mooshie-theme-applied"));
}

function clearCustomThemeTokens(root: HTMLElement, body: HTMLElement | null): void {
  for (const key of CUSTOM_THEME_CSS_VARS) {
    root.style.removeProperty(key);
  }
  if (body) {
    body.style.removeProperty("--theme-bg-image");
    body.style.removeProperty("--theme-bg-fade");
  }
  activeThemeLogoUrl = null;
}

function applyCustomThemeTokens(root: HTMLElement, tone: ThemeTone): void {
  const { main, sub, trim, background, text } = tone;

  root.style.setProperty("--theme-main", main);
  root.style.setProperty("--theme-sub", sub);
  root.style.setProperty("--theme-trim", trim);
  root.style.setProperty("--theme-background", background);
  root.style.setProperty("--theme-text", text);
  root.style.setProperty("--theme-accent-foreground", text);

  const accentScale: Record<string, string> = {
    50: mixColor(main, background, 12),
    100: mixColor(main, background, 22),
    200: mixColor(trim, background, 34),
    300: mixColor(trim, background, 48),
    400: trim,
    500: main,
    600: mixColor(main, sub, 82),
    700: mixColor(main, sub, 68),
    800: mixColor(main, sub, 52),
    900: mixColor(main, background, 34),
    950: mixColor(main, background, 20),
  };
  for (const [step, value] of Object.entries(accentScale)) {
    root.style.setProperty(`--theme-accent-${step}`, value);
  }

  // Canvas / backdrop: background token. Panels: background + sub.
  root.style.setProperty("--theme-surface-950", background);
  root.style.setProperty("--theme-surface-900", mixColor(background, sub, 16));
  root.style.setProperty("--theme-surface-800", mixColor(background, sub, 28));
  root.style.setProperty("--theme-border-700", mixColor(background, sub, 35));

  const structuralNeutral: Record<string, string> = {
    950: background,
    900: mixColor(background, sub, 10),
    800: mixColor(background, sub, 18),
    700: mixColor(background, sub, 28),
    600: mixColor(sub, background, 50),
  };

  // Muted/primary text: derived from text only.
  const textNeutral: Record<string, string> = {
    500: mixColor(text, background, 52),
    400: mixColor(text, background, 64),
    300: mixColor(text, background, 74),
    200: mixColor(text, background, 84),
    100: text,
    50: text,
  };

  for (const [step, value] of Object.entries(structuralNeutral)) {
    root.style.setProperty(`--color-neutral-${step}`, value);
  }
  for (const [step, value] of Object.entries(textNeutral)) {
    root.style.setProperty(`--color-neutral-${step}`, value);
  }
}

function normalizeThemeProfile(profile: ThemeProfile): ThemeProfile {
  return {
    ...profile,
    dark: normalizeThemeTone(profile.dark, DEFAULT_THEME_TONE_DARK),
    light: normalizeThemeTone(profile.light, DEFAULT_THEME_TONE_LIGHT),
    background_fade: clamp01(profile.background_fade),
  };
}

function findActiveProfile(config: ThemeConfigLike): ThemeProfile | null {
  const profiles = Array.isArray(config.theme_profiles) ? config.theme_profiles : [];
  const active = typeof config.theme_profile_id === "string" ? config.theme_profile_id : "";
  if (!active) return null;
  const profile = profiles.find((item) => item.id === active);
  return profile ? normalizeThemeProfile(profile) : null;
}

export function applyTheme(config: ThemeConfigLike): void {
  const themeMode = normalizeThemeMode(config.theme);
  const activeProfile = findActiveProfile(config);
  const themePalette = activeProfile ? activeProfile.palette : normalizeThemePalette(config.theme_palette);
  const root = document.documentElement;
  const body = document.body;

  root.classList.toggle("light", themeMode === "light");
  root.dataset.palette = themePalette;
  root.dataset.branding = activeProfile?.hide_branding ? "off" : "on";

  const tone = themeMode === "light" ? activeProfile?.light : activeProfile?.dark;
  if (tone) {
    applyCustomThemeTokens(root, tone);
    activeThemeLogoUrl = activeProfile?.logo_image ?? null;
    if (activeThemeLogoUrl) {
      root.style.setProperty("--theme-logo-image", `url("${activeThemeLogoUrl}")`);
    } else {
      root.style.removeProperty("--theme-logo-image");
    }
  } else {
    clearCustomThemeTokens(root, body);
  }

  if (body) {
    if (activeProfile?.background_image) {
      body.style.setProperty("--theme-bg-image", `url("${activeProfile.background_image}")`);
      body.style.setProperty("--theme-bg-fade", String(activeProfile.background_fade));
      root.dataset.themeBackground = "custom";
    } else {
      body.style.removeProperty("--theme-bg-image");
      body.style.removeProperty("--theme-bg-fade");
      root.dataset.themeBackground = "default";
    }
  }

  const browserMode = Boolean((window as any).__MOOSHIE_BROWSER_MODE__);
  if (browserMode && activeProfile?.logo_image) {
    const link =
      (document.querySelector("link[rel='icon']") as HTMLLinkElement | null) ??
      (document.querySelector("link[rel='shortcut icon']") as HTMLLinkElement | null);
    if (link) link.href = activeProfile.logo_image;
  }

  notifyThemeApplied();
}
