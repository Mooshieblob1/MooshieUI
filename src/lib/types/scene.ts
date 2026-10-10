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
