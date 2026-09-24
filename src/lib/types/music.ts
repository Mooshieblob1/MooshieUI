export interface ReferenceSong {
  id: number;
  title: string;
  artist: string;
  album: string;
  genre: string;
  year: string;
  duration_seconds: number | null;
  url: string;
}
export interface MusicReferenceContext {
  song: ReferenceSong;
  sources: { title: string; url: string; text: string }[];
}
export interface MusicReferenceDraft {
  style: string;
  estimates: string[];
}

export interface MusicParams {
  title?: string;
  checkpoint: string;
  style: string;
  lyrics: string;
  planning: "full" | "melody" | "off";
  abc: string;
  cover?: boolean;
  max_duration: number;
  steps: number;
  seed: string;
  task?: "audio" | "plan";
  sampling?: MusicSampling;
  lineage?: MusicLineage;
}

export interface MusicSampling {
  temperature: number;
  top_p: number;
  top_k: number;
  repetition_penalty: number;
  cfg_scale: number;
  max_abc_tokens: number;
}

export interface MusicLineage {
  project_id: string;
  parent_id?: string;
  change?: string;
  comparison?: { match: boolean; differences: string[]; constraints: string[] };
  candidate_group?: string;
  candidate_index?: number;
}

export interface MusicMetadata {
  abc_truncated: boolean | null;
  semantic_truncated: boolean | null;
  score_source: "generated" | "provided" | "transcribed" | "none" | null;
  abc_tokens?: number | null;
  semantic_frames?: number | null;
  generated_seconds?: number | null;
  cfg_scale?: number | null;
}

export interface MusicPlan extends MusicJob {
  abc: string;
  params: MusicParams;
  metadata?: MusicMetadata;
  createdAt: number;
}

export interface MusicBatch {
  id: string;
  params: MusicParams;
  createdAt: number;
  attempts: { seed: string; status: "pending" | "running" | "completed" | "failed" | "cancelled"; prompt_id?: string; error?: string }[];
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
  review?: import("../utils/musicReview.js").MusicReviewRecord;
  metadata?: MusicMetadata;
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
  kind?: "audio" | "plan" | "transcription";
  backend?: string;
}

export interface MusicCapabilities {
  missing_nodes: string[];
  checkpoints: string[];
  extended?: boolean;
  cover_missing_nodes?: string[];
  audio_encoders?: string[];
  workers?: { worker_id: number; missing_nodes: string[]; checkpoints: string[]; extended: boolean; cover_missing_nodes: string[]; audio_encoders: string[] }[];
}

export interface CoverTranscriptionStatus {
  status: "running" | "completed" | "error" | "cancelled";
  abc?: string;
  warnings?: string[];
  error?: string;
  kind?: "audio" | "plan" | "transcription";
  metadata?: MusicMetadata;
}

export interface MusicStatus {
  status: "queued" | "running" | "completed" | "error" | "missing";
  filename?: string;
  abc?: string;
  error?: string;
  kind?: "audio" | "plan" | "transcription";
  metadata?: MusicMetadata;
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
