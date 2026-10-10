/** Types for the anime scene pipeline (`src-tauri/src/cloud`). Field names
 *  match the Rust structs, so request bodies use snake_case. */

export interface ElevenLabsSubscription {
  tier: string | null;
  character_count: number | null;
  character_limit: number | null;
  voice_slots_used: number | null;
  voice_limit: number | null;
  next_character_count_reset_unix: number | null;
}

export interface ElevenLabsVoice {
  voice_id: string;
  name: string;
  category: string | null;
  description: string | null;
  preview_url: string | null;
}

export interface VoiceBrief {
  language: string;
  gender: string;
  age: string;
  quality: string;
  persona: string;
  emotions: string;
  delivery: string;
}

export interface VoicePreview {
  generated_voice_id: string;
  audio_base_64: string;
  media_type: string | null;
  duration_secs: number | null;
}

/** A provider refusal is a normal, final outcome, never retried. */
export type Moderated<T> =
  | { status: "ok"; value: T }
  | { status: "blocked"; message: string };

export interface DesignResponse {
  description: string;
  result: Moderated<{ previews: VoicePreview[]; text: string | null }>;
}

export interface SpeechSettings {
  stability: number;
  similarity: number;
  language_code: string;
  normalize_text: boolean;
  seed: number | null;
}

export interface TakeRequest {
  voice_id: string;
  text: string;
  settings: SpeechSettings;
  take_index: number;
}

export interface TakeInfo {
  take_id: string;
  audio_base64: string;
  media_type: string;
  credits: number | null;
  cached: boolean;
}

export interface SpeechEstimate {
  requests: number;
  characters: number;
  cached: number;
}

// --- Video step (Phase 2) ---

export type VideoModelId = "fal_seedance25";

/** What a video model can do. The page reads limits from here and never
 *  branches on a provider's name. */
export interface VideoCapabilities {
  model: VideoModelId;
  provider: "elevenlabs" | "fal" | "segmind";
  label: string;
  min_seconds: number;
  max_seconds: number;
  max_images: number;
  max_image_bytes: number;
  max_audio: number;
  audio_min_seconds: number;
  audio_max_seconds: number;
  max_audio_bytes: number;
  resolutions: string[];
  aspect_ratios: string[];
  draft: boolean;
  audio_reference: boolean;
  price_checked: string;
}

/** What one reference image controls. `shot` is a 0-based shot index. */
export type ImageRole = { kind: "character" } | { kind: "location" } | { kind: "shot"; shot: number };

export interface SceneImageInput {
  filename: string;
  role: ImageRole;
}

export interface SceneLineInput {
  line_id: string;
  take_id: string;
  text: string;
  delivery: string;
}

export interface SceneShotInput {
  framing: string;
  action: string;
  line_id: string | null;
  /** Pause before the shot's line, or the whole length of a silent shot. */
  lead: number;
}

export interface SceneRequest {
  model: VideoModelId;
  resolution: string;
  draft: boolean;
  aspect: string;
  seed: number | null;
  continuous: boolean;
  character_name: string;
  character_traits: string;
  language: string;
  starting_state: string;
  ending_state: string;
  ambience: string;
  minor: boolean;
  voice_id: string;
  lines: SceneLineInput[];
  shots: SceneShotInput[];
  tail: number;
  images: SceneImageInput[];
  /** The user's edit of the built prompt; null sends the built one. */
  prompt_override: string | null;
  /** Put each measured take's mouth timing in the prompt. */
  mouth_map: boolean;
}

export interface VideoEstimate {
  usd: number;
  resolution: string;
  seconds: number;
  checked: string;
}

export interface ScenePlan {
  prompt: string;
  builder_version: number;
  seconds: number;
  track_seconds: number;
  windows: { line_id: string; start: number; end: number }[];
  shots: { start: number; end: number }[];
  estimate: VideoEstimate;
  model_label: string;
  /** Lines whose mouth timing is in the prompt. */
  mouth_measured: number;
  /** Lines whose take has not been measured yet. */
  mouth_missing: number;
}

export interface AlignEstimate {
  takes: number;
  seconds: number;
  usd: number;
  cached: number;
  checked: string;
}

/** What can still be done with a finished scene clip. */
export interface ClipInfo {
  model_label: string;
  seconds: number;
  resolution: string;
  draft: boolean;
  draft_expires_unix: number | null;
  /** The price of completing the draft at 1080p, while it can be. */
  upgrade: VideoEstimate | null;
  /** The user's own voice track can replace the clip's sound. */
  exact_voice: boolean;
}

/** The gallery entry a finished clip became, as `comfyui:output_video` sends it. */
export interface SavedSceneClip {
  video_filename: string;
  poster_filename: string | null;
  duration_seconds: number;
  fps: number;
  width: number;
  height: number;
}

export interface KeyframeRequest {
  /** Gallery filename of the character image. */
  reference: string;
  character_tags: string;
  shot_tags: string;
  aspect: string;
  minor: boolean;
}
