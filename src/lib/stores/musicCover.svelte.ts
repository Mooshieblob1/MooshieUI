import { downloadModel, getCoverCapabilities, getCoverTranscription, getMusicCapabilities, getMusicStatus, interruptGeneration, transcribeMusicCover, transcribeMusicNative } from "../utils/api.js";
import { blobBase64 } from "../utils/musicAudio.js";
import { getAuthUser } from "../utils/ipc.js";
import { locale } from "./locale.svelte.js";
import type { MusicCapabilities, MusicJob } from "../types/music.js";

class MusicCoverStore {
  legacyConfigured = $state(false);
  backend = $state<"native" | "separate">("native");
  nativeCapabilities = $state<MusicCapabilities | null>(null);
  nativeJob = $state<MusicJob | null>(null);
  encoder = $state("");
  downloading = $state(false);
  candidateMode = $state<"melody" | "full">("melody");
  get configured() { return this.backend === "separate" ? this.legacyConfigured : !!this.encoder && (this.nativeCapabilities?.audio_encoders ?? []).includes(this.encoder); }
  get nativeSupported() { return this.nativeCapabilities?.cover_missing_nodes?.length === 0; }
  device = $state("");
  refreshing = $state(false);
  busy = $state(false);
  cancelling = $state(false);
  error = $state("");
  candidate = $state("");
  warnings = $state<string[]>([]);
  filename = $state("");
  jobId = $state("");
  private owner: string | null | undefined = undefined;
  private sequence = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private failures = 0;
  private missing = 0;

  private saveNativeJob() {
    try {
      const key = `mooshieui.cover.job.${getAuthUser() ?? "host"}`;
      if (this.nativeJob) localStorage.setItem(key, JSON.stringify({ job: this.nativeJob, mode: this.candidateMode, filename: this.filename, time: Date.now() }));
      else localStorage.removeItem(key);
    } catch (error) { console.warn("Failed to retain transcription job", error); }
  }

  async downloadEncoder() {
    if (this.downloading || this.busy) return;
    this.downloading = true; this.error = "";
    try {
      await downloadModel("https://huggingface.co/Comfy-Org/YuE2/resolve/8e6fcf0f23252ed188b634bd50d44f4b01fba890/audio_encoders/sheetsage2_bf16.safetensors",
        "audio_encoders", "sheetsage2_bf16.safetensors", undefined, "5fd960ce3df281e3f3a889d174584d88f96247711480cf96377b12d7e8b6adc5");
      await this.refresh();
    } catch (error) { this.error = String(error); }
    finally { this.downloading = false; }
  }

  async refresh() {
    if (this.owner !== getAuthUser()) {
      this.sequence++;
      if (this.timer) clearTimeout(this.timer);
      this.owner = getAuthUser();
      this.busy = false; this.cancelling = false; this.jobId = "";
      this.nativeJob = null; this.nativeCapabilities = null; this.encoder = ""; this.legacyConfigured = false;
      this.candidate = ""; this.warnings = []; this.filename = ""; this.error = "";
    }
    this.refreshing = true;
    const owner = this.owner;
    try {
      const [separate, native] = await Promise.allSettled([getCoverCapabilities(), getMusicCapabilities()]);
      if (owner !== getAuthUser()) return;
      const caps = separate.status === "fulfilled" ? separate.value : { configured: false, device: "", latest_job: undefined };
      this.legacyConfigured = caps.configured;
      this.device = caps.device ?? "";
      this.nativeCapabilities = native.status === "fulfilled" ? native.value : null;
      if (!(this.nativeCapabilities?.audio_encoders ?? []).includes(this.encoder)) this.encoder = this.nativeCapabilities?.audio_encoders?.[0] ?? "";
      // An explicitly configured legacy environment remains usable when no native worker is ready.
      if (!this.nativeSupported && this.legacyConfigured && !this.busy) this.backend = "separate";
      if (!this.nativeJob && !this.busy) {
        try {
          const saved = JSON.parse(localStorage.getItem(`mooshieui.cover.job.${owner ?? "host"}`) ?? "null");
          if (saved?.job?.kind === "transcription" && typeof saved.job.prompt_id === "string" && Number.isInteger(saved.job.worker_id) && Date.now() - saved.time < 86400000) {
            this.nativeJob = saved.job; this.jobId = saved.job.prompt_id; this.backend = "native";
            this.filename = saved.filename ?? ""; this.candidateMode = saved.mode === "full" ? "full" : "melody";
          }
        } catch { /* Local recovery is optional; source drafts remain intact. */ }
      }
      if (this.nativeJob && !this.busy) {
        this.busy = true; this.failures = 0; this.missing = 0;
        void this.poll(++this.sequence);
      } else if (this.backend === "separate" && caps.latest_job && !this.busy && (this.jobId || !this.candidate)) {
        this.backend = "separate";
        this.jobId = caps.latest_job;
        this.busy = true;
        this.failures = 0;
        void this.poll(++this.sequence);
      }
    } catch (error) { if (owner === getAuthUser()) this.error = String(error); }
    finally { this.refreshing = false; }
  }

  async transcribe(file: File, mode: "melody" | "full" = "melody") {
    if (this.busy) return;
    if (!file.size || file.size > 64 * 1024 * 1024) { this.error = locale.t("music.cover_audio_limit"); return; }
    const sequence = ++this.sequence;
    const owner = getAuthUser();
    this.owner = owner;
    this.busy = true; this.error = ""; this.cancelling = false;
    this.failures = 0;
    this.filename = file.name;
    this.candidateMode = mode;
    this.missing = 0;
    const backend = this.backend;
    try {
      const encoded = await blobBase64(file);
      if (sequence !== this.sequence || owner !== getAuthUser()) return;
      const job = backend === "native" ? await transcribeMusicNative(encoded, file.name, this.encoder, mode) : null;
      const id = job?.prompt_id ?? await transcribeMusicCover(encoded, file.name);
      if (sequence !== this.sequence || owner !== getAuthUser()) return;
      this.jobId = id;
      this.nativeJob = job;
      if (backend === "separate") this.candidateMode = "melody";
      this.saveNativeJob();
      await this.poll(sequence);
    } catch (error) {
      if (sequence === this.sequence && owner === getAuthUser()) { this.error = String(error); this.busy = false; }
    }
  }

  private async poll(sequence: number) {
    if (sequence !== this.sequence || this.owner !== getAuthUser()) return;
    try {
      const result = this.nativeJob ? await getMusicStatus(this.nativeJob) : await getCoverTranscription(this.jobId);
      if (sequence !== this.sequence || this.owner !== getAuthUser()) return;
      this.failures = 0;
      this.error = "";
      if (result.status === "missing" && ++this.missing >= 5) {
        this.error = locale.t("music.missing"); this.busy = false; this.cancelling = false;
        this.nativeJob = null; this.jobId = ""; this.saveNativeJob(); return;
      }
      if (result.status !== "missing") this.missing = 0;
      if (result.status !== "running" && result.status !== "queued" && result.status !== "missing") {
        if (result.status === "completed") {
          this.candidate = result.abc ?? "";
          this.warnings = "warnings" in result ? result.warnings ?? [] : [];
          this.error = "";
        } else if (result.status === "error") this.error = result.error ?? locale.t("music.failed");
        this.busy = false; this.cancelling = false; this.jobId = "";
        this.nativeJob = null; this.saveNativeJob();
        return;
      }
    } catch (error) {
      if (sequence !== this.sequence || this.owner !== getAuthUser()) return;
      this.error = String(error);
      // Keep the job recoverable via Refresh after a prolonged disconnection.
      if (++this.failures >= 15) { this.busy = false; this.cancelling = false; return; }
    }
    // A brief lost connection must not abandon a running subprocess or overwrite a draft.
    if (sequence === this.sequence && this.owner === getAuthUser()) this.timer = setTimeout(() => void this.poll(sequence), 2000);
  }

  async cancel() {
    if (!this.jobId || this.cancelling) return;
    this.cancelling = true;
    const sequence = this.sequence;
    const owner = this.owner;
    try {
      if (this.nativeJob) {
        await interruptGeneration(this.nativeJob.prompt_id);
        if (sequence !== this.sequence || owner !== getAuthUser()) return;
        this.busy = false; this.cancelling = false; this.jobId = ""; this.nativeJob = null;
        this.saveNativeJob(); this.sequence++;
        if (this.timer) clearTimeout(this.timer);
      } else await getCoverTranscription(this.jobId, true);
    }
    catch (error) { if (sequence === this.sequence && owner === getAuthUser()) { this.error = String(error); this.cancelling = false; } }
  }
}

export const musicCover = new MusicCoverStore();
