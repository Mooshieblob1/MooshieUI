/**
 * The finishing half of the Moody Moss 4K HD workflow (face detailer, area
 * detailer, skin contrast, SeedVR2 final). Pure helpers shared by the
 * generation store, the Moody card and the generate button; the graph itself
 * is built in `src-tauri/src/templates/moody_moss.rs`.
 */

export interface MoodyMossSettings {
  faceDetail: boolean;
  /** Empty reuses the main prompt. */
  facePrompt: string;
  /** LoRA for the face detailer only; empty for none. */
  faceLora: string;
  faceLoraStrength: number;
  faceDenoise: number;
  /** "Detail Anything": inpainting details the painted area. */
  areaDetail: boolean;
  areaPrompt: string;
  areaLora: string;
  areaLoraStrength: number;
  areaDenoise: number;
  skinContrast: boolean;
  skinContrastStrength: number;
  seedvr2: boolean;
  seedvr2Large: boolean;
  /** Share of the 4x model upscale in the SeedVR2 blend. */
  seedvr2Mix: number;
  longEdge: number;
}

/** The V3 workflow's own values, every optional step off as it ships. */
export const MOODY_MOSS_FINISH_DEFAULTS: MoodyMossSettings = {
  faceDetail: false,
  facePrompt: "",
  faceLora: "",
  faceLoraStrength: 1,
  faceDenoise: 0.35,
  areaDetail: false,
  areaPrompt: "",
  areaLora: "",
  areaLoraStrength: 1,
  areaDenoise: 0.45,
  skinContrast: false,
  skinContrastStrength: 0.2,
  seedvr2: false,
  seedvr2Large: false,
  seedvr2Mix: 0.4,
  longEdge: 4096,
};

export const MOODY_MOSS_LONG_EDGES = [2048, 3072, 4096] as const;

/** Saved settings merged over the defaults, ignoring fields of the wrong type. */
export function restoreMoodyMossSettings(saved: unknown): MoodyMossSettings {
  const out: MoodyMossSettings = { ...MOODY_MOSS_FINISH_DEFAULTS };
  if (!saved || typeof saved !== "object") return out;
  const record = saved as Record<string, unknown>;
  const target = out as unknown as Record<string, unknown>;
  for (const key of Object.keys(out)) {
    const value = record[key];
    if (value !== undefined && typeof value === typeof target[key]) target[key] = value;
  }
  return out;
}

export interface MoodyMossLoraPayload {
  name: string;
  strength_model: number;
  strength_clip: number;
}

/** `GenerationParams.moody_moss` (snake_case, `MoodyMossFinish` in Rust). */
export interface MoodyMossFinishPayload {
  face_detail: boolean;
  face_prompt: string;
  face_loras: MoodyMossLoraPayload[];
  face_denoise: number;
  area_detail: boolean;
  area_prompt: string;
  area_loras: MoodyMossLoraPayload[];
  area_denoise: number;
  skin_contrast: boolean;
  skin_contrast_strength: number;
  seedvr2: boolean;
  seedvr2_7b: boolean;
  seedvr2_mix: number;
  long_edge: number;
}

function loraList(name: string, strength: number): MoodyMossLoraPayload[] {
  const trimmed = name.trim();
  return trimmed ? [{ name: trimmed, strength_model: strength, strength_clip: strength }] : [];
}

export function moodyMossFinishPayload(s: MoodyMossSettings): MoodyMossFinishPayload {
  return {
    face_detail: s.faceDetail,
    face_prompt: s.facePrompt,
    face_loras: loraList(s.faceLora, s.faceLoraStrength),
    face_denoise: s.faceDenoise,
    area_detail: s.areaDetail,
    area_prompt: s.areaPrompt,
    area_loras: loraList(s.areaLora, s.areaLoraStrength),
    area_denoise: s.areaDenoise,
    skin_contrast: s.skinContrast,
    skin_contrast_strength: s.skinContrastStrength,
    seedvr2: s.seedvr2,
    seedvr2_7b: s.seedvr2Large,
    seedvr2_mix: s.seedvr2Mix,
    long_edge: s.longEdge,
  };
}

/** Image size after the SeedVR2 final, which scales the long edge to `longEdge`. */
export function moodyMossLongEdgeSize(
  width: number,
  height: number,
  longEdge: number,
): { width: number; height: number } {
  if (width >= height) {
    return { width: longEdge, height: Math.round((height / width) * longEdge) };
  }
  return { width: Math.round((width / height) * longEdge), height: longEdge };
}

export interface MoodyMossFile {
  url: string;
  category: "diffusion_models" | "vae" | "upscale_models" | "ultralytics";
  filename: string;
  sha256: string;
  /** Size in bytes, for the download toast. */
  bytes: number;
}

const SEEDVR2_HF = "https://huggingface.co/Comfy-Org/SeedVR2/resolve/main";

// Filenames match FACE_DETECTOR / NOMOS_UPSCALER / SEEDVR2_*_UNET_FILE in
// templates/moody_moss.rs and SEEDVR2_VAE_FILE in templates/upscale.rs.
export const MOODY_MOSS_FILES = {
  faceDetector: {
    url: "https://huggingface.co/Bingsu/adetailer/resolve/53cc19de382014514d9d4038601d261a7faa9b7b/face_yolov8m.pt",
    category: "ultralytics",
    filename: "face_yolov8m.pt",
    sha256: "717923c19b3f4bbf5250b728f1fa6b2cb72a33aed1d236ea9caf0e21ad943e5f",
    bytes: 52026019,
  },
  nomos: {
    url: "https://github.com/Phhofm/models/releases/download/4xNomosWebPhoto_RealPLKSR/4xNomosWebPhoto_RealPLKSR.pth",
    category: "upscale_models",
    filename: "4xNomosWebPhoto_RealPLKSR.pth",
    sha256: "a9db66c9b674c6a5025b6ef3bee71a57c33b8605d8a2de0980470f89002efbbe",
    bytes: 29683482,
  },
  seedvr2Small: {
    url: `${SEEDVR2_HF}/diffusion_models/seedvr2_3b_int8_convrot.safetensors`,
    category: "diffusion_models",
    filename: "seedvr2_3b_int8_convrot.safetensors",
    sha256: "c3dec8bcc5916843a8a858572970597462e1f2dc598d6dfd818f6cd40f53a157",
    bytes: 3458259704,
  },
  seedvr2Large: {
    url: `${SEEDVR2_HF}/diffusion_models/seedvr2_7b_fp8_e4m3fn.safetensors`,
    category: "diffusion_models",
    filename: "seedvr2_7b_fp8_e4m3fn.safetensors",
    sha256: "5065e77d647dd553d9090a81e20d6de590d931a61df79d785e008433926ee418",
    bytes: 8240979248,
  },
  seedvr2Vae: {
    url: `${SEEDVR2_HF}/vae/seedvr2_ema_vae_fp16.safetensors`,
    category: "vae",
    filename: "seedvr2_ema_vae_fp16.safetensors",
    sha256: "20678548f420d98d26f11442d3528f8b8c94e57ee046ef93dbb7633da8612ca1",
    bytes: 501324814,
  },
} satisfies Record<string, MoodyMossFile>;

/** Model files the turned-on txt2img finishing steps load. */
export function moodyMossRequiredFiles(s: MoodyMossSettings): MoodyMossFile[] {
  const files: MoodyMossFile[] = [];
  if (s.faceDetail) files.push(MOODY_MOSS_FILES.faceDetector);
  if (s.seedvr2) {
    files.push(s.seedvr2Large ? MOODY_MOSS_FILES.seedvr2Large : MOODY_MOSS_FILES.seedvr2Small);
    files.push(MOODY_MOSS_FILES.seedvr2Vae);
    if (s.seedvr2Mix > 0) files.push(MOODY_MOSS_FILES.nomos);
  }
  return files;
}

export interface InstalledModelLists {
  diffusionModels: string[];
  vaes: string[];
  upscaleModels: string[];
  ultralyticsModels: string[];
}

function listFor(installed: InstalledModelLists, category: MoodyMossFile["category"]): string[] {
  switch (category) {
    case "diffusion_models":
      return installed.diffusionModels;
    case "vae":
      return installed.vaes;
    case "upscale_models":
      return installed.upscaleModels;
    case "ultralytics":
      return installed.ultralyticsModels;
  }
}

/** The required files not installed yet. A file in a subfolder counts as installed. */
export function missingMoodyMossFiles(
  files: MoodyMossFile[],
  installed: InstalledModelLists,
): MoodyMossFile[] {
  return files.filter(
    (f) =>
      !listFor(installed, f.category).some(
        (name) => name === f.filename || name.endsWith(`/${f.filename}`) || name.endsWith(`\\${f.filename}`),
      ),
  );
}

/** Total download size in GB, one decimal. */
export function downloadSizeGb(files: MoodyMossFile[]): string {
  const bytes = files.reduce((sum, f) => sum + f.bytes, 0);
  return (bytes / 1e9).toFixed(1);
}
