import { generation } from "./generation.svelte.js";
import { triggerSync } from "../utils/syncTrigger.js";
import { registerTimelineActivity, registerTimelineCompiler } from "../utils/timelineProvider.js";
import {
  H3_FPS,
  H3_MAX_REF_AUDIOS,
  H3_MAX_REF_VIDEOS,
  H3_MAX_TIMELINE_CHARACTERS,
} from "../utils/videoParams.js";

/**
 * Client-side model of the H3 Director's `timeline_data` payload.
 *
 * The vendored node (`comfyui-nodes/minimax_director/minimax_plan.py`) reads a
 * single JSON string and derives every shot, reference and audio cue from it.
 * These interfaces use MooshieUI naming; `compileTimelineData()` is the only
 * place that translates to the node's key names, which are documented inline
 * there. All timing is in frames at `H3_FPS` (24), matching the node's
 * `frame_rate` widget.
 */

const TIMELINE_KEY = "mooshieui.videoTimeline.v1";

/** Default shot length when a new segment is added: two seconds. */
const DEFAULT_SEGMENT_FRAMES = H3_FPS * 2;

export type TimelineSegmentKind = "image" | "video";

/**
 * H3 accepts two prompt dialects. `minimax` is the model's own structured
 * format; `comfyui` emits plain prose. Mirrors `FORMAT_MINIMAX`/`FORMAT_COMFYUI`.
 */
export type TimelinePromptFormat = "minimax" | "comfyui";

/** One shot on the main track: a still or a clip plus the prompt covering it. */
export interface TimelineSegment {
  id: string;
  /** Frame this shot starts on. */
  start: number;
  /** Shot length in frames. */
  length: number;
  kind: TimelineSegmentKind;
  prompt: string;
  /** ComfyUI input-folder filename, resolved by the node's `resolve_input_path`. */
  file: string;
  /** Original filename, shown in the UI and echoed back in node errors. */
  fileName: string;
  /** Pin this frame as the window's last frame instead of letting the node classify it. */
  isEndFrame: boolean;
  /** Frames to skip into the source clip. Video shots only. */
  trimStart: number;
}

/** A recurring cast member, addressable from prompts as `@character1`-`@character3`. */
export interface TimelineCharacter {
  id: string;
  description: string;
  /** Input-folder filenames; the node resolves each by name, no base64 needed. */
  files: string[];
}

/** A reference clip on the motion track (ref2va only, at most 3, 2-15 s each). */
export interface TimelineMotionSegment {
  id: string;
  start: number;
  length: number;
  trimStart: number;
  file: string;
  fileName: string;
}

/** A reference cue on the audio track (ref2va only, at most 3). */
export interface TimelineAudioSegment {
  id: string;
  start: number;
  length: number;
  trimStart: number;
  file: string;
  fileName: string;
}

interface PersistedTimeline {
  enabled: boolean;
  segments: TimelineSegment[];
  characters: TimelineCharacter[];
  motionSegments: TimelineMotionSegment[];
  audioSegments: TimelineAudioSegment[];
  overallSoundscape: string;
  nonDiegeticMusic: string;
  promptFormat: TimelinePromptFormat;
}

function newId(): string {
  return crypto.randomUUID?.() || `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function asString(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function asFrames(value: unknown, fallback: number, min: number): number {
  const n = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(n)) return fallback;
  return Math.max(min, Math.round(n));
}

class VideoTimelineStore {
  /** Master switch: off routes generation back to the native H3 template. */
  enabled = $state(false);
  segments = $state<TimelineSegment[]>([]);
  characters = $state<TimelineCharacter[]>([]);
  motionSegments = $state<TimelineMotionSegment[]>([]);
  audioSegments = $state<TimelineAudioSegment[]>([]);
  /** Ambient sound description applied across the whole timeline. */
  overallSoundscape = $state("");
  /** Score/background music description, kept separate from diegetic sound. */
  nonDiegeticMusic = $state("");
  promptFormat = $state<TimelinePromptFormat>("minimax");

  private saveTimer: ReturnType<typeof setTimeout> | null = null;

  constructor() {
    this.loadSettings();
    registerTimelineCompiler((globalPrompt) => {
      const data = this.compileTimelineData(globalPrompt);
      if (data === null) return null;
      return {
        data,
        useCustomMotion: this.useCustomMotion,
        useCustomAudio: this.useCustomAudio,
      };
    });
    registerTimelineActivity(() => this.enabled && this.hasContent);
  }

  /** True once the timeline carries something the node could actually render. */
  get hasContent(): boolean {
    return this.segments.some((s) => s.prompt.trim() !== "" || s.file !== "");
  }

  /** Drives the node's `use_custom_motion` widget (a widget, not timeline data). */
  get useCustomMotion(): boolean {
    return this.motionSegments.some((s) => s.file !== "");
  }

  /** Drives the node's `use_custom_audio` widget. */
  get useCustomAudio(): boolean {
    return this.audioSegments.some((s) => s.file !== "");
  }

  /** Last frame covered by any track, so the UI can size the strip. */
  get totalFrames(): number {
    const ends = [
      ...this.segments,
      ...this.motionSegments,
      ...this.audioSegments,
    ].map((s) => s.start + s.length);
    return ends.length > 0 ? Math.max(...ends) : 0;
  }

  // --- Mutations -----------------------------------------------------------
  // Every mutator reassigns its array (Svelte 5 runes do not track `.push()`)
  // and schedules a debounced save, so the UI never has to remember to persist.

  setEnabled(value: boolean) {
    this.enabled = value;
    this.scheduleSave();
  }

  setPromptFormat(value: TimelinePromptFormat) {
    this.promptFormat = value;
    this.scheduleSave();
  }

  setOverallSoundscape(value: string) {
    this.overallSoundscape = value;
    this.scheduleSave();
  }

  setNonDiegeticMusic(value: string) {
    this.nonDiegeticMusic = value;
    this.scheduleSave();
  }

  /** Append a shot after the current end of the main track. */
  addSegment(init: Partial<TimelineSegment> = {}): TimelineSegment {
    const last = this.segments[this.segments.length - 1];
    const segment: TimelineSegment = {
      id: newId(),
      start: last ? last.start + last.length : 0,
      length: DEFAULT_SEGMENT_FRAMES,
      kind: "image",
      prompt: "",
      file: "",
      fileName: "",
      isEndFrame: false,
      trimStart: 0,
      ...init,
    };
    this.segments = [...this.segments, segment];
    this.scheduleSave();
    return segment;
  }

  updateSegment(id: string, patch: Partial<TimelineSegment>) {
    this.segments = this.segments.map((s) => (s.id === id ? { ...s, ...patch, id: s.id } : s));
    this.scheduleSave();
  }

  removeSegment(id: string) {
    this.segments = this.segments.filter((s) => s.id !== id);
    this.scheduleSave();
  }

  addCharacter(init: Partial<TimelineCharacter> = {}): TimelineCharacter | null {
    if (this.characters.length >= H3_MAX_TIMELINE_CHARACTERS) return null;
    const character: TimelineCharacter = {
      id: newId(),
      description: "",
      files: [],
      ...init,
    };
    this.characters = [...this.characters, character];
    this.scheduleSave();
    return character;
  }

  updateCharacter(id: string, patch: Partial<TimelineCharacter>) {
    this.characters = this.characters.map((c) => (c.id === id ? { ...c, ...patch, id: c.id } : c));
    this.scheduleSave();
  }

  removeCharacter(id: string) {
    this.characters = this.characters.filter((c) => c.id !== id);
    this.scheduleSave();
  }

  addMotionSegment(init: Partial<TimelineMotionSegment> = {}): TimelineMotionSegment | null {
    if (this.motionSegments.length >= H3_MAX_REF_VIDEOS) return null;
    const segment: TimelineMotionSegment = {
      id: newId(),
      start: 0,
      length: DEFAULT_SEGMENT_FRAMES,
      trimStart: 0,
      file: "",
      fileName: "",
      ...init,
    };
    this.motionSegments = [...this.motionSegments, segment];
    this.scheduleSave();
    return segment;
  }

  updateMotionSegment(id: string, patch: Partial<TimelineMotionSegment>) {
    this.motionSegments = this.motionSegments.map((s) =>
      s.id === id ? { ...s, ...patch, id: s.id } : s,
    );
    this.scheduleSave();
  }

  removeMotionSegment(id: string) {
    this.motionSegments = this.motionSegments.filter((s) => s.id !== id);
    this.scheduleSave();
  }

  addAudioSegment(init: Partial<TimelineAudioSegment> = {}): TimelineAudioSegment | null {
    if (this.audioSegments.length >= H3_MAX_REF_AUDIOS) return null;
    const segment: TimelineAudioSegment = {
      id: newId(),
      start: 0,
      length: DEFAULT_SEGMENT_FRAMES,
      trimStart: 0,
      file: "",
      fileName: "",
      ...init,
    };
    this.audioSegments = [...this.audioSegments, segment];
    this.scheduleSave();
    return segment;
  }

  updateAudioSegment(id: string, patch: Partial<TimelineAudioSegment>) {
    this.audioSegments = this.audioSegments.map((s) =>
      s.id === id ? { ...s, ...patch, id: s.id } : s,
    );
    this.scheduleSave();
  }

  removeAudioSegment(id: string) {
    this.audioSegments = this.audioSegments.filter((s) => s.id !== id);
    this.scheduleSave();
  }

  /** Wipe every track but keep the enabled flag and the format choice. */
  clear() {
    this.segments = [];
    this.characters = [];
    this.motionSegments = [];
    this.audioSegments = [];
    this.overallSoundscape = "";
    this.nonDiegeticMusic = "";
    this.scheduleSave();
  }

  // --- Compilation ---------------------------------------------------------

  /**
   * Serialize the timeline into the node's `timeline_data` string, or `null`
   * when there is nothing to render (the caller then falls back to the native
   * H3 template rather than submitting an empty Director graph).
   */
  compileTimelineData(globalPrompt: string): string | null {
    if (!this.enabled || !this.hasContent) return null;

    // The node derives reference mode from this string alone
    // (`str(...).upper() != "OFF"`). `videoVariant` stays the single source of
    // truth so the panel toggle and the timeline can never disagree.
    const refModeOn = generation.videoVariant === "ref2va";

    const payload = {
      reference_mode: refModeOn ? "ON" : "OFF",
      prompt_format: this.promptFormat,
      global_prompt: globalPrompt,
      overall_soundscape: this.overallSoundscape.trim(),
      non_diegetic_music: this.nonDiegeticMusic.trim(),
      characters: this.characters
        .filter((c) => c.description.trim() !== "" || c.files.length > 0)
        .slice(0, H3_MAX_TIMELINE_CHARACTERS)
        .map((c) => ({
          description: c.description.trim(),
          // `load_image_source(b64, name)` resolves `name` through
          // `resolve_input_path()`, so an empty b64 plus the input filename is
          // the whole contract - no data URIs on the wire.
          images: c.files.filter((f) => f !== "").map((f) => ({ b64: "", name: f })),
        })),
      // Stills and clips both travel in `imageFile`; `_load_event_tensor()`
      // switches on `type`, not on the key name.
      segments: this.segments
        .filter((s) => s.file !== "" || s.prompt.trim() !== "")
        .map((s) => ({
          start: Math.max(0, Math.round(s.start)),
          length: Math.max(1, Math.round(s.length)),
          type: s.kind,
          prompt: s.prompt,
          imageFile: s.file,
          fileName: s.fileName,
          isEndFrame: s.isEndFrame,
          trimStart: Math.max(0, Math.round(s.trimStart)),
        })),
      // The node ignores both reference tracks when reference mode is off, but
      // they are still emitted so flipping the variant does not lose the work.
      motionSegments: this.motionSegments
        .filter((s) => s.file !== "")
        .slice(0, H3_MAX_REF_VIDEOS)
        .map((s) => ({
          start: Math.max(0, Math.round(s.start)),
          length: Math.max(1, Math.round(s.length)),
          trimStart: Math.max(0, Math.round(s.trimStart)),
          videoFile: s.file,
          fileName: s.fileName,
        })),
      audioSegments: this.audioSegments
        .filter((s) => s.file !== "")
        .slice(0, H3_MAX_REF_AUDIOS)
        .map((s) => ({
          start: Math.max(0, Math.round(s.start)),
          length: Math.max(1, Math.round(s.length)),
          trimStart: Math.max(0, Math.round(s.trimStart)),
          audioFile: s.file,
          fileName: s.fileName,
        })),
    };

    return JSON.stringify(payload);
  }

  // --- Persistence ---------------------------------------------------------

  loadSettings() {
    try {
      const raw = localStorage.getItem(TIMELINE_KEY);
      if (!raw) return;
      this.hydrate(JSON.parse(raw));
    } catch (e) {
      console.error("Failed to load video timeline:", e);
    }
  }

  /** Persist after a short debounce; prompt and drag edits are high-frequency. */
  private scheduleSave() {
    if (this.saveTimer !== null) clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => {
      this.saveTimer = null;
      this.saveSettings();
    }, 500);
  }

  /** Flush a pending debounced save immediately (e.g. before generating). */
  flush() {
    if (this.saveTimer !== null) {
      clearTimeout(this.saveTimer);
      this.saveTimer = null;
      this.saveSettings();
    }
  }

  saveSettings() {
    try {
      localStorage.setItem(TIMELINE_KEY, JSON.stringify(this.collectPrefs()));
      triggerSync();
    } catch (e) {
      console.error("Failed to save video timeline:", e);
    }
  }

  collectPrefs(): PersistedTimeline {
    return {
      enabled: this.enabled,
      segments: this.segments,
      characters: this.characters,
      motionSegments: this.motionSegments,
      audioSegments: this.audioSegments,
      overallSoundscape: this.overallSoundscape,
      nonDiegeticMusic: this.nonDiegeticMusic,
      promptFormat: this.promptFormat,
    };
  }

  applyServerPrefs(data: unknown): void {
    try {
      this.hydrate(data);
      localStorage.setItem(TIMELINE_KEY, JSON.stringify(this.collectPrefs()));
    } catch (e) {
      console.error("Failed to apply server prefs (video timeline):", e);
    }
  }

  /**
   * Rebuild state from an untrusted blob (localStorage or the prefs server).
   * Every field is validated and every list entry is normalized, so a payload
   * written by an older build can never produce a segment the compiler would
   * turn into invalid `timeline_data`.
   */
  private hydrate(data: unknown): void {
    const parsed = data as Partial<PersistedTimeline> | null;
    if (!parsed || typeof parsed !== "object") return;

    if (typeof parsed.enabled === "boolean") this.enabled = parsed.enabled;
    if (parsed.promptFormat === "minimax" || parsed.promptFormat === "comfyui") {
      this.promptFormat = parsed.promptFormat;
    }
    if (typeof parsed.overallSoundscape === "string") {
      this.overallSoundscape = parsed.overallSoundscape;
    }
    if (typeof parsed.nonDiegeticMusic === "string") {
      this.nonDiegeticMusic = parsed.nonDiegeticMusic;
    }

    if (Array.isArray(parsed.segments)) {
      this.segments = parsed.segments.map((raw: any) => ({
        id: asString(raw?.id) || newId(),
        start: asFrames(raw?.start, 0, 0),
        length: asFrames(raw?.length, DEFAULT_SEGMENT_FRAMES, 1),
        kind: raw?.kind === "video" ? "video" : "image",
        prompt: asString(raw?.prompt),
        file: asString(raw?.file),
        fileName: asString(raw?.fileName),
        isEndFrame: raw?.isEndFrame === true,
        trimStart: asFrames(raw?.trimStart, 0, 0),
      }));
    }

    if (Array.isArray(parsed.characters)) {
      this.characters = parsed.characters
        .slice(0, H3_MAX_TIMELINE_CHARACTERS)
        .map((raw: any) => ({
          id: asString(raw?.id) || newId(),
          description: asString(raw?.description),
          files: Array.isArray(raw?.files)
            ? raw.files.filter((f: unknown): f is string => typeof f === "string" && f !== "")
            : [],
        }));
    }

    if (Array.isArray(parsed.motionSegments)) {
      this.motionSegments = parsed.motionSegments
        .slice(0, H3_MAX_REF_VIDEOS)
        .map((raw: any) => this.hydrateTrackSegment(raw));
    }

    if (Array.isArray(parsed.audioSegments)) {
      this.audioSegments = parsed.audioSegments
        .slice(0, H3_MAX_REF_AUDIOS)
        .map((raw: any) => this.hydrateTrackSegment(raw));
    }
  }

  /** Motion and audio reference segments share a shape; normalize both here. */
  private hydrateTrackSegment(raw: any): TimelineMotionSegment {
    return {
      id: asString(raw?.id) || newId(),
      start: asFrames(raw?.start, 0, 0),
      length: asFrames(raw?.length, DEFAULT_SEGMENT_FRAMES, 1),
      trimStart: asFrames(raw?.trimStart, 0, 0),
      file: asString(raw?.file),
      fileName: asString(raw?.fileName),
    };
  }
}

export const videoTimeline = new VideoTimelineStore();
