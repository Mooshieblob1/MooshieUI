import { downloadModel, generateMusic, getMusicCapabilities, getMusicStatus, interruptGeneration, loadMusicAudio } from "../utils/api.js";
import { getAuthUser } from "../utils/ipc.js";
import { locale } from "./locale.svelte.js";
import { exportFlac, songTitle, blobBase64 } from "../utils/musicAudio.js";
import { musicLibrary } from "../utils/musicLibrary.js";
import { coverScoreError } from "../utils/musicCover.js";
import { inspectMusicScore } from "../utils/musicScore.js";
import { cloneMusicParams, defaultMusicSampling, musicId, musicSeed, readMusicSampling } from "../utils/musicSettings.js";
import type { MusicBatch, MusicPlan, MusicCapabilities, MusicExecutionEvent, MusicJob, MusicParams, MusicStage, MusicResult, MusicPlaylist } from "../types/music.js";
import type { LyricAlignment } from "../utils/lyricTiming.js";

// Native workflow node IDs from templates/music.rs. PreviewAny (9) can execute
// after saving, so it must not send the visible stage back to score writing.
const nodeStages: Record<string, MusicStage> = {
  "1": "prepare", "2": "score", "3": "compose", "5": "render", "6": "decode", "8": "save",
};
const stageOrder: MusicStage[] = ["prepare", "score", "compose", "render", "decode", "save", "retrieve"];

const defaults = (): MusicParams => ({
  title: "", checkpoint: "", style: "", lyrics: "", planning: "full", abc: "",
  max_duration: 120, steps: 32, seed: "-1", cover: false, sampling: defaultMusicSampling(),
});

const libraryAccount = () => {
  const user = getAuthUser();
  return user === null ? "host" : `user:${user}`;
};

class MusicStore {
  plans = $state<MusicPlan[]>([]);
  planCandidate = $state<MusicPlan | null>(null);
  scoreUndo = $state<{ before: MusicParams; after: string } | null>(null);
  candidateCount = $state(1);
  batches = $state<MusicBatch[]>([]);
  activeBatch = $state<MusicBatch | null>(null);
  batchRunning = $state(false);
  batchIndex = $state(0);
  compareIds = $state<string[]>([]);
  comparing = $state(false);
  editingComposition = $state<string | null>(null);
  reviewingSong = $state<string | null>(null);
  reviewSource = $state.raw<File | null>(null);
  private jobOwner: string | null = null;
  private preparing = $state(false);
  private cancelRequested = false;
  view = $state<"generate" | "library">("generate");
  editingSong = $state<string | null>(null);
  playlists = $state<MusicPlaylist[]>([]);
  libraryLoading = $state(false);
  librarySaving = $state(false);
  libraryError = $state("");
  playing = $state(false);
  playPending = $state(false);
  waiting = $state(false);
  playError = $state(false);
  currentTime = $state(0);
  duration = $state(0);
  volume = $state(0.8);
  muted = $state(false);
  looping = $state(false);
  queue = $state<string[]>([]);
  private audio: HTMLAudioElement | null = null;
  private playbackSequence = 0;
  private libraryOwner: string | null = null;
  private libraryLoad: Promise<void> | null = null;
  private writes: Promise<unknown> = Promise.resolve();
  private pendingWrites = 0;
  private libraryLoaded = false;
  params = $state<MusicParams>(defaults());
  reviewedCoverAbc = $state("");
  get coverReady() { return !this.params.cover || (!coverScoreError(this.params.abc, this.params.planning === "full") && this.reviewedCoverAbc === this.params.abc); }
  setCover(cover: boolean) {
    this.params.cover = cover;
    this.reviewedCoverAbc = "";
    if (cover) this.params.planning = "melody";
    this.saveSettings();
  }
  capabilities = $state<MusicCapabilities | null>(null);
  refreshing = $state(false);
  downloading = $state(false);
  error = $state("");
  phase = $state<"idle" | "submitting" | "queued" | "running" | "loading">("idle");
  stage = $state<MusicStage>("prepare");
  stageValue = $state(0);
  stageMax = $state(0);
  completedStages = $state<MusicStage[]>([]);
  startedAt = $state(0);
  finishedAt = $state(0);
  outcome = $state<"" | "completed" | "cancelled" | "failed">("");
  queuePosition = $state<number | null>(null);
  job = $state<MusicJob | null>(null);
  results = $state<MusicResult[]>([]);
  audioUrl = $state("");
  audioBlob = $state.raw<Blob | null>(null);
  selectedResult = $state<MusicResult | null>(null);
  loadingAudio = $state(false);
  exporting = $state(false);
  exportStatus = $state<"" | "saved" | "downloaded">("");
  exportError = $state("");
  cancelling = $state(false);
  private audioBase64 = "";
  private timer: ReturnType<typeof setTimeout> | null = null;
  private missingPolls = 0;
  private failedPolls = 0;
  private submittedParams = $state.raw<MusicParams | null>(null);
  private knownPrompts = new Set<string>();

  rememberPrompt(id: string) { this.knownPrompts.add(id); }
  isMusicPrompt(id: string) { return this.knownPrompts.has(id); }

  get busy() { return this.preparing || this.phase !== "idle" || this.batchRunning; }
  get title() { return this.selectedResult ? this.resultTitle(this.selectedResult) : locale.t("music.untitled"); }
  get queueIndex() { return this.queue.indexOf(this.selectedResult?.prompt_id ?? ""); }
  get hasNext() { return this.queueIndex >= 0 && this.queueIndex < this.queue.length - 1; }
  get hasPrevious() { return this.currentTime > 3 || this.queueIndex > 0; }
  resultTitle(result: MusicResult) { return result.title?.trim() || songTitle(result.params.lyrics) || locale.t("music.untitled"); }
  get stages() {
    if (this.submittedParams?.task === "plan") return ["prepare", "score", "retrieve"] as MusicStage[];
    const writesScore = this.submittedParams?.planning !== "off" && !this.submittedParams?.abc.trim();
    return stageOrder.filter(stage => stage !== "score" || writesScore);
  }
  get stagePercent() {
    return this.stageMax > 0 ? Math.min(100, Math.max(0, this.stageValue / this.stageMax * 100)) : null;
  }
  get ready() {
    return this.capabilities !== null && this.capabilities.missing_nodes.length === 0
      && this.capabilities.checkpoints.includes(this.params.checkpoint);
  }

  private markStageComplete(stage: MusicStage) {
    if (!this.completedStages.includes(stage)) this.completedStages = [...this.completedStages, stage];
  }

  private enterStage(stage: MusicStage) {
    if (stageOrder.indexOf(stage) < stageOrder.indexOf(this.stage)) return false;
    if (stage !== this.stage) {
      this.markStageComplete(this.stage);
      this.stage = stage;
      this.stageValue = 0;
      this.stageMax = 0;
    }
    return true;
  }

  private updateStageCounter(value: number | undefined, max: number | undefined) {
    if (typeof value !== "number" || typeof max !== "number" || !Number.isFinite(value) || !Number.isFinite(max) || value < 0 || max <= 0) return;
    if (max === this.stageMax && value < this.stageValue) return;
    this.stageValue = Math.min(value, max);
    this.stageMax = max;
  }

  /** Return true for music events so App.svelte keeps them out of image progress. */
  handleEvent(type: string, data: MusicExecutionEvent): boolean {
    const id = data.prompt_id;
    if (typeof id !== "string") return false;
    if (this.job?.prompt_id !== id || !this.busy) return this.isMusicPrompt(id);
    if (type === "queue") {
      if (typeof data.position === "number" && Number.isInteger(data.position) && data.position >= 0) this.queuePosition = data.position + 1;
      return true;
    }
    if (type === "execution_error" || type === "execution_interrupted") {
      if (type === "execution_error") this.error = data.exception_message || locale.t("music.failed");
      this.finish(type === "execution_interrupted" ? "cancelled" : "failed");
      return true;
    }
    this.phase = "running";
    this.queuePosition = null;
    if (type === "execution_success" || (type === "executing" && data.node === null)) {
      this.enterStage("retrieve");
    } else if (type === "executing" || type === "progress") {
      const stage = typeof data.node === "string" ? nodeStages[data.node] : undefined;
      if (stage && this.enterStage(stage) && !this.completedStages.includes(stage) && type === "progress") {
        this.updateStageCounter(data.value, data.max);
      }
    } else if (type === "execution_cached" && Array.isArray(data.nodes)) {
      for (const node of data.nodes) if (nodeStages[node]) this.markStageComplete(nodeStages[node]);
    } else if (type === "progress_state" && data.nodes && !Array.isArray(data.nodes)) {
      for (const [node, state] of Object.entries(data.nodes)) {
        const stage = nodeStages[node];
        if (!stage) continue;
        if (state.state === "finished") this.markStageComplete(stage);
        if (state.state === "running" && this.enterStage(stage) && !this.completedStages.includes(stage)) {
          // max=1/value=0 is ComfyUI's placeholder before any measurable work.
          if ((state.max ?? 0) > 1 || (state.value ?? 0) > 0) this.updateStageCounter(state.value, state.max);
        }
      }
    }
    return true;
  }

  loadSettings() {
    try {
      const raw = localStorage.getItem(`mooshieui.music.${getAuthUser() ?? "host"}`);
      if (!raw) return;
      const parsed = JSON.parse(raw);
      const next = defaults();
      for (const key of ["title", "checkpoint", "style", "lyrics", "abc", "seed"] as const) {
        if (typeof parsed[key] === "string") next[key] = parsed[key];
      }
      if (["full", "melody", "off"].includes(parsed.planning)) next.planning = parsed.planning;
      next.cover = parsed.cover === true;
      if (next.cover && next.planning === "off") next.planning = "melody";
      if (Number.isFinite(parsed.max_duration)) next.max_duration = parsed.max_duration;
      if (Number.isInteger(parsed.steps)) next.steps = parsed.steps;
      next.sampling = readMusicSampling(parsed.sampling);
      if (parsed.lineage && typeof parsed.lineage.project_id === "string") next.lineage = parsed.lineage;
      this.params = next;
      this.reviewedCoverAbc = "";
    } catch (error) { console.warn("Failed to load music settings", error); }
  }

  saveSettings() {
    try {
      localStorage.setItem(`mooshieui.music.${getAuthUser() ?? "host"}`, JSON.stringify(this.params));
    } catch (error) { console.warn("Failed to save music settings", error); }
  }

  async refresh() {
    if (this.refreshing) return;
    this.refreshing = true;
    this.error = "";
    try {
      this.capabilities = await getMusicCapabilities();
      if (!this.capabilities.checkpoints.includes(this.params.checkpoint)) {
        this.params.checkpoint = this.capabilities.checkpoints[0] ?? "";
      }
    } catch (error) {
      this.capabilities = null;
      this.error = String(error);
    } finally { this.refreshing = false; }
  }

  async downloadCheckpoint(tier: "bf16" | "int8_convrot") {
    if (this.downloading) return;
    this.downloading = true;
    this.error = "";
    try {
      const filename = `yue2_3b_${tier}.safetensors`;
      const sha256 = tier === "bf16"
        ? "33765adbf9813c9a50318218760b2fd819a319862460a04884607581961c6fee"
        : "96fe199377309001ed8cd26a944baeee8cc31a20ba7c36d1d3c0a7e1f4149db6";
      await downloadModel(`https://huggingface.co/Comfy-Org/YuE2/resolve/8e6fcf0f23252ed188b634bd50d44f4b01fba890/checkpoints/${filename}`, "checkpoints", filename, undefined, sha256);
      await this.refresh();
    } catch (error) { this.error = String(error); }
    finally { this.downloading = false; }
  }

  async generate() {
    if (this.busy || !this.ready) return;
    this.error = "";
    this.cancelRequested = false;
    if (this.params.abc.trim()) {
      const inspection = inspectMusicScore(this.params.abc);
      if (!inspection.score) { this.error = `${locale.t("music.score_invalid")} ${inspection.error}`; return; }
    }
    if (this.params.cover) {
      const scoreError = coverScoreError(this.params.abc, this.params.planning === "full");
      if (scoreError || !this.coverReady) {
        this.error = locale.t(`music.${scoreError ?? "cover_review_required"}`);
        return;
      }
      if (this.params.planning === "off") this.params.planning = "melody";
    }
    if (!/^-?\d+$/.test(this.params.seed.trim()) || BigInt(this.params.seed) < -1n || BigInt(this.params.seed) > 9223372036854775807n) {
      this.error = locale.t("music.invalid_seed");
      return;
    }
    const snapshot = cloneMusicParams(this.params);
    snapshot.seed = BigInt(snapshot.seed).toString();
    const owner = libraryAccount();
    this.preparing = true;
    try { if (!this.libraryLoaded) await this.loadLibrary(); }
    finally { this.preparing = false; }
    if (owner !== libraryAccount() || this.phase !== "idle" || this.cancelRequested) return;
    snapshot.task = "audio";
    snapshot.lineage ??= { project_id: musicId() };
    this.params.lineage = { ...snapshot.lineage };
    this.saveSettings();
    const count = [1, 2, 4, 8].includes(this.candidateCount) ? this.candidateCount : 1;
    if (count > 1) {
      const seeds = new Set<string>();
      if (snapshot.seed !== "-1") seeds.add(snapshot.seed);
      while (seeds.size < count) seeds.add(musicSeed());
      this.activeBatch = { id: musicId(), params: snapshot, createdAt: Date.now(),
        attempts: [...seeds].map(seed => ({ seed, status: "pending" })) };
      this.batchRunning = true; this.batchIndex = 0;
      this.batches = [this.activeBatch, ...this.batches];
      if (!await this.persistBatch()) { this.batchRunning = false; return; }
      await this.submitBatchAttempt();
    } else { this.activeBatch = null; await this.submit(snapshot); }
  }
  get isPlanJob() { return this.submittedParams?.task === "plan"; }

  async generatePlan() {
    if (this.busy || !this.ready || this.params.cover || this.params.planning === "off") return;
    this.cancelRequested = false;
    if (!this.params.style.trim() || !this.params.lyrics.trim()) { this.error = locale.t("music.plan_needs_text"); return; }
    const params = cloneMusicParams(this.params);
    const owner = libraryAccount();
    this.preparing = true;
    try { if (!this.libraryLoaded) await this.loadLibrary(); }
    finally { this.preparing = false; }
    if (owner !== libraryAccount() || this.phase !== "idle" || this.cancelRequested) return;
    params.task = "plan";
    params.lineage ??= { project_id: musicId() };
    this.error = "";
    this.activeBatch = null;
    await this.submit(params);
  }

  async resumeBatch(batch: MusicBatch) {
    if (this.busy || !this.ready) return;
    this.cancelRequested = false;
    const index = batch.attempts.findIndex(a => a.status === "pending");
    if (index < 0) return;
    this.activeBatch = JSON.parse(JSON.stringify(batch)); this.batchIndex = index; this.batchRunning = true;
    this.error = "";
    await this.submitBatchAttempt();
  }

  private persistJob() {
    if (!this.jobOwner) return;
    try {
      const key = `mooshieui.music.active.${this.jobOwner}`;
      if (this.job && this.submittedParams) localStorage.setItem(key, JSON.stringify({ job: this.job, params: this.submittedParams, startedAt: this.startedAt, batchId: this.activeBatch?.id }));
      else localStorage.removeItem(key);
    } catch { this.libraryError = locale.t("music.job_recovery_unavailable"); }
  }

  private recoverJob(owner: string) {
    if (this.job || this.phase !== "idle") return;
    try {
      const raw = localStorage.getItem(`mooshieui.music.active.${owner}`);
      if (!raw) return;
      const saved = JSON.parse(raw);
      if (!saved.job || typeof saved.job.prompt_id !== "string" || !Number.isInteger(saved.job.worker_id) || !saved.params || typeof saved.params.abc !== "string" || typeof saved.params.lyrics !== "string") return;
      this.job = saved.job; this.submittedParams = saved.params; this.startedAt = saved.startedAt ?? Date.now();
      this.jobOwner = owner; this.phase = "queued"; this.outcome = ""; this.stage = "prepare"; this.completedStages = []; this.failedPolls = 0; this.missingPolls = 0;
      this.activeBatch = this.batches.find(b => b.id === saved.batchId) ?? null;
      this.batchIndex = this.activeBatch ? this.activeBatch.attempts.findIndex(a => a.prompt_id === saved.job.prompt_id) : 0;
      if (this.batchIndex < 0) this.activeBatch = null;
      // Observe the submitted job only; remaining candidates require explicit Resume.
      this.batchRunning = false; this.rememberPrompt(this.job!.prompt_id); this.schedulePoll();
    } catch { this.libraryError = locale.t("music.job_recovery_unavailable"); }
  }

  private async persistBatch(): Promise<boolean> {
    const batch = this.activeBatch ? JSON.parse(JSON.stringify(this.activeBatch)) as MusicBatch : null;
    if (!batch) return true;
    this.batches = this.batches.map(b => b.id === batch.id ? batch : b);
    return this.writeLibrary(owner => musicLibrary.saveBatch(owner, batch));
  }

  private async submitBatchAttempt() {
    const batch = this.activeBatch;
    if (!this.batchRunning || !batch || this.batchIndex >= batch.attempts.length) return;
    const attempt = batch.attempts[this.batchIndex];
    attempt.status = "running";
    if (!await this.persistBatch()) { this.batchRunning = false; return; }
    if (!this.batchRunning) return;
    await this.submit({ ...cloneMusicParams(batch.params), seed: attempt.seed,
      lineage: { ...batch.params.lineage!, candidate_group: batch.id, candidate_index: this.batchIndex + 1 } });
  }

  private async submit(params: MusicParams) {
    this.jobOwner = libraryAccount();
    const owner = this.jobOwner;
    this.phase = "submitting";
    this.stage = "prepare";
    this.stageValue = 0;
    this.stageMax = 0;
    this.completedStages = [];
    this.startedAt = Date.now();
    this.finishedAt = 0;
    this.outcome = "";
    this.queuePosition = null;
    this.missingPolls = 0;
    this.failedPolls = 0;
    this.submittedParams = cloneMusicParams(params);
    try {
      const job = await generateMusic(this.submittedParams);
      if (owner !== libraryAccount()) return;
      this.job = job;
      if (this.activeBatch && this.batchRunning) {
        this.activeBatch.attempts[this.batchIndex].prompt_id = job.prompt_id;
        void this.persistBatch();
      }
      this.rememberPrompt(this.job.prompt_id);
      this.phase = "queued";
      this.persistJob();
      if (this.cancelRequested) { await this.cancel(); return; }
      this.schedulePoll();
    } catch (error) {
      if (owner !== libraryAccount()) return;
      this.error = String(error);
      this.finish("failed");
    }
  }

  private schedulePoll() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => { this.timer = null; void this.poll(); }, 2000);
  }

  private async poll() {
    const job = this.job;
    if (!job) return;
    try {
      const status = await getMusicStatus(job);
      if (this.job?.prompt_id !== job.prompt_id || this.jobOwner !== libraryAccount()) return;
      if (this.failedPolls > 0) this.error = "";
      this.failedPolls = 0;
      if (status.status === "completed" && (status.kind === "plan" || this.submittedParams?.task === "plan")) {
        if (!status.abc?.trim()) { this.error = locale.t("music.failed"); this.finish("failed"); return; }
        const plan: MusicPlan = { ...job, abc: status.abc, params: cloneMusicParams(this.submittedParams!), metadata: status.metadata, createdAt: Date.now() };
        this.plans = [plan, ...this.plans.filter(p => p.prompt_id !== plan.prompt_id)];
        this.planCandidate = plan;
        await this.writeLibrary(owner => musicLibrary.savePlan(owner, plan));
        if (this.jobOwner !== libraryAccount()) return;
        this.finish("completed");
        return;
      }
      if (status.status === "completed" && status.filename) {
        const result: MusicResult = { ...job, filename: status.filename, abc: status.abc ?? "", params: cloneMusicParams(this.submittedParams!), metadata: status.metadata, title: this.submittedParams?.title?.trim().slice(0, 200), createdAt: Date.now() };
        this.results = [result, ...this.results.filter(r => r.prompt_id !== result.prompt_id)];
        this.enterStage("retrieve");
        this.phase = "loading";
        this.job = null;
        // A completed generation must not interrupt a song already playing.
        if (this.playing || this.loadingAudio) {
          try { await this.archive(result); }
          catch (error) { this.libraryError = `${locale.t("music.library_error")} ${String(error)}`; }
        }
        else await this.play(result, false);
        if (this.jobOwner === libraryAccount()) this.finish("completed");
        return;
      }
      if (status.status === "error") {
        this.error = status.error ?? locale.t("music.failed");
        this.finish("failed");
        return;
      }
      if (status.status === "missing") {
        if (++this.missingPolls >= 5) {
          this.error = locale.t("music.missing");
          this.finish("failed");
          return;
        }
      } else {
        this.missingPolls = 0;
        // An in-flight queue response can arrive after a newer execution event.
        if (status.status === "running" || this.phase !== "running") {
          this.phase = status.status === "running" ? "running" : "queued";
        }
      }
    } catch (error) {
      // Keep the job recoverable across transient server/network failures.
      if (this.job?.prompt_id !== job.prompt_id) return;
      if (++this.failedPolls >= 3) this.error = `${locale.t("music.reconnecting")} ${String(error)}`;
    }
    if (this.job) this.schedulePoll();
  }

  private finish(outcome: "completed" | "cancelled" | "failed") {
    this.phase = "idle";
    this.outcome = outcome;
    this.finishedAt = Date.now();
    this.queuePosition = null;
    if (outcome === "completed") this.completedStages = [...this.stages];
    this.job = null;
    this.persistJob();
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    if (this.activeBatch && this.activeBatch.attempts[this.batchIndex]) {
      this.activeBatch.attempts[this.batchIndex].status = outcome;
      if (outcome === "failed") this.activeBatch.attempts[this.batchIndex].error = this.error;
      const batchId = this.activeBatch.id;
      const owner = this.jobOwner;
      void this.persistBatch().then(saved => {
        if (!this.batchRunning || this.activeBatch?.id !== batchId || owner !== libraryAccount()) return;
        if (!saved || outcome === "cancelled") { this.batchRunning = false; return; }
        this.batchIndex = this.activeBatch.attempts.findIndex((attempt, index) => index > this.batchIndex && attempt.status === "pending");
        if (this.batchIndex < 0) { this.batchRunning = false; return; }
        this.error = "";
        void this.submitBatchAttempt();
      });
    }
  }

  async cancel() {
    if (this.cancelling) return;
    this.cancelRequested = true;
    if (this.batchRunning && this.activeBatch) {
      this.batchRunning = false;
      for (const attempt of this.activeBatch.attempts) if (attempt.status === "pending" || attempt.status === "running") attempt.status = "cancelled";
      void this.persistBatch();
    }
    if (!this.job) return;
    const job = this.job;
    this.cancelling = true;
    try {
      await interruptGeneration(job.prompt_id);
      if (this.job?.prompt_id === job.prompt_id) this.finish("cancelled");
    } catch (error) { this.error = String(error); }
    finally { this.cancelling = false; }
  }

  async loadLibrary() {
    const owner = libraryAccount();
    if (this.libraryOwner === owner && (this.libraryLoaded || this.libraryLoad)) return this.libraryLoad;
    if (this.libraryOwner !== null && this.libraryOwner !== owner) {
      this.stopPlayback();
      this.results = [];
      this.playlists = [];
      this.plans = []; this.planCandidate = null; this.scoreUndo = null; this.batches = [];
      this.batchRunning = false; this.activeBatch = null; this.compareIds = []; this.comparing = false; this.editingComposition = null;
      this.job = null; this.phase = "idle"; this.jobOwner = null;
      if (this.timer) clearTimeout(this.timer);
      this.timer = null;
      this.params = defaults(); this.loadSettings();
      this.editingSong = null;
      this.reviewingSong = null; this.reviewSource = null;
    }
    this.libraryOwner = owner;
    this.libraryLoading = true;
    this.libraryLoaded = false;
    this.libraryError = "";
    const pending = (async () => {
      try {
        const saved = await musicLibrary.load(owner);
        if (this.libraryOwner !== owner) return;
        const existing = new Set(this.results.map(song => song.prompt_id));
        this.results = [...this.results, ...saved.songs.filter(song => !existing.has(song.prompt_id))]
          .sort((a, b) => (b.createdAt ?? 0) - (a.createdAt ?? 0));
        this.playlists = saved.playlists;
        this.plans = saved.plans ?? [];
        this.batches = saved.batches ?? [];
        this.libraryLoaded = true;
        this.recoverJob(owner);
      } catch (error) {
        if (this.libraryOwner === owner) this.libraryError = `${locale.t("music.library_error")} ${String(error)}`;
      } finally {
        if (this.libraryOwner === owner) { this.libraryLoading = false; this.libraryLoad = null; }
      }
    })();
    this.libraryLoad = pending;
    return pending;
  }

  private writeLibrary(operation: (owner: string) => Promise<void>): Promise<boolean> {
    const owner = this.libraryOwner ?? libraryAccount();
    this.pendingWrites++;
    this.librarySaving = true;
    const pending = this.writes.then(async () => {
      if (this.libraryOwner !== owner) return false;
      try {
        await operation(owner);
        if (this.libraryOwner === owner) this.libraryError = "";
        return true;
      } catch (error) {
        if (this.libraryOwner === owner) this.libraryError = `${locale.t("music.library_error")} ${String(error)}`;
        return false;
      }
    }).finally(() => { this.librarySaving = --this.pendingWrites > 0; });
    this.writes = pending;
    return pending;
  }

  private replaceSong(song: MusicResult) {
    this.results = this.results.map(item => item.prompt_id === song.prompt_id ? song : item);
    if (this.selectedResult?.prompt_id === song.prompt_id) this.selectedResult = song;
  }

  async updateSong(id: string, patch: Pick<Partial<MusicResult>, "title" | "alignment" | "duration" | "review">): Promise<boolean> {
    return this.writeLibrary(async owner => {
      const current = this.results.find(song => song.prompt_id === id);
      if (!current) throw new Error("Song is no longer available");
      const next = { ...current, ...patch };
      if (patch.title !== undefined) next.title = patch.title.trim().slice(0, 200);
      // Do not claim a durable edit if this recording has not been archived.
      const blob = current.saved ? undefined : this.selectedResult?.prompt_id === id ? this.audioBlob ?? undefined : undefined;
      if (!current.saved && !blob) throw new Error("Save the recording first");
      next.saved = true;
      await musicLibrary.saveSong(owner, next, blob);
      if (this.libraryOwner === owner) this.replaceSong(next);
    });
  }

  async saveAlignment(alignment: LyricAlignment): Promise<boolean> {
    return this.selectedResult ? this.updateSong(this.selectedResult.prompt_id, { alignment }) : false;
  }

  async savePlaylist(name: string, id?: string): Promise<boolean> {
    const trimmed = name.trim().slice(0, 100);
    if (!trimmed) return false;
    return this.writeLibrary(async owner => {
      const current = this.playlists.find(list => list.id === id);
      const playlist: MusicPlaylist = current ? { ...current, name: trimmed }
        : { id: crypto.randomUUID?.() ?? Array.from(crypto.getRandomValues(new Uint8Array(16)), byte => byte.toString(16).padStart(2, "0")).join(""), name: trimmed, songIds: [], createdAt: Date.now() };
      await musicLibrary.savePlaylist(owner, playlist);
      if (this.libraryOwner === owner) this.playlists = current
        ? this.playlists.map(list => list.id === id ? playlist : list) : [...this.playlists, playlist];
    });
  }

  async setPlaylistSong(playlistId: string, songId: string, include: boolean): Promise<boolean> {
    return this.writeLibrary(async owner => {
      const current = this.playlists.find(list => list.id === playlistId);
      if (!current || !this.results.some(song => song.prompt_id === songId)) throw new Error("Song or playlist is no longer available");
      const next = { ...current, songIds: include ? [...new Set([...current.songIds, songId])] : current.songIds.filter(id => id !== songId) };
      await musicLibrary.savePlaylist(owner, next);
      if (this.libraryOwner === owner) this.playlists = this.playlists.map(list => list.id === playlistId ? next : list);
    });
  }

  async deletePlaylist(id: string): Promise<boolean> {
    return this.writeLibrary(async owner => {
      await musicLibrary.deletePlaylist(owner, id);
      if (this.libraryOwner === owner) this.playlists = this.playlists.filter(list => list.id !== id);
    });
  }

  async archive(result: MusicResult, suppliedBlob?: Blob): Promise<Blob> {
    const requestedOwner = libraryAccount();
    await this.loadLibrary();
    if (requestedOwner !== libraryAccount()) throw new Error("Music library account changed");
    const owner = this.libraryOwner!;
    let blob = suppliedBlob;
    let needsSave = !result.saved;
    if (!blob && result.saved) blob = await musicLibrary.audio(owner, result.prompt_id);
    if (!blob) {
      needsSave = true;
      const base64 = await loadMusicAudio(result);
      blob = new Blob([Uint8Array.from(atob(base64), char => char.charCodeAt(0))], { type: "audio/flac" });
    }
    if (needsSave) {
      if (this.libraryOwner !== owner) throw new Error("Music library account changed");
      const recording = blob;
      const saved = await this.writeLibrary(async writeOwner => {
        const current = this.results.find(song => song.prompt_id === result.prompt_id) ?? result;
        const next = { ...current, saved: true };
        await musicLibrary.saveSong(writeOwner, next, recording);
        if (this.libraryOwner === writeOwner) this.replaceSong(next);
      });
      if (!saved) throw new Error(this.libraryError || "Music recording could not be saved");
    }
    if (owner !== libraryAccount()) throw new Error("Music library account changed");
    return blob;
  }

  async retryArchive() {
    const result = this.selectedResult;
    if (!result || this.librarySaving) return;
    try { await this.archive(result, this.audioBlob ?? undefined); }
    catch (error) { this.libraryError = `${locale.t("music.library_error")} ${String(error)}`; }
  }

  async play(result: MusicResult, autoplay = true, songs?: MusicResult[]) {
    if (songs) this.queue = songs.map(song => song.prompt_id);
    else if (!this.queue.includes(result.prompt_id)) this.queue = this.results.map(song => song.prompt_id);
    if (this.selectedResult?.prompt_id === result.prompt_id && this.audio && !this.loadingAudio) {
      if (autoplay && !this.playing) await this.togglePlayback();
      return;
    }
    const sequence = ++this.playbackSequence;
    this.audio?.pause();
    this.loadingAudio = true;
    this.playError = false;
    this.error = "";
    try {
      const blob = await this.archive(result);
      if (sequence !== this.playbackSequence) return;
      this.releaseAudio();
      this.audioBlob = blob;
      this.audioBase64 = "";
      this.audioUrl = URL.createObjectURL(blob);
      this.selectedResult = this.results.find(song => song.prompt_id === result.prompt_id) ?? result;
      this.currentTime = 0;
      this.duration = 0;
      this.exportStatus = "";
      this.exportError = "";
      const audio = new Audio(this.audioUrl);
      this.audio = audio;
      audio.preload = "metadata";
      audio.volume = this.volume;
      audio.muted = this.muted;
      audio.loop = this.looping;
      const ownsAudio = () => this.audio === audio;
      audio.ontimeupdate = () => { if (ownsAudio()) this.currentTime = audio.currentTime; };
      audio.onloadedmetadata = () => {
        if (!ownsAudio()) return;
        this.duration = Number.isFinite(audio.duration) ? audio.duration : 0;
        if (this.duration > 0) void this.updateSong(result.prompt_id, { duration: this.duration });
      };
      audio.onplay = () => { if (ownsAudio()) this.playing = true; };
      audio.onpause = () => { if (ownsAudio()) { this.playing = false; this.waiting = false; } };
      audio.onwaiting = () => { if (ownsAudio()) this.waiting = true; };
      audio.onplaying = audio.oncanplay = () => { if (ownsAudio()) this.waiting = false; };
      audio.onerror = () => { if (ownsAudio()) { this.playError = true; this.playing = false; this.waiting = false; } };
      audio.onended = () => { if (ownsAudio()) { this.playing = false; this.waiting = false; void this.next(); } };
      if (autoplay) await this.togglePlayback();
    } catch (error) {
      if (sequence === this.playbackSequence) { this.error = String(error); this.playError = true; }
    } finally { if (sequence === this.playbackSequence) this.loadingAudio = false; }
  }

  async togglePlayback() {
    const audio = this.audio;
    if (!audio || this.playPending) return;
    if (!audio.paused) { audio.pause(); return; }
    this.playError = false;
    this.playPending = true;
    try { await audio.play(); }
    catch { if (this.audio === audio) this.playError = true; }
    finally { if (this.audio === audio) this.playPending = false; }
  }

  seek(seconds: number) {
    if (!this.audio || !Number.isFinite(seconds) || this.duration <= 0) return;
    this.audio.currentTime = Math.max(0, Math.min(this.duration, seconds));
    this.currentTime = this.audio.currentTime;
  }

  setVolume(volume: number) {
    if (!Number.isFinite(volume)) return;
    this.volume = Math.max(0, Math.min(1, volume));
    this.muted = false;
    if (this.audio) { this.audio.volume = this.volume; this.audio.muted = false; }
  }

  toggleMute() {
    if (this.volume === 0) { this.setVolume(0.8); return; }
    this.muted = !this.muted;
    if (this.audio) this.audio.muted = this.muted;
  }

  toggleLoop() { this.looping = !this.looping; if (this.audio) this.audio.loop = this.looping; }

  async next() {
    if (!this.hasNext) return;
    const song = this.results.find(song => song.prompt_id === this.queue[this.queueIndex + 1]);
    if (song) await this.play(song);
  }

  async previous() {
    if (this.currentTime > 3) { this.seek(0); return; }
    const song = this.results.find(song => song.prompt_id === this.queue[this.queueIndex - 1]);
    if (song) await this.play(song);
  }

  async playAll(songs: MusicResult[], shuffle = false) {
    const ordered = [...songs];
    if (shuffle) for (let i = ordered.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [ordered[i], ordered[j]] = [ordered[j], ordered[i]];
    }
    if (ordered.length) await this.play(ordered[0], true, ordered);
  }

  private releaseAudio() {
    const audio = this.audio;
    this.audio = null;
    if (audio) { audio.pause(); audio.removeAttribute("src"); audio.load(); }
    if (this.audioUrl) URL.revokeObjectURL(this.audioUrl);
    this.audioUrl = "";
    this.playing = false;
    this.waiting = false;
    this.playPending = false;
  }

  stopPlayback() {
    this.playbackSequence++;
    this.releaseAudio();
    this.loadingAudio = false;
    this.selectedResult = null;
    this.audioBlob = null;
    this.audioBase64 = "";
    this.currentTime = this.duration = 0;
    this.queue = [];
  }

  async retryPlayback() {
    const song = this.selectedResult;
    this.stopPlayback();
    if (song) await this.play(song);
  }

  async downloadAudio() {
    if (this.exporting || !this.selectedResult || !this.audioBlob) return;
    const result = this.selectedResult;
    this.exporting = true;
    this.exportStatus = "";
    this.exportError = "";
    this.error = "";
    try {
      const filename = `${this.resultTitle(result).replace(/[<>:"/\\|?*\x00-\x1f]/g, "_").replace(/[. ]+$/, "").slice(0, 120) || "song"}.flac`;
      const base64 = this.audioBase64 || await blobBase64(this.audioBlob);
      const status = await exportFlac(base64, this.audioBlob, filename);
      if (this.selectedResult?.prompt_id === result.prompt_id && status !== "cancelled") {
        this.exportStatus = status;
      }
    } catch (error) {
      if (this.selectedResult?.prompt_id === result.prompt_id) this.exportError = String(error);
      else this.error = String(error);
    }
    finally { this.exporting = false; }
  }

  reuseScore() {
    if (!this.selectedResult || this.busy) return;
    this.useVersion(this.selectedResult);
  }

  useVersion(result: MusicResult) {
    if (this.busy) return;
    this.params = { ...cloneMusicParams(result.params), task: "audio", seed: result.seed, abc: result.abc,
      sampling: readMusicSampling(result.params.sampling),
      lineage: { project_id: result.params.lineage?.project_id ?? result.prompt_id, parent_id: result.prompt_id } };
    this.reviewedCoverAbc = "";
    this.view = "generate";
    this.saveSettings();
  }

  applyPlan(plan: MusicPlan) {
    if (this.busy) return;
    const before = cloneMusicParams(this.params);
    this.params = { ...cloneMusicParams(plan.params), task: "audio", seed: plan.seed, abc: plan.abc, sampling: readMusicSampling(plan.params.sampling) };
    this.scoreUndo = { before, after: JSON.stringify(this.params) };
    this.planCandidate = null;
    this.saveSettings();
  }

  replaceScore(abc: string) {
    if (this.busy) return;
    const before = cloneMusicParams(this.params);
    this.params.abc = abc; this.reviewedCoverAbc = "";
    this.scoreUndo = { before, after: JSON.stringify(this.params) };
    this.saveSettings();
  }

  undoScore() {
    if (this.busy || !this.scoreUndo || JSON.stringify(this.params) !== this.scoreUndo.after) return;
    this.params = cloneMusicParams(this.scoreUndo.before); this.scoreUndo = null; this.reviewedCoverAbc = "";
    this.saveSettings();
  }

  toggleCompare(id: string) {
    this.compareIds = this.compareIds.includes(id) ? this.compareIds.filter(item => item !== id) : [...this.compareIds.slice(-1), id];
  }
}

export const music = new MusicStore();
