import {
  downloadModel, generateMesh, generateSplat, getMeshCapabilities, getMeshStatus, getSplatCapabilities, getSplatStatus,
  interruptGeneration, loadMeshGlb, loadSplatPly, saveMeshFile, saveSplatFile,
} from "../utils/api.js";
import { getAuthUser, isTauri } from "../utils/ipc.js";
import { locale } from "./locale.svelte.js";
import { splatLibrary } from "../utils/splatLibrary.js";
import { parseSplatPly, plyVertexCount, toSplatBytes, type SplatData } from "../utils/splatPly.js";
import { disposeMesh, glbFaceCount, parseGlb, type MeshData } from "../utils/glb.js";
import { loadOutputImageForGenerationInput } from "../utils/galleryActions.js";
import type { OutputImage } from "../types/index.js";
import type {
  MeshCapabilities, MeshDetail, MeshParams, Model3DEngine, SplatCapabilities, SplatJob, SplatParams, SplatResult,
} from "../types/model3d.js";

/**
 * Image to 3D on ComfyUI's native nodes: a textured mesh with Pixal3D
 * (TRELLIS.2 + MoGe), or a Gaussian splat with Idol / TripoSplat. Jobs are tracked here, not in the image progress store, the same way
 * the Music page tracks its songs.
 */

const COMFY_IDOL = "https://huggingface.co/Comfy-Org/Idol/resolve/edb29d75b656a4484306d564aace9a0627f5bab4";
const COMFY_BIREFNET = "https://huggingface.co/Comfy-Org/BiRefNet/resolve/884af709e9b795d89dafbf97f29c8d6a2386f5c6";

const COMFY_PIXAL3D = "https://huggingface.co/Comfy-Org/Pixal3D/resolve/f37641be376725d324c56626007cc477f1249a69";
const COMFY_MOGE = "https://huggingface.co/Comfy-Org/MoGe/resolve/1484985258ba7ef45c314144fcf0e90924ffbf44";

export type Model3DPart = "model" | "encoder" | "decoder" | "vae" | "background" | "shape_vae" | "texture_vae" | "geometry";

export interface Model3DFile {
  part: Model3DPart;
  filename: string;
  category: string;
  url: string;
  sizeBytes: number;
  sha256: string;
}

/** Comfy-Org's repackaged Idol weights (ungated, Apache 2.0) plus BiRefNet. */
export const IDOL_FILES: Model3DFile[] = [
  { part: "model", filename: "idol_fp16.safetensors", category: "diffusion_models", url: `${COMFY_IDOL}/diffusion_models/idol_fp16.safetensors`, sizeBytes: 741_106_994, sha256: "9abf6a99fc6bdad701901b12de8fe1b256f02cae4948f7489d25d86b11b52a93" },
  { part: "encoder", filename: "dino_v3_vit_h_bf16.safetensors", category: "clip_vision", url: `${COMFY_IDOL}/clip_vision/dino_v3_vit_h_bf16.safetensors`, sizeBytes: 1_681_247_696, sha256: "a29ef35101a16966972a0d50732a6f3a608ff7cfffb2afa9bbe9007cb842cc53" },
  { part: "decoder", filename: "idol_decoder_fp16.safetensors", category: "vae", url: `${COMFY_IDOL}/vae/idol_decoder_fp16.safetensors`, sizeBytes: 576_148_442, sha256: "6a146a1e6b2653f74422f7ea835eb8715c547c7de8fc4545051f15c247ae54a9" },
  { part: "vae", filename: "flux2-vae.safetensors", category: "vae", url: `${COMFY_IDOL}/vae/flux2-vae.safetensors`, sizeBytes: 336_211_292, sha256: "868fe7b343cc8f3a19dbcfcafbc3d5f888802be3f89bd81b65b3621a066ce8f3" },
  { part: "background", filename: "birefnet.safetensors", category: "background_removal", url: `${COMFY_BIREFNET}/background_removal/birefnet.safetensors`, sizeBytes: 444_473_596, sha256: "9ab37426bf4de0567af6b5d21b16151357149139362e6e8992021b8ce356a154" },
];

/** Comfy-Org's repackaged Pixal3D (Tencent ARC, MIT), TRELLIS.2 VAEs and MoGe 2, plus BiRefNet. */
export const PIXAL_FILES: Model3DFile[] = [
  { part: "model", filename: "pixal3d_int8_convrot.safetensors", category: "diffusion_models", url: `${COMFY_PIXAL3D}/diffusion_models/pixal3d_int8_convrot.safetensors`, sizeBytes: 5_584_555_824, sha256: "4621eac3b715484f79303c7152af641fe0b2b14f4d0e3d394fd6922d00f955ec" },
  { part: "encoder", filename: "dino_v3_L_naf_fp32.safetensors", category: "clip_vision", url: `${COMFY_PIXAL3D}/clip_vision/dino_v3_L_naf_fp32.safetensors`, sizeBytes: 1_215_214_176, sha256: "4ad2ec4e0879a5b5b04cd97325cc37da954a7b6edca5170b86510f17f2b2290f" },
  { part: "shape_vae", filename: "trellis_2_shape_vae_bf16.safetensors", category: "vae", url: `${COMFY_PIXAL3D}/vae/trellis_2_shape_vae_bf16.safetensors`, sizeBytes: 1_095_844_024, sha256: "de0cb4949a76c59ee5c091a995a69bcc8c51d5aeda939f0c641a50d2a72341f4" },
  { part: "texture_vae", filename: "trellis_2_texture_vae_bf16.safetensors", category: "vae", url: `${COMFY_PIXAL3D}/vae/trellis_2_texture_vae_bf16.safetensors`, sizeBytes: 948_461_364, sha256: "714e5ebf094a610e12a8e3b5175c18a62f37f6ea4218acb6073644456b73ab0e" },
  { part: "geometry", filename: "moge_2_vitl_normal_fp16.safetensors", category: "geometry_estimation", url: `${COMFY_MOGE}/geometry_estimation/moge_2_vitl_normal_fp16.safetensors`, sizeBytes: 661_859_924, sha256: "cb1a692d03235671e959e81360d7b4d9f44aefadb1f852d6ca6aa17799d5e31f" },
  ...IDOL_FILES.filter((file) => file.part === "background"),
];

export const ENGINES: Model3DEngine[] = ["pixal3d", "idol"];
export const MESH_DETAILS: { value: MeshDetail; faces: number; texture: number }[] = [
  { value: "standard", faces: 300_000, texture: 2048 },
  { value: "high", faces: 700_000, texture: 4096 },
];

export const GAUSSIAN_CHOICES = [131_072, 262_144, 524_288, 1_048_576] as const;

export type Model3DStage = "prepare" | "structure" | "shape" | "detail" | "decode" | "texture" | "bake" | "save" | "retrieve";

// Node ids from templates/splat.rs.
const splatStages: Record<string, Model3DStage> = {
  "1": "prepare", "2": "prepare", "3": "prepare", "4": "prepare", "5": "prepare",
  "6": "prepare", "7": "prepare", "8": "prepare", "9": "prepare", "10": "prepare",
  "11": "shape", "12": "decode", "13": "save", "14": "save",
};
// Node ids from templates/mesh3d.rs.
const meshStages: Record<string, Model3DStage> = {
  ...Object.fromEntries(Array.from({ length: 15 }, (_, i) => [String(i + 1), "prepare" as Model3DStage])),
  "16": "structure", "17": "structure", "18": "structure", "19": "structure",
  "20": "shape", "21": "shape", "22": "shape",
  "23": "detail", "24": "detail", "25": "detail",
  "26": "texture", "27": "texture", "28": "texture", "29": "texture",
  ...Object.fromEntries(Array.from({ length: 9 }, (_, i) => [String(i + 30), "bake" as Model3DStage])),
  "39": "save",
};
// Both flows move forward through this order (splat: prepare, shape, decode, save).
const stageOrder: Model3DStage[] = ["prepare", "structure", "shape", "detail", "decode", "texture", "bake", "save", "retrieve"];
// Where each stage starts and ends on the progress bar (percent).
const splatSpans: Partial<Record<Model3DStage, [number, number]>> = {
  prepare: [0, 8], shape: [8, 90], decode: [90, 96], save: [96, 98], retrieve: [98, 100],
};
const meshSpans: Partial<Record<Model3DStage, [number, number]>> = {
  prepare: [0, 6], structure: [6, 16], shape: [16, 40], detail: [40, 55], texture: [55, 70],
  bake: [70, 95], save: [95, 97], retrieve: [97, 100],
};

// Idol's own sampler settings (idol-sdk): 50 Euler steps, guidance 3.0 and time shift 3.0. Idol writes
// guidance as (1 + w) * cond - w * uncond, so its 3.0 is 4.0 on ComfyUI's scale. ComfyUI's stock Idol
// workflow copied TripoSplat's 20 steps and 3.0, which is weaker than what the published examples used.
const defaults = (): SplatParams => ({
  model: "", remove_background: true, steps: 50, cfg: 4, num_gaussians: 262_144, seed: "-1",
});
// Pixal3D keeps ComfyUI's template sampling (it matches Tencent ARC's own pipeline), so only these are exposed.
const meshDefaults = (): MeshParams => ({ model: "", remove_background: true, detail: "high", seed: "-1" });
/** Saved settings older than this kept ComfyUI's stock 20 steps / guidance 3, so those two are reset. */
const SETTINGS_VERSION = 2;
const EXPERIMENTAL_KEY = "mooshieui.experimental.model3d";

const libraryAccount = () => {
  const user = getAuthUser();
  return user === null ? "host" : `user:${user}`;
};

interface Model3DEvent {
  prompt_id?: string;
  node?: string | null;
  value?: number;
  max?: number;
  position?: number;
  exception_message?: string;
}

function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).split(",")[1] ?? "");
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}

function base64ToBlob(base64: string, type: string): Blob {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return new Blob([bytes], { type });
}

/** Small JPEG of the source image for the library grid. */
function thumbnailOf(url: string): Promise<string> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      const size = 192;
      const scale = Math.min(1, size / Math.max(img.naturalWidth, img.naturalHeight));
      const canvas = document.createElement("canvas");
      canvas.width = Math.max(1, Math.round(img.naturalWidth * scale));
      canvas.height = Math.max(1, Math.round(img.naturalHeight * scale));
      const ctx = canvas.getContext("2d");
      if (!ctx) { resolve(""); return; }
      ctx.fillStyle = "#171717";
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
      resolve(canvas.toDataURL("image/jpeg", 0.8));
    };
    img.onerror = () => resolve("");
    img.src = url;
  });
}

function downloadBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  try { anchor.click(); } finally {
    anchor.remove();
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
  }
}

const isMesh = (value: SplatData | MeshData | null): value is MeshData => !!value && "primitives" in value;

function safeName(title: string): string {
  return (title.trim() || "model").replace(/[\\/:*?"<>|\x00-\x1f]+/g, "_").slice(0, 80);
}

class Model3DStore {
  /** The 3D page is experimental and hidden until turned on in Settings, Appearance. */
  enabled = $state(false);
  capabilities = $state<SplatCapabilities | null>(null);
  meshCapabilities = $state<MeshCapabilities | null>(null);
  engine = $state<Model3DEngine>("pixal3d");
  meshParams = $state<MeshParams>(meshDefaults());
  refreshing = $state(false);
  downloading = $state(false);
  downloadingFile = $state("");
  error = $state("");
  params = $state<SplatParams>(defaults());

  inputUrl = $state("");
  inputName = $state("");
  private inputBlob: Blob | null = null;
  loadingInput = $state(false);

  phase = $state<"idle" | "submitting" | "queued" | "running" | "retrieving">("idle");
  stage = $state<Model3DStage>("prepare");
  stageValue = $state(0);
  stageMax = $state(0);
  queuePosition = $state<number | null>(null);
  startedAt = $state(0);
  cancelling = $state(false);
  private job: SplatJob | null = null;
  private jobEngine: Model3DEngine = "pixal3d";
  private submitted: { params: SplatParams | MeshParams; inputUrl: string; title: string } | null = null;
  private knownPrompts = new Set<string>();
  private timer: ReturnType<typeof setTimeout> | null = null;
  private missingPolls = 0;
  private failedPolls = 0;

  results = $state<SplatResult[]>([]);
  selectedId = $state<string | null>(null);
  /** Parsed splat or mesh on screen. Raw: typed arrays must not be proxied. */
  current = $state.raw<SplatData | MeshData | null>(null);
  loadingResult = $state(false);
  exportStatus = $state("");
  private libraryOwner: string | null = null;
  private libraryLoad: Promise<void> | null = null;

  get busy() { return this.phase !== "idle"; }
  get selected() { return this.results.find((r) => r.id === this.selectedId) ?? null; }
  /** Capabilities for the chosen engine (null until loaded). */
  get engineCapabilities(): SplatCapabilities | MeshCapabilities | null {
    return this.engine === "pixal3d" ? this.meshCapabilities : this.capabilities;
  }
  get nodesReady() { const caps = this.engineCapabilities; return !!caps && caps.missing_nodes.length === 0; }
  get models(): string[] { return this.engineCapabilities?.models ?? []; }

  /** Model files the reachable workers are missing for the chosen engine. */
  get missingFiles(): Model3DFile[] {
    if (this.engine === "pixal3d") {
      const caps = this.meshCapabilities;
      if (!caps) return [];
      return PIXAL_FILES.filter((file) => {
        switch (file.part) {
          case "model": return caps.models.length === 0;
          case "encoder": return caps.clip_visions.length === 0;
          case "shape_vae": return caps.shape_vaes.length === 0;
          case "texture_vae": return caps.texture_vaes.length === 0;
          case "geometry": return caps.moge_models.length === 0;
          case "background": return caps.bg_models.length === 0;
          default: return false;
        }
      });
    }
    const caps = this.capabilities;
    if (!caps) return [];
    return IDOL_FILES.filter((file) => {
      switch (file.part) {
        case "model": return caps.models.length === 0;
        case "encoder": return caps.clip_visions.length === 0;
        case "decoder": return !caps.decoders.some((d) => d.toLowerCase().includes("idol")) && caps.decoders.length === 0;
        case "vae": return caps.flux2_vaes.length === 0;
        case "background": return caps.bg_models.length === 0;
        default: return false;
      }
    });
  }

  get ready() {
    if (this.engine === "pixal3d") {
      const caps = this.meshCapabilities;
      return !!caps && this.nodesReady && caps.models.includes(this.meshParams.model)
        && caps.clip_visions.length > 0 && caps.shape_vaes.length > 0 && caps.texture_vaes.length > 0
        && caps.moge_models.length > 0 && (!this.meshParams.remove_background || caps.bg_models.length > 0);
    }
    const caps = this.capabilities;
    return !!caps && this.nodesReady && caps.models.includes(this.params.model)
      && caps.clip_visions.length > 0 && caps.decoders.length > 0 && caps.flux2_vaes.length > 0
      && (!this.params.remove_background || caps.bg_models.length > 0);
  }

  rememberPrompt(id: string) { this.knownPrompts.add(id); }
  isModel3DPrompt(id: string) { return this.knownPrompts.has(id); }

  loadSettings() {
    try {
      const raw = localStorage.getItem(`mooshieui.model3d.${getAuthUser() ?? "host"}`);
      if (!raw) return;
      const saved = JSON.parse(raw);
      const next = defaults();
      if (typeof saved.model === "string") next.model = saved.model;
      if (typeof saved.remove_background === "boolean") next.remove_background = saved.remove_background;
      if (saved.v === SETTINGS_VERSION) {
        if (Number.isInteger(saved.steps) && saved.steps >= 4 && saved.steps <= 100) next.steps = saved.steps;
        if (typeof saved.cfg === "number" && saved.cfg >= 1 && saved.cfg <= 10) next.cfg = saved.cfg;
      }
      if (GAUSSIAN_CHOICES.includes(saved.num_gaussians)) next.num_gaussians = saved.num_gaussians;
      if (typeof saved.seed === "string" && /^-?\d{1,20}$/.test(saved.seed)) next.seed = saved.seed;
      this.params = next;
      if (ENGINES.includes(saved.engine)) this.engine = saved.engine;
      const mesh = meshDefaults();
      const savedMesh = saved.mesh ?? {};
      if (typeof savedMesh.model === "string") mesh.model = savedMesh.model;
      if (typeof savedMesh.remove_background === "boolean") mesh.remove_background = savedMesh.remove_background;
      if (MESH_DETAILS.some((d) => d.value === savedMesh.detail)) mesh.detail = savedMesh.detail;
      if (typeof savedMesh.seed === "string" && /^-?\d{1,20}$/.test(savedMesh.seed)) mesh.seed = savedMesh.seed;
      this.meshParams = mesh;
    } catch { /* keep defaults */ }
  }

  saveSettings() {
    try { localStorage.setItem(`mooshieui.model3d.${getAuthUser() ?? "host"}`, JSON.stringify({ ...this.params, v: SETTINGS_VERSION, engine: this.engine, mesh: this.meshParams })); } catch { /* private mode */ }
  }

  loadEnabled() {
    try { this.enabled = localStorage.getItem(EXPERIMENTAL_KEY) === "1"; } catch { this.enabled = false; }
  }

  setEnabled(value: boolean) {
    this.enabled = value;
    try {
      if (value) localStorage.setItem(EXPERIMENTAL_KEY, "1");
      else localStorage.removeItem(EXPERIMENTAL_KEY);
    } catch { /* private mode */ }
  }

  setEngine(engine: Model3DEngine) {
    if (this.busy || engine === this.engine) return;
    this.engine = engine;
    this.error = "";
    this.saveSettings();
  }

  async refresh() {
    if (this.refreshing) return;
    this.refreshing = true;
    try {
      const [splat, mesh] = await Promise.allSettled([getSplatCapabilities(), getMeshCapabilities()]);
      this.capabilities = splat.status === "fulfilled" ? splat.value : null;
      this.meshCapabilities = mesh.status === "fulfilled" ? mesh.value : null;
      const failed = this.engine === "pixal3d" ? mesh : splat;
      if (failed.status === "rejected") throw failed.reason;
      const models = this.capabilities?.models ?? [];
      if (!models.includes(this.params.model)) {
        this.params.model = models.find((m) => m.toLowerCase().includes("idol")) ?? models[0] ?? "";
      }
      const meshModels = this.meshCapabilities?.models ?? [];
      if (!meshModels.includes(this.meshParams.model)) {
        this.meshParams.model = meshModels.find((m) => /int8|convrot/i.test(m)) ?? meshModels[0] ?? "";
      }
      if (this.error && this.ready) this.error = "";
    } catch (error) {
      this.error = String(error);
    } finally { this.refreshing = false; }
  }

  async downloadMissing() {
    if (this.downloading) return;
    this.downloading = true;
    this.error = "";
    try {
      for (const file of [...this.missingFiles]) {
        this.downloadingFile = file.filename;
        await downloadModel(file.url, file.category, file.filename, undefined, file.sha256);
      }
      await this.refresh();
    } catch (error) {
      this.error = String(error);
    } finally {
      this.downloading = false;
      this.downloadingFile = "";
    }
  }

  setInput(blob: Blob, name: string) {
    if (!/^image\/(png|jpeg|webp)$/.test(blob.type)) {
      this.error = locale.t("model3d.input_type");
      return;
    }
    if (this.inputUrl) URL.revokeObjectURL(this.inputUrl);
    this.inputBlob = blob;
    this.inputUrl = URL.createObjectURL(blob);
    this.inputName = name;
    this.error = "";
  }

  clearInput() {
    if (this.inputUrl) URL.revokeObjectURL(this.inputUrl);
    this.inputBlob = null;
    this.inputUrl = "";
    this.inputName = "";
  }

  async useGalleryImage(image: OutputImage) {
    this.loadingInput = true;
    try {
      const { bytes, filename } = await loadOutputImageForGenerationInput(image, "input.png");
      const view = new Uint8Array(bytes);
      const type = view[0] === 0xff && view[1] === 0xd8 ? "image/jpeg"
        : view[0] === 0x52 && view[8] === 0x57 ? "image/webp" : "image/png";
      this.setInput(new Blob([view], { type }), filename);
    } catch (error) {
      this.error = `${locale.t("model3d.input_failed")} ${String(error)}`;
    } finally { this.loadingInput = false; }
  }

  async loadLibrary() {
    const owner = libraryAccount();
    if (this.libraryOwner === owner && this.libraryLoad) return this.libraryLoad;
    if (this.libraryOwner !== null && this.libraryOwner !== owner) {
      this.results = [];
      this.selectedId = null;
      this.show(null);
      this.loadSettings();
    }
    this.libraryOwner = owner;
    this.libraryLoad = (async () => {
      try {
        const saved = await splatLibrary.load(owner);
        if (this.libraryOwner !== owner) return;
        const known = new Set(this.results.map((r) => r.id));
        this.results = [...this.results, ...saved.filter((r) => !known.has(r.id))].sort((a, b) => b.createdAt - a.createdAt);
      } catch (error) {
        if (this.libraryOwner === owner) this.error = `${locale.t("model3d.library_error")} ${String(error)}`;
      }
    })();
    return this.libraryLoad;
  }

  /** Swap what the viewer shows, freeing the previous mesh's textures. */
  private show(next: SplatData | MeshData | null) {
    const previous = this.current;
    this.current = next;
    if (isMesh(previous) && previous !== next) {
      // After the viewer has uploaded the new model, so nothing draws a closed bitmap.
      setTimeout(() => disposeMesh(previous), 1000);
    }
  }

  async select(result: SplatResult) {
    if (this.selectedId === result.id && this.current) return;
    this.selectedId = result.id;
    this.loadingResult = true;
    this.exportStatus = "";
    try {
      const blob = await splatLibrary.ply(libraryAccount(), result.id);
      if (!blob) throw new Error(locale.t("model3d.missing_file"));
      const buffer = await blob.arrayBuffer();
      const data = result.kind === "mesh" ? await parseGlb(buffer) : parseSplatPly(buffer);
      if (this.selectedId === result.id) this.show(data);
      else if (isMesh(data)) disposeMesh(data);
    } catch (error) {
      if (this.selectedId === result.id) { this.show(null); this.error = String(error); }
    } finally { this.loadingResult = false; }
  }

  async remove(result: SplatResult) {
    try {
      await splatLibrary.remove(libraryAccount(), result.id);
      this.results = this.results.filter((r) => r.id !== result.id);
      if (this.selectedId === result.id) { this.selectedId = null; this.show(null); }
    } catch (error) { this.error = `${locale.t("model3d.library_error")} ${String(error)}`; }
  }

  async rename(result: SplatResult, title: string) {
    const next = { ...result, title: title.trim().slice(0, 120) };
    try {
      await splatLibrary.rename(libraryAccount(), next);
      this.results = this.results.map((r) => (r.id === next.id ? next : r));
    } catch (error) { this.error = `${locale.t("model3d.library_error")} ${String(error)}`; }
  }

  async exportResult(result: SplatResult, format: "ply" | "splat" | "glb") {
    this.exportStatus = "";
    try {
      const stored = await splatLibrary.ply(libraryAccount(), result.id);
      if (!stored) throw new Error(locale.t("model3d.missing_file"));
      const blob = format === "splat"
        ? new Blob([toSplatBytes(parseSplatPly(await stored.arrayBuffer())) as BlobPart], { type: "application/octet-stream" })
        : stored;
      const filename = `${safeName(result.title)}.${format}`;
      if (isTauri) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const path = await save({ defaultPath: filename, filters: [{ name: format.toUpperCase(), extensions: [format] }] });
        if (!path) return;
        const save3d = format === "glb" ? saveMeshFile : saveSplatFile;
        await save3d(await blobToBase64(blob), path);
        this.exportStatus = locale.t("model3d.export_saved");
      } else {
        downloadBlob(blob, filename);
        this.exportStatus = locale.t("model3d.export_downloaded");
      }
    } catch (error) {
      this.error = `${locale.t("model3d.export_failed")} ${String(error)}`;
    }
  }

  async generate() {
    if (this.busy || !this.ready || !this.inputBlob) return;
    this.error = "";
    this.exportStatus = "";
    this.phase = "submitting";
    this.stage = "prepare";
    this.stageValue = 0;
    this.stageMax = 0;
    this.queuePosition = null;
    this.missingPolls = 0;
    this.failedPolls = 0;
    this.cancelling = false;
    this.startedAt = Date.now();
    const engine = this.engine;
    this.jobEngine = engine;
    const params: SplatParams | MeshParams = engine === "pixal3d" ? { ...this.meshParams } : { ...this.params };
    this.submitted = { params, inputUrl: this.inputUrl, title: this.inputName.replace(/\.[a-z0-9]+$/i, "") };
    this.saveSettings();
    try {
      const image = await blobToBase64(this.inputBlob);
      const job = engine === "pixal3d"
        ? await generateMesh(params as MeshParams, image)
        : await generateSplat(params as SplatParams, image);
      this.job = job;
      this.rememberPrompt(job.prompt_id);
      if (this.phase === "submitting") this.phase = "queued";
      this.schedulePoll();
    } catch (error) {
      this.error = String(error);
      this.finish();
    }
  }

  async cancel() {
    if (!this.job) return;
    this.cancelling = true;
    try { await interruptGeneration(this.job.prompt_id); this.finish(); }
    catch (error) { this.error = String(error); }
    finally { this.cancelling = false; }
  }

  private finish() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    this.job = null;
    this.phase = "idle";
    this.queuePosition = null;
  }

  private enterStage(stage: Model3DStage) {
    if (stageOrder.indexOf(stage) < stageOrder.indexOf(this.stage)) return;
    if (stage !== this.stage) { this.stage = stage; this.stageValue = 0; this.stageMax = 0; }
  }

  /** Return true for 3D events so App.svelte keeps them out of image progress. */
  handleEvent(type: string, data: Model3DEvent): boolean {
    const id = data?.prompt_id;
    if (typeof id !== "string") return false;
    if (!this.isModel3DPrompt(id)) return false;
    if (this.job?.prompt_id !== id) return true;
    if (type === "queue") {
      if (typeof data.position === "number" && data.position >= 0) this.queuePosition = data.position + 1;
      return true;
    }
    if (type === "execution_error") {
      this.error = data.exception_message || locale.t("model3d.failed");
      this.finish();
      return true;
    }
    if (type === "execution_interrupted") { this.finish(); return true; }
    if (this.phase === "queued") this.phase = "running";
    this.queuePosition = null;
    const stages = this.jobEngine === "pixal3d" ? meshStages : splatStages;
    const stage = typeof data.node === "string" ? stages[data.node] : undefined;
    if (stage) this.enterStage(stage);
    if (type === "progress" && stage && stage === this.stage && typeof data.value === "number" && typeof data.max === "number" && data.max > 0) {
      this.stageValue = Math.min(data.value, data.max);
      this.stageMax = data.max;
    }
    if (type === "execution_success" || (type === "executing" && data.node === null)) {
      if (this.timer) clearTimeout(this.timer);
      this.timer = setTimeout(() => { this.timer = null; void this.poll(); }, 100);
    }
    return true;
  }

  /** Rough overall progress for the bar, from the stage and its sampler steps. */
  get progressPercent(): number {
    const spans = this.jobEngine === "pixal3d" ? meshSpans : splatSpans;
    const [start, end] = spans[this.stage] ?? [0, 5];
    const within = this.stageMax > 0 ? this.stageValue / this.stageMax : 0;
    return Math.max(2, start + (end - start) * within);
  }

  private schedulePoll() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => { this.timer = null; void this.poll(); }, 2000);
  }

  private async poll() {
    const job = this.job;
    if (!job || this.phase === "retrieving") return;
    try {
      const status = this.jobEngine === "pixal3d" ? await getMeshStatus(job) : await getSplatStatus(job);
      if (this.job?.prompt_id !== job.prompt_id) return;
      this.failedPolls = 0;
      if (status.status === "completed") { await this.retrieve(job); return; }
      if (status.status === "error") { this.error = status.error || locale.t("model3d.failed"); this.finish(); return; }
      if (status.status === "missing") {
        if (++this.missingPolls > 5) { this.error = locale.t("model3d.lost"); this.finish(); return; }
      } else {
        this.missingPolls = 0;
        if (status.status === "running" && this.phase === "queued") this.phase = "running";
      }
      this.schedulePoll();
    } catch (error) {
      if (this.job?.prompt_id !== job.prompt_id) return;
      if (++this.failedPolls > 5) { this.error = String(error); this.finish(); return; }
      this.schedulePoll();
    }
  }

  private async retrieve(job: SplatJob) {
    this.phase = "retrieving";
    this.enterStage("retrieve");
    const submitted = this.submitted;
    const mesh = this.jobEngine === "pixal3d";
    try {
      const blob = mesh
        ? base64ToBlob(await loadMeshGlb(job), "model/gltf-binary")
        : base64ToBlob(await loadSplatPly(job), "application/octet-stream");
      const buffer = await blob.arrayBuffer();
      const data = mesh ? await parseGlb(buffer) : parseSplatPly(buffer);
      const result: SplatResult = {
        ...job,
        id: job.prompt_id,
        kind: mesh ? "mesh" : "splat",
        title: submitted?.title || locale.t("model3d.untitled"),
        params: { ...(submitted?.params ?? (mesh ? this.meshParams : this.params)), seed: job.seed },
        count: isMesh(data) ? glbFaceCount(buffer) || data.faceCount : plyVertexCount(buffer) || data.count,
        thumbnail: submitted ? await thumbnailOf(submitted.inputUrl) : "",
        createdAt: Date.now(),
      };
      const owner = libraryAccount();
      this.results = [result, ...this.results.filter((r) => r.id !== result.id)];
      this.selectedId = result.id;
      this.show(data);
      try { await splatLibrary.save(owner, result, blob); }
      catch (error) { this.error = `${locale.t("model3d.library_error")} ${String(error)}`; }
    } catch (error) {
      this.error = `${locale.t("model3d.retrieve_failed")} ${String(error)}`;
    } finally {
      if (this.job?.prompt_id === job.prompt_id) this.finish();
    }
  }
}

export const model3d = new Model3DStore();
model3d.loadSettings();
model3d.loadEnabled();
