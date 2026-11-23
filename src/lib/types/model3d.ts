/** Image to 3D types, mirroring `commands/splat.rs` (Idol) and `commands/mesh3d.rs` (Pixal3D). */

/** Idol makes a Gaussian splat, Pixal3D a textured mesh. */
export type Model3DEngine = "pixal3d" | "idol";

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

export interface MeshWorkerCapabilities {
  worker_id: number;
  missing_nodes: string[];
  models: string[];
  clip_visions: string[];
  shape_vaes: string[];
  texture_vaes: string[];
  moge_models: string[];
  bg_models: string[];
}

export interface MeshCapabilities {
  missing_nodes: string[];
  models: string[];
  clip_visions: string[];
  shape_vaes: string[];
  texture_vaes: string[];
  moge_models: string[];
  bg_models: string[];
  workers: MeshWorkerCapabilities[];
}

/** Face and texture budget: high is 700,000 faces and 4096 px, standard 300,000 and 2048 px. */
export type MeshDetail = "standard" | "high";

export interface MeshParams {
  model: string;
  remove_background: boolean;
  detail: MeshDetail;
  /** "-1" for random. */
  seed: string;
}

/** One finished model in the per-user library (IndexedDB). */
export interface SplatResult extends SplatJob {
  id: string;
  title: string;
  /** Missing on records saved before Pixal3D: those are splats. */
  kind?: "splat" | "mesh";
  params: SplatParams | MeshParams;
  /** Gaussians in the stored PLY, or faces in the stored GLB. */
  count: number;
  /** Small JPEG data URL of the source image, for the library grid. */
  thumbnail: string;
  createdAt: number;
}
