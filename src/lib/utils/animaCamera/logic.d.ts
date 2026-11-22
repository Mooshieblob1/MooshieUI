/** Types for the unmodified v1.0.1 prompt implementation; see public/licenses/anima-camera-LICENSE.txt. */
export interface CameraPosition {
  horizontal_angle: number;
  vertical_angle: number;
  zoom: number;
  roll: number;
}

export function createCameraLogic(data: typeof import("./data.json")): {
  number(value: unknown, fallback: number, min?: number, max?: number): number;
  normalizeCamera(horizontal?: unknown, vertical?: unknown, zoom?: unknown, roll?: unknown): CameraPosition;
  buildExtraConfig(values?: Record<string, string | number>): object;
  cameraPrompt(horizontal?: number, vertical?: number, zoom?: number, roll?: number, tags?: string, extra?: object): string;
  closestPreset(key: "horizontal_angle" | "vertical_angle" | "zoom", value: number): { label: string; value: number };
};
