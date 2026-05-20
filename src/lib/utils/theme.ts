export type ThemeMode = "dark" | "light";
export type ThemePalette = "mooshie" | "nord" | "solarized" | "gruvbox" | "catppuccin";

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

export function applyTheme(mode: string | null | undefined, palette?: string | null): void {
  const themeMode = normalizeThemeMode(mode);
  const themePalette = normalizeThemePalette(palette);
  const root = document.documentElement;
  root.classList.toggle("light", themeMode === "light");
  root.dataset.palette = themePalette;
}
