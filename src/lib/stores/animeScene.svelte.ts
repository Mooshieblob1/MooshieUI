import { userScopedKey } from "../utils/ipc.js";
import {
  elevenlabsDeleteVoice,
  elevenlabsDesignVoice,
  elevenlabsListVoices,
  elevenlabsSaveVoice,
  elevenlabsSubscription,
  sceneAlignTakes,
  sceneClipInfo,
  sceneEstimateAlignment,
  sceneEstimateTakes,
  sceneExactVoice,
  sceneGenerate,
  sceneKeyframe,
  sceneLoadTake,
  scenePlan,
  sceneRenderTake,
  sceneResumeJobs,
  sceneUpgradeDraft,
  sceneVideoCapabilities,
} from "../utils/api.js";
import { progress } from "./progress.svelte.js";
import { locale } from "./locale.svelte.js";
import { novelai } from "./novelai.svelte.js";
import { estimateNovelAiCost } from "../utils/novelaiCost.js";
import type {
  ClipInfo,
  ElevenLabsSubscription,
  ElevenLabsVoice,
  ImageRole,
  SavedSceneClip,
  ScenePlan,
  SceneRequest,
  SpeechSettings,
  VideoCapabilities,
  VideoModelId,
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

/** A reference image from the gallery and the one job it does. */
export interface SceneReference {
  filename: string;
  /** Display only. */
  thumb: string | null;
  role: ImageRole;
}

export interface SceneShot {
  id: string;
  framing: string;
  action: string;
  lineId: string | null;
  /** Pause before the line, or the whole length of a shot without one. */
  lead: number;
  /** NovelAI tags for this shot's keyframe: framing, pose, expression, place. */
  keyframeTags: string;
}

export interface SceneVideoSettings {
  model: VideoModelId;
  resolution: string;
  /** A cheap 480p draft. On by default: it is how the prototype iterated. */
  draft: boolean;
  aspect: string;
  seed: number | null;
  continuous: boolean;
  characterTraits: string;
  language: string;
  startingState: string;
  endingState: string;
  ambience: string;
  /** Silence after the last line, in seconds. */
  tail: number;
  /** NovelAI appearance tags for keyframes, `1girl` first. ASCII only. */
  characterTags: string;
  /** Put each measured take's mouth timing in the prompt (rule S10). */
  mouthMap: boolean;
}

/** A finished scene clip in the gallery, newest first. */
export interface SceneClip {
  filename: string;
  /** The prompt id that made it, to tell drafts and upgrades apart. */
  promptId: string;
}

/** How many finished clips the page lists. */
const MAX_CLIPS = 12;

function defaultVideo(): SceneVideoSettings {
  return {
    model: "fal_seedance25",
    resolution: "720p",
    draft: true,
    aspect: "16:9",
    seed: null,
    continuous: false,
    characterTraits: "",
    language: "Japanese",
    startingState: "",
    endingState: "",
    ambience: "",
    tail: 1,
    characterTags: "",
    mouthMap: true,
  };
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
  references: SceneReference[];
  shots: SceneShot[];
  video: SceneVideoSettings;
  clips: SceneClip[];
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
  references = $state<SceneReference[]>([]);
  shots = $state<SceneShot[]>([]);
  video = $state<SceneVideoSettings>(defaultVideo());
  clips = $state<SceneClip[]>([]);

  // Not persisted.
  capabilities = $state<VideoCapabilities[]>([]);
  plan = $state<ScenePlan | null>(null);
  /** The user's edit of the built prompt; null sends the built one. */
  promptOverride = $state<string | null>(null);
  planning = $state(false);
  videoError = $state<string | null>(null);
  starting = $state(false);
  /** Scene renders started from this page that are still running. */
  activeJobs = $state<string[]>([]);
  /** NovelAI keyframe prompt id -> the shot it is for. */
  pendingKeyframes = $state<Record<string, string>>({});
  keyframeError = $state<string | null>(null);
  measuring = $state(false);
  mouthError = $state<string | null>(null);
  /** filename -> what can still be done with it. */
  clipInfo = $state<Record<string, ClipInfo>>({});
  /** The clip an action is running on. */
  clipBusy = $state<string | null>(null);
  clipError = $state<string | null>(null);
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
      if (Array.isArray(p.references)) {
        this.references = p.references
          .filter((r) => r && typeof r.filename === "string" && r.role && typeof r.role.kind === "string")
          // Session blob URLs die with the page; the card falls back to the name.
          .map((r) => ({ ...r, thumb: typeof r.thumb === "string" && !r.thumb.startsWith("blob:") ? r.thumb : null }));
      }
      if (Array.isArray(p.shots)) {
        this.shots = p.shots
          .filter((sh) => sh && typeof sh.framing === "string")
          .map((sh) => ({
            id: typeof sh.id === "string" ? sh.id : newId(),
            framing: sh.framing,
            action: typeof sh.action === "string" ? sh.action : "",
            lineId: sh.lineId ?? null,
            lead: typeof sh.lead === "number" ? sh.lead : 0,
            keyframeTags: typeof sh.keyframeTags === "string" ? sh.keyframeTags : "",
          }));
      }
      if (p.video && typeof p.video === "object") this.video = { ...defaultVideo(), ...p.video };
      if (Array.isArray(p.clips)) {
        this.clips = p.clips
          .filter((c) => c && typeof c.filename === "string" && typeof c.promptId === "string")
          .slice(0, MAX_CLIPS);
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
      references: this.references,
      shots: this.shots,
      video: this.video,
      clips: this.clips,
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
    this.references = [];
    this.shots = [];
    this.video = defaultVideo();
    this.invalidatePlan();
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
    this.invalidatePlan();
    this.saveSettings();
  }

  removeLine(id: string) {
    this.lines = this.lines.filter((l) => l.id !== id);
    this.shots = this.shots.map((sh) => (sh.lineId === id ? { ...sh, lineId: null } : sh));
    this.invalidatePlan();
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
    this.invalidatePlan();
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

  // --- Video step ---

  /** A change to the scene makes the plan, and any edit of its prompt, stale. */
  private invalidatePlan() {
    this.plan = null;
    this.promptOverride = null;
  }

  setPromptOverride(text: string) {
    this.promptOverride = this.plan && text === this.plan.prompt ? null : text;
  }

  get capability(): VideoCapabilities | null {
    return this.capabilities.find((c) => c.model === this.video.model) ?? this.capabilities[0] ?? null;
  }

  /** Lines with a chosen take, which are the only ones a shot can use. */
  get readyLines(): SceneLine[] {
    return this.lines.filter((l) => l.chosenTakeId && !l.blocked);
  }

  async loadCapabilities() {
    try {
      this.capabilities = await sceneVideoCapabilities();
      const caps = this.capability;
      if (caps) {
        // Settings saved against an older model's options fall back to valid ones.
        const v = { ...this.video, model: caps.model };
        if (!caps.resolutions.includes(v.resolution)) v.resolution = caps.resolutions[0];
        if (!caps.aspect_ratios.includes(v.aspect)) v.aspect = caps.aspect_ratios[0];
        if (!caps.draft) v.draft = false;
        this.video = v;
      }
    } catch (e) {
      this.videoError = errorText(e);
    }
  }

  /** Follow scenes a restart left unfinished. Costs nothing. */
  async resumeJobs() {
    try {
      const ids = await sceneResumeJobs();
      for (const id of ids) this.track(id);
    } catch {
      /* nothing to resume, or no key: the page says so elsewhere */
    }
  }

  private track(promptId: string) {
    progress.enqueue(promptId, false, "video", null);
    if (!this.activeJobs.includes(promptId)) this.activeJobs = [...this.activeJobs, promptId];
  }

  /** Drop finished jobs from the page's list. Called from the page's effect. */
  syncActiveJobs() {
    const pending = new Set(progress.pendingPrompts.map((p) => p.promptId));
    const still = this.activeJobs.filter((id) => pending.has(id));
    if (still.length !== this.activeJobs.length) this.activeJobs = still;
  }

  updateVideo(patch: Partial<SceneVideoSettings>) {
    this.video = { ...this.video, ...patch };
    this.invalidatePlan();
    this.saveSettings();
  }

  addReferences(images: { filename: string; thumb: string | null }[]) {
    const max = this.capability?.max_images ?? 30;
    const known = new Set(this.references.map((r) => r.filename));
    const hasCharacter = this.references.some((r) => r.role.kind === "character");
    const added: SceneReference[] = [];
    for (const image of images) {
      if (known.has(image.filename)) continue;
      known.add(image.filename);
      // The first image becomes the character reference; the rest start as
      // the location and can be reassigned.
      const role: ImageRole = !hasCharacter && added.length === 0 ? { kind: "character" } : { kind: "location" };
      added.push({ filename: image.filename, thumb: image.thumb, role });
    }
    this.references = [...this.references, ...added].slice(0, max);
    this.invalidatePlan();
    this.saveSettings();
  }

  setReferenceRole(filename: string, role: ImageRole) {
    this.references = this.references.map((r) => (r.filename === filename ? { ...r, role } : r));
    this.invalidatePlan();
    this.saveSettings();
  }

  removeReference(filename: string) {
    this.references = this.references.filter((r) => r.filename !== filename);
    this.invalidatePlan();
    this.saveSettings();
  }

  addShot() {
    // A new shot takes the next line no shot uses yet, if there is one.
    const used = new Set(this.shots.map((sh) => sh.lineId));
    const next = this.readyLines.find((l) => !used.has(l.id));
    this.shots = [
      ...this.shots,
      {
        id: newId(),
        framing: "",
        action: "",
        lineId: next?.id ?? null,
        lead: next ? (this.shots.length === 0 ? 0 : 0.5) : 1.5,
        keyframeTags: "",
      },
    ];
    this.invalidatePlan();
    this.saveSettings();
  }

  updateShot(id: string, patch: Partial<Omit<SceneShot, "id">>) {
    this.shots = this.shots.map((sh) => (sh.id === id ? { ...sh, ...patch } : sh));
    this.invalidatePlan();
    this.saveSettings();
  }

  removeShot(id: string) {
    const index = this.shots.findIndex((sh) => sh.id === id);
    this.shots = this.shots.filter((sh) => sh.id !== id);
    // Shot references point at shots by position, so later ones shift down.
    this.references = this.references.flatMap((r) => {
      if (r.role.kind !== "shot") return [r];
      if (r.role.shot === index) return [{ ...r, role: { kind: "location" } as ImageRole }];
      return [r.role.shot > index ? { ...r, role: { kind: "shot", shot: r.role.shot - 1 } as ImageRole } : r];
    });
    this.invalidatePlan();
    this.saveSettings();
  }

  moveShot(id: string, delta: number) {
    const i = this.shots.findIndex((sh) => sh.id === id);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= this.shots.length) return;
    const next = [...this.shots];
    [next[i], next[j]] = [next[j], next[i]];
    this.shots = next;
    this.references = this.references.map((r) => {
      if (r.role.kind !== "shot") return r;
      if (r.role.shot === i) return { ...r, role: { kind: "shot", shot: j } };
      if (r.role.shot === j) return { ...r, role: { kind: "shot", shot: i } };
      return r;
    });
    this.invalidatePlan();
    this.saveSettings();
  }

  // --- Keyframes (NovelAI V4.5 + Precise Reference) ---

  get characterReference(): SceneReference | null {
    return this.references.find((r) => r.role.kind === "character") ?? null;
  }

  /** Anlas a keyframe is expected to cost: free on Opus apart from the
   *  Precise Reference surcharge, which Opus does not cover. */
  keyframeAnlas(width: number, height: number): number {
    return estimateNovelAiCost({
      width,
      height,
      steps: 28,
      nSamples: 1,
      strength: 1,
      isOpus: novelai.isOpus,
      vibeEncodes: 0,
      preciseReferences: 1,
    });
  }

  async makeKeyframe(shotId: string, size: { width: number; height: number }) {
    const shot = this.shots.find((sh) => sh.id === shotId);
    const character = this.characterReference;
    if (!shot || !character) return;
    this.keyframeError = null;
    const anlas = this.keyframeAnlas(size.width, size.height);
    const ok = await this.confirm(
      "scene.cost.keyframe_title",
      [
        { labelKey: "scene.cost.keyframe_item", params: { width: size.width, height: size.height, anlas } },
        ...(novelai.subscription ? [{ labelKey: "scene.cost.anlas_left", params: { anlas: novelai.anlas.toLocaleString() } }] : []),
      ],
      ["scene.cost.keyframe_note"],
    );
    if (!ok) return;
    try {
      const id = await sceneKeyframe({
        reference: character.filename,
        character_tags: this.video.characterTags,
        shot_tags: shot.keyframeTags,
        aspect: this.video.aspect,
        minor: this.isMinor,
      });
      this.pendingKeyframes = { ...this.pendingKeyframes, [id]: shotId };
      progress.enqueue(id, false, "txt2img", null);
    } catch (e) {
      this.keyframeError = errorText(e);
    }
  }

  /** Called from App.svelte when a generation finishes: the shot a keyframe
   *  prompt was for, if it was one. */
  resolveKeyframe(promptId: string): string | null {
    const shotId = this.pendingKeyframes[promptId];
    if (shotId === undefined) return null;
    const { [promptId]: _, ...rest } = this.pendingKeyframes;
    this.pendingKeyframes = rest;
    return shotId;
  }

  /** Use a finished keyframe as its shot's reference image. */
  recordKeyframe(shotId: string, filename: string, thumb: string | null) {
    const index = this.shots.findIndex((sh) => sh.id === shotId);
    if (index < 0) return;
    const role: ImageRole = { kind: "shot", shot: index };
    // One keyframe per shot: a new one replaces the previous one's role.
    const others = this.references.filter(
      (r) => !(r.role.kind === "shot" && r.role.shot === index) && r.filename !== filename,
    );
    this.references = [...others, { filename, thumb, role }];
    this.invalidatePlan();
    this.saveSettings();
  }

  failKeyframe(message: string) {
    this.keyframeError = message;
  }

  private sceneRequest(): SceneRequest {
    const v = this.video;
    return {
      model: v.model,
      resolution: v.resolution,
      draft: v.draft,
      aspect: v.aspect,
      seed: v.seed,
      continuous: v.continuous,
      character_name: this.characterName,
      character_traits: v.characterTraits,
      language: v.language,
      starting_state: v.startingState,
      ending_state: v.endingState,
      ambience: v.ambience,
      minor: this.isMinor,
      voice_id: this.voiceId ?? "",
      lines: this.readyLines.map((l) => ({
        line_id: l.id,
        take_id: l.chosenTakeId!,
        text: l.text,
        delivery: l.delivery,
      })),
      shots: this.shots.map((sh) => ({
        framing: sh.framing,
        action: sh.action,
        line_id: sh.lineId,
        lead: sh.lead,
      })),
      tail: v.tail,
      images: this.references.map((r) => ({ filename: r.filename, role: r.role })),
      prompt_override: this.promptOverride,
      mouth_map: v.mouthMap,
    };
  }

  // --- Mouth map (ElevenLabs forced alignment, rule S10) ---

  /** Chosen takes of the lines the shots use. */
  get sceneTakeIds(): string[] {
    const used = new Set(this.shots.map((sh) => sh.lineId).filter((id): id is string => !!id));
    return this.readyLines.filter((l) => used.has(l.id)).map((l) => l.chosenTakeId!);
  }

  /** Measure when the mouth moves in each take not measured yet, after the
   *  itemized estimate, then rebuild the plan so the prompt uses it. */
  async measureMouth() {
    const ids = this.sceneTakeIds;
    if (this.measuring || ids.length === 0) return;
    this.mouthError = null;
    try {
      const estimate = await sceneEstimateAlignment(ids);
      if (estimate.takes > 0) {
        const ok = await this.confirm(
          "scene.cost.mouth_title",
          [
            {
              labelKey: "scene.cost.mouth_item",
              params: { takes: estimate.takes, seconds: estimate.seconds.toFixed(1), usd: estimate.usd.toFixed(3) },
            },
            { labelKey: "scene.cost.mouth_checked", params: { date: estimate.checked } },
          ],
          ["scene.cost.mouth_note"],
        );
        if (!ok) return;
        this.measuring = true;
        await sceneAlignTakes(ids);
      }
      if (this.plan) await this.planScene();
    } catch (e) {
      this.mouthError = errorText(e);
    } finally {
      this.measuring = false;
    }
  }

  // --- Finished clips: exact voice and draft completion ---

  /** Called from App.svelte when a clip lands: remember it if this page made it. */
  recordClip(promptId: string, filename: string) {
    if (!this.activeJobs.includes(promptId)) return;
    this.clips = [{ filename, promptId }, ...this.clips.filter((c) => c.filename !== filename)].slice(0, MAX_CLIPS);
    this.saveSettings();
    void this.loadClipInfo(filename);
  }

  removeClip(filename: string) {
    this.clips = this.clips.filter((c) => c.filename !== filename);
    const { [filename]: _, ...rest } = this.clipInfo;
    this.clipInfo = rest;
    this.saveSettings();
  }

  async loadClipInfo(filename: string) {
    try {
      const info = await sceneClipInfo(filename);
      this.clipInfo = { ...this.clipInfo, [filename]: info };
    } catch {
      // Deleted or renamed in the gallery: nothing left to offer.
      this.removeClip(filename);
    }
  }

  /** Complete a draft at 1080p, after the itemized estimate. */
  async upgradeDraft(filename: string) {
    if (this.clipBusy) return;
    this.clipError = null;
    await this.loadClipInfo(filename);
    const info = this.clipInfo[filename];
    if (!info?.upgrade) {
      this.clipError = locale.t("scene.clips.upgrade_unavailable");
      return;
    }
    const expires = info.draft_expires_unix ? new Date(info.draft_expires_unix * 1000).toLocaleString() : "";
    const ok = await this.confirm(
      "scene.cost.upgrade_title",
      [
        {
          labelKey: "scene.cost.video_item",
          params: { model: info.model_label, seconds: info.seconds, resolution: "1080p", usd: info.upgrade.usd.toFixed(2) },
        },
        { labelKey: "scene.cost.upgrade_expires", params: { date: expires } },
        { labelKey: "scene.cost.video_checked", params: { date: info.upgrade.checked } },
      ],
      ["scene.cost.upgrade_note", "scene.cost.fal_note", "scene.cost.cancel_note"],
    );
    if (!ok) return;
    this.clipBusy = filename;
    try {
      this.track(await sceneUpgradeDraft(filename));
      // Hide the offer while it runs; the finished 1080p clip joins the list.
      this.clipInfo = { ...this.clipInfo, [filename]: { ...info, upgrade: null } };
    } catch (e) {
      this.clipError = errorText(e);
    } finally {
      this.clipBusy = null;
    }
  }

  /** Swap a clip's sound for the user's own voice track. Free and local, so
   *  no confirmation. Returns the new gallery clip for the page to show. */
  async exactVoice(filename: string): Promise<SavedSceneClip | null> {
    if (this.clipBusy) return null;
    this.clipError = null;
    this.clipBusy = filename;
    try {
      const saved = await sceneExactVoice(filename);
      this.clips = [{ filename: saved.video_filename, promptId: "" }, ...this.clips].slice(0, MAX_CLIPS);
      this.saveSettings();
      void this.loadClipInfo(saved.video_filename);
      return saved;
    } catch (e) {
      this.clipError = errorText(e);
      return null;
    } finally {
      this.clipBusy = null;
    }
  }

  /** Timings, prompt and price. Free: nothing leaves this machine. */
  async planScene(): Promise<ScenePlan | null> {
    this.planning = true;
    this.videoError = null;
    try {
      this.plan = await scenePlan(this.sceneRequest());
      // Show exactly what will be sent, including anything the backend adds.
      if (this.promptOverride !== null) this.promptOverride = this.plan.prompt;
      return this.plan;
    } catch (e) {
      this.plan = null;
      this.videoError = errorText(e);
      return null;
    } finally {
      this.planning = false;
    }
  }

  /** Show the itemized estimate, then start the paid render on confirm. */
  async generateVideo() {
    if (this.starting || this.planning) return;
    const plan = await this.planScene();
    if (!plan) return;
    const caps = this.capability;
    const ok = await this.confirm(
      "scene.cost.video_title",
      [
        {
          labelKey: this.video.draft ? "scene.cost.video_item_draft" : "scene.cost.video_item",
          params: {
            model: plan.model_label,
            seconds: plan.seconds,
            resolution: plan.estimate.resolution,
            usd: plan.estimate.usd.toFixed(2),
          },
        },
        { labelKey: "scene.cost.video_inputs", params: { images: this.references.length, audio: plan.track_seconds.toFixed(1) } },
        { labelKey: "scene.cost.video_checked", params: { date: caps?.price_checked ?? plan.estimate.checked } },
      ],
      ["scene.cost.fal_note", "scene.cost.cancel_note"],
    );
    if (!ok) return;
    this.starting = true;
    this.videoError = null;
    try {
      const id = await sceneGenerate(this.sceneRequest());
      this.track(id);
    } catch (e) {
      this.videoError = errorText(e);
    } finally {
      this.starting = false;
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
