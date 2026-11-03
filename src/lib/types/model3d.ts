/** Image to 3D (Gaussian splat) types, mirroring `commands/splat.rs`. */

export interface SplatWorkerCapabilities {
  worker_id: number;
  missing_nodes: string[];
  models: string[];
  clip_visions: string[];
  decoders: string[];
  flux2_vaes: string[];
  bg_models: string[];
}

export interface SplatCapabilities {
  missing_nodes: string[];
  models: string[];
  clip_visions: string[];
  decoders: string[];
  flux2_vaes: string[];
  bg_models: string[];
  workers: SplatWorkerCapabilities[];
}

export interface SplatParams {
  model: string;
  remove_background: boolean;
  steps: number;
  cfg: number;
  num_gaussians: number;
  /** "-1" for random, like the other generation seeds. */
  seed: string;
}

export interface SplatJob {
  prompt_id: string;
  worker_id: number;
  seed: string;
  model: string;
}

export type SplatStatus =
  | { status: "queued" | "running" | "missing" }
  | { status: "completed"; filename: string }
  | { status: "error"; error: string };

/** One finished splat in the per-user library (IndexedDB). */
export interface SplatResult extends SplatJob {
  id: string;
  title: string;
  params: SplatParams;
  /** Number of Gaussians in the stored PLY. */
  count: number;
  /** Small JPEG data URL of the source image, for the library grid. */
  thumbnail: string;
  createdAt: number;
}
