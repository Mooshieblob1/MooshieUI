import data from "./animaCamera/data.json";
import { createCameraLogic, type CameraPosition } from "./animaCamera/logic.js";

export const animaCameraData = data;
export const animaCameraLogic = createCameraLogic(data);
export type CameraAxis = "horizontal_angle" | "vertical_angle" | "zoom";
export type CameraGroup = "lens" | "dof" | "movement" | "composition" | "style" | "lighting";

export interface ExtraSelection {
  preset: string;
  custom: string;
  weight: number;
}

export interface AnimaCameraSettings extends CameraPosition {
  extremePreset: string;
  extremeWeight: number;
  extras: Record<CameraGroup, ExtraSelection>;
}

/** Only accept catalog presets and bounded values from settings/profiles. */
export function normalizeAnimaCamera(value: unknown = {}): AnimaCameraSettings {
  const saved = value && typeof value === "object" ? value as Partial<AnimaCameraSettings> : {};
  const extras = Object.fromEntries(data.extra_groups.map((group) => {
    const entry = saved.extras?.[group.key as CameraGroup];
    const preset = entry?.preset;
    return [group.key, {
      preset: preset === "Custom" || group.presets.some((p) => p.label === preset) ? preset : "None",
      custom: typeof entry?.custom === "string" ? entry.custom : "",
      weight: animaCameraLogic.number(entry?.weight, 1, 0.1, 10),
    }];
  })) as Record<CameraGroup, ExtraSelection>;
  return {
    ...animaCameraLogic.normalizeCamera(saved.horizontal_angle, saved.vertical_angle, saved.zoom, saved.roll),
    extremePreset: data.extreme_presets.some((p) => p.label === saved.extremePreset) ? saved.extremePreset! : "None",
    extremeWeight: animaCameraLogic.number(saved.extremeWeight, 1, 0.1, 50),
    extras,
  };
}

export function animaCameraPrompt(settings: AnimaCameraSettings): string {
  const values: Record<string, string | number> = {
    extreme_type: settings.extremePreset,
    extreme_weight: settings.extremeWeight,
  };
  for (const group of data.extra_groups) {
    const extra = settings.extras[group.key as CameraGroup];
    values[`${group.key}_preset`] = extra.preset;
    values[`${group.key}_custom`] = extra.custom;
    values[`${group.key}_weight`] = extra.weight;
  }
  return animaCameraLogic.cameraPrompt(
    settings.horizontal_angle, settings.vertical_angle, settings.zoom, settings.roll,
    "{}", animaCameraLogic.buildExtraConfig(values),
  );
}

/** Keep weighted phrases intact: comma tokenization can split custom groups. */
export function appendAnimaCameraPrompt(prompt: string, fragment: string): string {
  const text = prompt.trim();
  if (!fragment || text === fragment || text.endsWith(`, ${fragment}`)) return prompt;
  return text ? `${text}, ${fragment}` : fragment;
}
