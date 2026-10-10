import { userScopedKey } from "../utils/ipc.js";
import {
  elevenlabsDeleteVoice,
  elevenlabsDesignVoice,
  elevenlabsListVoices,
  elevenlabsSaveVoice,
  elevenlabsSubscription,
  sceneEstimateTakes,
  sceneLoadTake,
  sceneRenderTake,
} from "../utils/api.js";
import type {
  ElevenLabsSubscription,
  ElevenLabsVoice,
  SpeechSettings,
  TakeRequest,
  VoiceBrief,
  VoicePreview,
} from "../types/scene.js";

const STORAGE_KEY = "mooshieui.animeScene.v1";

export interface SceneTake {
  takeId: string;
  takeIndex: number;
  /** Credits ElevenLabs reported for this take, when it reported them. */
  credits: number | null;
}

export interface SceneLine {
  id: string;
  text: string;
  /** Delivery note, sent as an ElevenLabs audio tag before the line. */
  delivery: string;
  takes: SceneTake[];
  chosenTakeId: string | null;
  /** A provider refusal for this line. Final when the character is a minor. */
  blocked: string | null;
}

/** One line of a cost confirmation. */
export interface CostItem {
  labelKey: string;
  params?: Record<string, string | number>;
}

export interface PendingConfirm {
  titleKey: string;
  items: CostItem[];
  noteKeys: string[];
  resolve: (ok: boolean) => void;
}

interface Persisted {
  characterName: string;
  isMinor: boolean;
  designLocked: boolean;
  designLockedMessage: string | null;
  voiceId: string | null;
  brief: VoiceBrief;
  previewText: string;
  speech: SpeechSettings;
  lines: SceneLine[];
}

function newId(): string {
  return crypto.randomUUID?.() || `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function emptyBrief(): VoiceBrief {
  return { language: "", gender: "", age: "", quality: "", persona: "", emotions: "", delivery: "" };
}

function defaultSpeech(): SpeechSettings {
  // 0.35 stability gave the prototype expressive, usable takes.
  return { stability: 0.35, similarity: 0.75, language_code: "ja", normalize_text: false, seed: null };
}

/** The text ElevenLabs is asked to speak: the delivery note as an audio tag
 *  next to the line it shapes (rule E1). */
export function spokenText(line: Pick<SceneLine, "text" | "delivery">): string {
  const text = line.text.trim();
  const delivery = line.delivery.trim().replace(/[[\]]/g, "");
  return delivery ? `[${delivery}] ${text}` : text;
}

function base64ToBlobUrl(base64: string, type: string): string {
  const raw = atob(base64);
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  return URL.createObjectURL(new Blob([bytes], { type }));
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

class AnimeSceneStore {
  characterName = $state("");
  /** Turns on the minor-safety rules: a provider refusal is final. */
  isMinor = $state(false);
  /** Set when Voice Design refused a minor-flagged character. Final. */
  designLocked = $state(false);
  designLockedMessage = $state<string | null>(null);
  voiceId = $state<string | null>(null);
  brief = $state<VoiceBrief>(emptyBrief());
  previewText = $state("");
  speech = $state<SpeechSettings>(defaultSpeech());
  lines = $state<SceneLine[]>([]);

  // Not persisted.
  subscription = $state<ElevenLabsSubscription | null>(null);
  voices = $state<ElevenLabsVoice[]>([]);
  accountLoading = $state(false);
  accountError = $state<string | null>(null);
  designing = $state(false);
  designError = $state<string | null>(null);
  designBlocked = $state<string | null>(null);
  lastDescription = $state("");
  previews = $state<VoicePreview[]>([]);
  savingVoice = $state(false);
  rendering = $state(false);
  renderError = $state<string | null>(null);
  pendingConfirm = $state<PendingConfirm | null>(null);
  /** take id -> object URL for playback. */
  audioUrls = $state<Record<string, string>>({});

  constructor() {
    this.loadSettings();
  }

  get selectedVoice(): ElevenLabsVoice | null {
    return this.voices.find((v) => v.voice_id === this.voiceId) ?? null;
  }

  /** Credits left this period, when ElevenLabs reports both numbers. */
  get remainingCredits(): number | null {
    const s = this.subscription;
    if (s?.character_limit == null || s.character_count == null) return null;
    return Math.max(0, s.character_limit - s.character_count);
  }

  get slotsFull(): boolean {
    const s = this.subscription;
    return s?.voice_limit != null && s.voice_slots_used != null && s.voice_slots_used >= s.voice_limit;
  }

  loadSettings() {
    try {
      const raw = localStorage.getItem(userScopedKey(STORAGE_KEY));
      if (!raw) return;
      const p = JSON.parse(raw) as Partial<Persisted>;
      if (typeof p.characterName === "string") this.characterName = p.characterName;
      if (typeof p.isMinor === "boolean") this.isMinor = p.isMinor;
      if (typeof p.designLocked === "boolean") this.designLocked = p.designLocked;
      if (p.designLockedMessage !== undefined) this.designLockedMessage = p.designLockedMessage ?? null;
      if (p.voiceId !== undefined) this.voiceId = p.voiceId ?? null;
      if (p.brief && typeof p.brief === "object") this.brief = { ...emptyBrief(), ...p.brief };
      if (typeof p.previewText === "string") this.previewText = p.previewText;
      if (p.speech && typeof p.speech === "object") this.speech = { ...defaultSpeech(), ...p.speech };
      if (Array.isArray(p.lines)) {
        this.lines = p.lines
          .filter((l) => l && typeof l.text === "string")
          .map((l) => ({
            id: typeof l.id === "string" ? l.id : newId(),
            text: l.text,
            delivery: typeof l.delivery === "string" ? l.delivery : "",
            takes: Array.isArray(l.takes) ? l.takes : [],
            chosenTakeId: l.chosenTakeId ?? null,
            blocked: l.blocked ?? null,
          }));
      }
    } catch {
      /* corrupt or unavailable storage: start fresh */
    }
  }

  saveSettings() {
    const data: Persisted = {
      characterName: this.characterName,
      isMinor: this.isMinor,
      designLocked: this.designLocked,
      designLockedMessage: this.designLockedMessage,
      voiceId: this.voiceId,
      brief: this.brief,
      previewText: this.previewText,
      speech: this.speech,
      lines: this.lines,
    };
    try {
      localStorage.setItem(userScopedKey(STORAGE_KEY), JSON.stringify(data));
    } catch {
      /* storage full or blocked: the scene still works for this session */
    }
  }

  /** Start a new scene. Clears the character, its minor flag and any lock. */
  resetScene() {
    this.characterName = "";
    this.isMinor = false;
    this.designLocked = false;
    this.designLockedMessage = null;
    this.designBlocked = null;
    this.brief = emptyBrief();
    this.previewText = "";
    this.previews = [];
    this.lines = [];
    this.saveSettings();
  }

  setMinor(value: boolean) {
    // Once a refusal has locked this character, the flag cannot be turned
    // off: that would be a way around the refusal.
    if (this.designLocked && !value) return;
    this.isMinor = value;
    this.saveSettings();
  }

  async refreshAccount() {
    this.accountLoading = true;
    this.accountError = null;
    try {
      const [sub, voices] = await Promise.all([elevenlabsSubscription(), elevenlabsListVoices()]);
      this.subscription = sub;
      this.voices = voices;
    } catch (e) {
      this.accountError = errorText(e);
    } finally {
      this.accountLoading = false;
    }
  }

  /** Ask the page to show an itemized estimate; resolves true on confirm. */
  confirm(titleKey: string, items: CostItem[], noteKeys: string[]): Promise<boolean> {
    return new Promise((resolve) => {
      this.pendingConfirm = {
        titleKey,
        items,
        noteKeys,
        resolve: (ok) => {
          this.pendingConfirm = null;
          resolve(ok);
        },
      };
    });
  }

  private creditItems(): CostItem[] {
    const remaining = this.remainingCredits;
    return remaining === null ? [] : [{ labelKey: "scene.cost.credits_left", params: { credits: remaining.toLocaleString() } }];
  }

  async designVoice() {
    if (this.designLocked || this.designing) return;
    const characters = this.previewText.trim().length;
    const ok = await this.confirm(
      "scene.cost.design_title",
      [{ labelKey: "scene.cost.design_item", params: { characters } }, ...this.creditItems()],
      ["scene.cost.elevenlabs_note"],
    );
    if (!ok) return;
    this.designing = true;
    this.designError = null;
    this.designBlocked = null;
    this.previews = [];
    try {
      const response = await elevenlabsDesignVoice({
        brief: this.brief,
        preview_text: this.previewText,
        seed: null,
      });
      this.lastDescription = response.description;
      if (response.result.status === "blocked") {
        this.designBlocked = response.result.message;
        if (this.isMinor) {
          // Final for a minor-flagged character: no redesign, no rewording.
          this.designLocked = true;
          this.designLockedMessage = response.result.message;
          this.saveSettings();
        }
      } else {
        this.previews = response.result.value.previews;
      }
    } catch (e) {
      this.designError = errorText(e);
    } finally {
      this.designing = false;
      void this.refreshSubscriptionQuietly();
    }
  }

  previewUrl(preview: VoicePreview): string {
    const key = `preview:${preview.generated_voice_id}`;
    if (!this.audioUrls[key]) {
      this.audioUrls = {
        ...this.audioUrls,
        [key]: base64ToBlobUrl(preview.audio_base_64, preview.media_type || "audio/mpeg"),
      };
    }
    return this.audioUrls[key];
  }

  async savePreview(preview: VoicePreview, name: string) {
    this.savingVoice = true;
    this.designError = null;
    try {
      const voice = await elevenlabsSaveVoice(name, this.lastDescription, preview.generated_voice_id);
      this.voiceId = voice.voice_id;
      this.previews = [];
      this.saveSettings();
      await this.refreshAccount();
    } catch (e) {
      this.designError = errorText(e);
    } finally {
      this.savingVoice = false;
    }
  }

  async deleteVoice(voiceId: string) {
    try {
      await elevenlabsDeleteVoice(voiceId);
      if (this.voiceId === voiceId) this.voiceId = null;
      this.saveSettings();
      await this.refreshAccount();
    } catch (e) {
      this.accountError = errorText(e);
    }
  }

  selectVoice(voiceId: string | null) {
    this.voiceId = voiceId;
    this.saveSettings();
  }

  addLine() {
    this.lines = [...this.lines, { id: newId(), text: "", delivery: "", takes: [], chosenTakeId: null, blocked: null }];
    this.saveSettings();
  }

  updateLine(id: string, patch: Partial<Pick<SceneLine, "text" | "delivery">>) {
    this.lines = this.lines.map((l) => {
      if (l.id !== id) return l;
      // A minor's refused line stays refused; rewording it is not offered.
      if (l.blocked && this.isMinor) return l;
      // Takes belong to the text they were rendered from. They stay on disk,
      // so going back to that text finds them in the cache for free.
      return { ...l, ...patch, takes: [], chosenTakeId: null, blocked: null };
    });
    this.saveSettings();
  }

  removeLine(id: string) {
    this.lines = this.lines.filter((l) => l.id !== id);
    this.saveSettings();
  }

  moveLine(id: string, delta: number) {
    const i = this.lines.findIndex((l) => l.id === id);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= this.lines.length) return;
    const next = [...this.lines];
    [next[i], next[j]] = [next[j], next[i]];
    this.lines = next;
    this.saveSettings();
  }

  chooseTake(lineId: string, takeId: string) {
    this.lines = this.lines.map((l) => (l.id === lineId ? { ...l, chosenTakeId: takeId } : l));
    this.saveSettings();
  }

  /** Lines that can be rendered: some text, and not refused for a minor. */
  private renderable(line: SceneLine): boolean {
    return line.text.trim() !== "" && !(line.blocked && this.isMinor);
  }

  private requestFor(line: SceneLine, takeIndex: number): TakeRequest {
    return {
      voice_id: this.voiceId ?? "",
      text: spokenText(line),
      settings: { ...this.speech },
      take_index: takeIndex,
    };
  }

  /**
   * Render takes. `extra` asks for one more take of each line (a new take
   * number); otherwise only lines without a take for the current text are
   * rendered. Shows the itemized estimate first and stops unless confirmed.
   */
  async renderTakes(lineIds: string[], extra: boolean) {
    if (this.rendering || !this.voiceId) return;
    const jobs = this.lines
      .filter((l) => lineIds.includes(l.id) && this.renderable(l))
      .map((l) => ({ line: l, request: this.requestFor(l, extra ? l.takes.length : 0) }));
    if (jobs.length === 0) return;
    this.renderError = null;
    let estimate;
    try {
      estimate = await sceneEstimateTakes(jobs.map((j) => j.request));
    } catch (e) {
      this.renderError = errorText(e);
      return;
    }
    if (estimate.requests > 0) {
      const ok = await this.confirm(
        "scene.cost.takes_title",
        [
          {
            labelKey: "scene.cost.takes_item",
            params: { requests: estimate.requests, characters: estimate.characters },
          },
          ...(estimate.cached > 0 ? [{ labelKey: "scene.cost.takes_cached", params: { count: estimate.cached } }] : []),
          ...this.creditItems(),
        ],
        ["scene.cost.elevenlabs_note"],
      );
      if (!ok) return;
    }
    this.rendering = true;
    try {
      // Sequential on purpose: one paid request at a time.
      for (const job of jobs) {
        const result = await sceneRenderTake(job.request);
        if (result.status === "blocked") {
          this.lines = this.lines.map((l) => (l.id === job.line.id ? { ...l, blocked: result.message } : l));
          continue;
        }
        const take = result.value;
        this.audioUrls = { ...this.audioUrls, [take.take_id]: base64ToBlobUrl(take.audio_base64, take.media_type) };
        this.lines = this.lines.map((l) => {
          if (l.id !== job.line.id) return l;
          const known = l.takes.some((t) => t.takeId === take.take_id);
          const takes = known
            ? l.takes
            : [...l.takes, { takeId: take.take_id, takeIndex: job.request.take_index, credits: take.credits }];
          return { ...l, takes, chosenTakeId: l.chosenTakeId ?? take.take_id, blocked: null };
        });
        this.saveSettings();
      }
    } catch (e) {
      this.renderError = errorText(e);
    } finally {
      this.rendering = false;
      this.saveSettings();
      void this.refreshSubscriptionQuietly();
    }
  }

  /** Object URL for a take, loading it from disk on first use. Free. */
  async takeUrl(takeId: string): Promise<string | null> {
    if (this.audioUrls[takeId]) return this.audioUrls[takeId];
    try {
      const take = await sceneLoadTake(takeId);
      const url = base64ToBlobUrl(take.audio_base64, take.media_type);
      this.audioUrls = { ...this.audioUrls, [takeId]: url };
      return url;
    } catch {
      return null;
    }
  }

  private async refreshSubscriptionQuietly() {
    try {
      this.subscription = await elevenlabsSubscription();
    } catch {
      /* the readout just stays as it was */
    }
  }
}

export const animeScene = new AnimeSceneStore();
