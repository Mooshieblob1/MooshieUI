/**
 * Camera angle prompts for Qwen Image Edit Plus, in the vocabulary of fal's
 * Multiple-Angles LoRA (96 poses: 8 azimuths x 4 elevations x 3 distances).
 * ComfyUI's core CameraAngle node (v0.40.0+) emits the same words, so the
 * phrase is built here instead of adding a node to the workflow.
 */

/** Azimuths clockwise from the front, 45 degrees apart. */
export const CAMERA_AZIMUTHS = [
  "front view",
  "front-right quarter view",
  "right side view",
  "back-right quarter view",
  "back view",
  "back-left quarter view",
  "left side view",
  "front-left quarter view",
] as const;

/** Elevations from below (-30 degrees) to high above (60 degrees). */
export const CAMERA_ELEVATIONS = [
  "low-angle shot",
  "eye-level shot",
  "elevated shot",
  "high-angle shot",
] as const;

export const CAMERA_DISTANCES = ["close-up", "medium shot", "wide shot"] as const;

/** The LoRA's trigger token; meaningless to the base model on its own. */
export const CAMERA_ANGLE_TRIGGER = "<sks>";

export const MULTI_ANGLE_LORA = {
  url: "https://huggingface.co/fal/Qwen-Image-Edit-2511-Multiple-Angles-LoRA/resolve/main/qwen-image-edit-2511-multiple-angles-lora.safetensors",
  filename: "qwen-image-edit-2511-multiple-angles-lora.safetensors",
  /** Model card recommends 0.8-1.0 and starting at 0.9. */
  strength: 0.9,
} as const;

function clampIndex(value: unknown, length: number, fallback: number): number {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 && value < length
    ? value
    : fallback;
}

export const clampAzimuth = (v: unknown) => clampIndex(v, CAMERA_AZIMUTHS.length, 0);
export const clampElevation = (v: unknown) => clampIndex(v, CAMERA_ELEVATIONS.length, 1);
export const clampDistance = (v: unknown) => clampIndex(v, CAMERA_DISTANCES.length, 1);

/**
 * The shot phrase, in the LoRA's required order (azimuth, elevation, distance).
 * The trigger is only added when the LoRA is applied; without it the plain
 * shot words still nudge the base model.
 */
export function cameraAnglePhrase(
  azimuth: number,
  elevation: number,
  distance: number,
  withTrigger: boolean,
): string {
  const words = `${CAMERA_AZIMUTHS[clampAzimuth(azimuth)]} ${CAMERA_ELEVATIONS[clampElevation(elevation)]} ${CAMERA_DISTANCES[clampDistance(distance)]}`;
  return withTrigger ? `${CAMERA_ANGLE_TRIGGER} ${words}` : words;
}

const basename = (n: string) => n.split(/[\\/]/).pop() ?? n;

/** The installed copy of the Multiple-Angles LoRA, matched by filename in any subfolder. */
export function installedMultiAngleLora(installed: readonly string[]): string | null {
  return installed.find((n) => basename(n) === MULTI_ANGLE_LORA.filename) ?? null;
}

/**
 * The Multiple-Angles LoRA to add to an outgoing generation, or null: only
 * while the camera angle is active, the file is installed, and the user has
 * not already put it in their own enabled LoRA list.
 */
export function multiAngleLoraToApply(
  active: boolean,
  installed: readonly string[],
  enabledLoras: readonly string[],
): string | null {
  if (!active) return null;
  if (enabledLoras.some((n) => basename(n) === MULTI_ANGLE_LORA.filename)) return null;
  return installedMultiAngleLora(installed);
}
