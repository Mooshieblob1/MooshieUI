/**
 * Registered model architectures, as the backend declares them in
 * `src-tauri/src/architectures/` and serves through `list_model_architectures`.
 *
 * A family listed here needs no hand-written TypeScript: labels, prompt style,
 * split-only loading, sampling presets, Model Hub entries, text encoder / VAE /
 * extra-file auto-download, VAE compatibility and Image Edit support all read
 * this registry. Pure module (no runes) so the node:test scripts can import
 * it; `stores/architectures.svelte.ts` loads it and exposes a reactive version.
 */

export interface ArchitectureModelFile {
  filename: string;
  url: string;
  category: string;
  size: string | null;
  sha256: string | null;
}

export interface ArchitectureSampling {
  steps: number;
  cfg: number;
  sampler: string;
  scheduler: string;
  width: number;
  height: number;
}

export interface ArchitectureEncoder {
  clip_type: string;
  markers: string[];
  strict: boolean;
  download: ArchitectureModelFile | null;
}

export interface ArchitectureVae {
  basenames: string[];
  markers: string[];
  strict: boolean;
  download: ArchitectureModelFile | null;
}

export interface ArchitectureModelSlot {
  key: string;
  label: string;
  category: string;
  markers: string[];
  strict: boolean;
  download: ArchitectureModelFile | null;
}

export interface ArchitectureRecommendedModel {
  label: string;
  size: string;
  diffusion_model: ArchitectureModelFile;
  min_compute_capability: number | null;
}

export interface ArchitectureFamily {
  family: string;
  label: string;
  turbo: boolean;
  sampling: ArchitectureSampling | null;
  recommended: ArchitectureRecommendedModel[];
  text_encoder: ArchitectureEncoder | null;
  vae: ArchitectureVae | null;
  /** Fixed sigma schedule the family was distilled for (Turbo models). */
  manual_sigmas: string | null;
}

export interface ArchitectureMode {
  mode: string;
  max_references: number;
  required_files: ArchitectureModelFile[];
  sampling: ArchitectureSampling | null;
}

export interface ArchitectureSpec {
  id: string;
  label: string;
  families: ArchitectureFamily[];
  split_only: boolean;
  prompt_style: "tags" | "natural";
  tensor_architecture: string | null;
  text_encoder: ArchitectureEncoder | null;
  vae: ArchitectureVae | null;
  model_slots: ArchitectureModelSlot[];
  core_nodes: string[];
  min_comfyui: string | null;
  int8_fast_model_type: string | null;
  modes: ArchitectureMode[];
  negative_prompt: boolean;
  resolution_multiple: number;
  /** False when the backend refuses ControlNet for this architecture. */
  controlnet: boolean;
}

let registry: ArchitectureSpec[] = [];

/** Replace the registry (the store does this once the backend answers). */
export function setArchitectures(list: ArchitectureSpec[]): void {
  registry = Array.isArray(list) ? list : [];
}

export function allArchitectures(): readonly ArchitectureSpec[] {
  return registry;
}

/** The architecture and family entry of a registered family. */
export function findFamily(
  family: string | null | undefined,
): { arch: ArchitectureSpec; family: ArchitectureFamily } | null {
  if (!family) return null;
  for (const arch of registry) {
    const entry = arch.families.find((f) => f.family === family);
    if (entry) return { arch, family: entry };
  }
  return null;
}

export function isRegisteredFamily(family: string | null | undefined): boolean {
  return findFamily(family) !== null;
}

/** Every registered family id, in registry order. */
export function registeredFamilies(): string[] {
  return registry.flatMap((arch) => arch.families.map((f) => f.family));
}

export function registeredFamilyLabel(family: string | null | undefined): string | undefined {
  return findFamily(family)?.family.label;
}

/** Sampling defaults for a family, the mode's own when it declares them. */
export function registeredSampling(
  family: string | null | undefined,
  mode?: string | null,
): ArchitectureSampling | null {
  const found = findFamily(family);
  if (!found) return null;
  const modeSampling = mode ? found.arch.modes.find((m) => m.mode === mode)?.sampling : null;
  return modeSampling ?? found.family.sampling ?? null;
}

export function registeredEncoder(family: string | null | undefined): ArchitectureEncoder | null {
  const found = findFamily(family);
  return found ? (found.family.text_encoder ?? found.arch.text_encoder) : null;
}

export function registeredVae(family: string | null | undefined): ArchitectureVae | null {
  const found = findFamily(family);
  return found ? (found.family.vae ?? found.arch.vae) : null;
}

export function registeredMode(
  family: string | null | undefined,
  mode: string,
): ArchitectureMode | null {
  return findFamily(family)?.arch.modes.find((m) => m.mode === mode) ?? null;
}

function lowerBasename(path: string): string {
  const lower = path.toLowerCase();
  return lower.split(/[\\/]/).pop() ?? lower;
}

/** Mirrors `encoder_matches` in architectures/mod.rs. */
export function encoderMatches(spec: ArchitectureEncoder, filename: string | null | undefined): boolean {
  if (!filename) return false;
  const lower = filename.toLowerCase();
  return (
    spec.markers.some((marker) => lower.includes(marker)) ||
    (!!spec.download && lowerBasename(filename) === spec.download.filename.toLowerCase())
  );
}

/** Mirrors `vae_matches` in architectures/mod.rs. */
export function vaeMatches(spec: ArchitectureVae, filename: string | null | undefined): boolean {
  if (!filename) return false;
  const base = lowerBasename(filename);
  const lower = filename.toLowerCase();
  return (
    spec.basenames.some((b) => base === b.toLowerCase()) ||
    spec.markers.some((marker) => lower.includes(marker)) ||
    (!!spec.download && base === spec.download.filename.toLowerCase())
  );
}

/** A model slot accepts a file named after its markers or its download. */
export function slotMatches(slot: ArchitectureModelSlot, filename: string | null | undefined): boolean {
  if (!filename) return false;
  const lower = filename.toLowerCase();
  return (
    slot.markers.some((marker) => lower.includes(marker)) ||
    (!!slot.download && lowerBasename(filename) === slot.download.filename.toLowerCase())
  );
}

/** Model Hub entries of every registered family, with the family they load. */
export function registeredRecommendedModels(): {
  arch: ArchitectureSpec;
  family: ArchitectureFamily;
  model: ArchitectureRecommendedModel;
}[] {
  return registry.flatMap((arch) =>
    arch.families.flatMap((family) => family.recommended.map((model) => ({ arch, family, model }))),
  );
}
