export interface MusicParams {
  title?: string;
  checkpoint: string;
  style: string;
  lyrics: string;
  planning: "full" | "melody" | "off";
  abc: string;
  max_duration: number;
  steps: number;
  seed: string;
}

export interface MusicResult extends MusicJob {
  filename: string;
  abc: string;
  params: MusicParams;
  title?: string;
  createdAt?: number;
  duration?: number;
  saved?: boolean;
  alignment?: import("../utils/lyricTiming.js").LyricAlignment;
}

export interface MusicPlaylist {
  id: string;
  name: string;
  songIds: string[];
  createdAt: number;
}

export interface MusicJob {
  prompt_id: string;
  worker_id: number;
  seed: string;
}

export interface MusicCapabilities {
  missing_nodes: string[];
  checkpoints: string[];
}

export interface MusicStatus {
  status: "queued" | "running" | "completed" | "error" | "missing";
  filename?: string;
  abc?: string;
  error?: string;
}

export type MusicStage = "prepare" | "score" | "compose" | "render" | "decode" | "save" | "retrieve";

export interface MusicExecutionEvent {
  prompt_id?: string;
  node?: string | null;
  value?: number;
  max?: number;
  position?: number;
  exception_message?: string;
  nodes?: string[] | Record<string, { state?: string; value?: number; max?: number }>;
}
