import type { VideoVariant, VideoTurboPreset } from "../types/index.js";

/**
 * The MiniMax H3 model stack, as data.
 *
 * Video mode only supports H3 right now, so the settings panel does not ask the
 * user to assemble four files out of four dropdowns — it asks for one quality
 * tier and derives the rest. A tier plus the fl2va/ref2va variant fully
 * determines the DiT; the text encoder and both VAEs are shared by every tier
 * (the published stacks pair the same Qwen3-VL encoder and the same video/audio
 * VAEs with every DiT quantisation).
 *
 * Sizes are the exact byte counts served by Hugging Face, so the panel can show
 * a real download size before anything is fetched.
 */

const COMFY_ORG = "https://huggingface.co/Comfy-Org/MiniMax-H3/resolve/main";
const NVFP4_REPO = "https://huggingface.co/lilcheaty/MiniMax-H3-NVFP4/resolve/main";
const TURBO_REPO = "https://huggingface.co/larryvrh/MiniMax-H3-Turbo-Lora/resolve/main";

/** ComfyUI model directories, matching `category_subdirs()` in `commands/api.rs`. */
export type H3ModelCategory = "diffusion_models" | "text_encoders" | "vae" | "loras";

export interface H3ModelFile {
  /** Name on disk inside the category directory. */
  filename: string;
  url: string;
  category: H3ModelCategory;
  /** Exact size, for the pre-download estimate and the progress rows. */
  sizeBytes: number;
  sha256?: string;
}

export type H3TierId = "nvfp4" | "int8" | "fp8" | "bf16" | "custom";

/**
 * Locale key for the custom tier label. The tier has no fixed files and no
 * download — the user supplies all four model paths from the model store.
 */
export const H3_CUSTOM_TIER_LABEL_KEY = "generation.video.stack.custom";

export interface H3Tier {
  id: H3TierId;
  /** Locale key under `generation.video.stack.` for the display name. */
  labelKey: string;
  /** Per-variant DiT weights — fl2va and ref2va are separate files. */
  diffusion: Record<VideoVariant, H3ModelFile>;
  /**
   * Tiers below this compute capability are offered but flagged: NVFP4 needs
   * Blackwell (CC 12.0) to run at its advertised speed.
   */
  minComputeCapability?: number;
}

/**
 * Shared across every tier. The NVFP4-AWQ encoder is the one the official
 * ComfyUI stack ships; the alternatives are 27 GB and 51 GB, which no consumer
 * card benefits from.
 */
export const H3_TEXT_ENCODER: H3ModelFile = {
  filename: "qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors",
  url: `${COMFY_ORG}/text_encoders/qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors`,
  category: "text_encoders",
  sizeBytes: 15_687_142_551,
};

export const H3_VIDEO_VAE: H3ModelFile = {
  filename: "minimax_h3_video_vae_fp16.safetensors",
  url: `${COMFY_ORG}/vae/minimax_h3_video_vae_fp16.safetensors`,
  category: "vae",
  sizeBytes: 5_207_808_496,
};

export const H3_AUDIO_VAE: H3ModelFile = {
  filename: "minimax_h3_audio_vae_fp32.safetensors",
  url: `${COMFY_ORG}/vae/minimax_h3_audio_vae_fp32.safetensors`,
  category: "vae",
  sizeBytes: 605_254_808,
};

/**
 * Turbo adapter for few-step sampling. Mirrors `H3_TURBO_LORA_FILENAME` and
 * `H3_TURBO_LORA_URL` in `src-tauri/src/comfyui/nodes.rs` — the backend falls
 * back to that filename when the client sends none, so the two must agree.
 */
export const H3_TURBO_LORA: H3ModelFile = {
  filename: "minimax_h3_turbo_v4_step600_ema.safetensors",
  url: `${TURBO_REPO}/minimax_h3_turbo_v4_step600_ema.safetensors`,
  category: "loras",
  sizeBytes: 779_849_816,
};

export interface H3TurboPreset {
  id: VideoTurboPreset;
  label: string;
  variant?: VideoVariant;
  steps?: number;
  videoShift: number;
  file: H3ModelFile;
}

const LIGHTX2V_REPO = "https://huggingface.co/lightx2v/Minimax-h3-Turbo/resolve/3ec17a324ced54151364f24f8b5fb6bf7e26414f";
function lightxFile(filename: string, sha256: string): H3ModelFile {
  return { filename, url: `${LIGHTX2V_REPO}/${filename}`, category: "loras", sizeBytes: 1_956_193_000, sha256 };
}
/**
 * Alibaba PAI's Parallel Decoding Distillation adapters, in Kijai's ComfyUI
 * conversion. The pruned pair matches the pruned DiT every tier ships. Needs
 * ComfyUI v0.35.0 or newer, where the stock LoRA loader learned PDD head banks.
 */
const KIJAI_H3_EXPERIMENTAL_REPO = "https://huggingface.co/Kijai/MiniMax-H3-experimental/resolve/e042fe480f58806578713532b8ae4e3d47d1bd63/loras";
function pddFile(filename: string, sha256: string): H3ModelFile {
  return { filename, url: `${KIJAI_H3_EXPERIMENTAL_REPO}/${filename}`, category: "loras", sizeBytes: 1_725_921_392, sha256 };
}
export const H3_TURBO_PRESETS: readonly H3TurboPreset[] = [
  { id: "larryvrh", label: "Larryvrh v4", videoShift: 12, file: H3_TURBO_LORA },
  { id: "lightx2v_fl2v_4", label: "LightX2V FL2V v1.2 · 4", variant: "fl2va", steps: 4, videoShift: 6,
    file: lightxFile("minimax_h3_fl2v_turbo_4step_v1.2_768p_comfyui_bf16.safetensors", "c8168ebc17bbacc4296103dda2fec1ba85b24392fa08cf2bfbcef0cff0dc3cc8") },
  { id: "lightx2v_fl2v_8", label: "LightX2V FL2V v1.0 · 8", variant: "fl2va", steps: 8, videoShift: 6,
    file: lightxFile("minimax_h3_fl2v_turbo_8step_v1.0_768p_comfyui_bf16.safetensors", "08cfe946033af7d27719b964b6e0a0e50c32138daabbd6ce4137e23df6bf9980") },
  { id: "lightx2v_ref2v_8", label: "LightX2V Ref2V v1.0 · 8", variant: "ref2va", steps: 8, videoShift: 12,
    file: lightxFile("minimax_h3_ref2v_turbo_8step_v1.0_768p_comfyui_bf16.safetensors", "6a56f41ab4229c9dd845b9501bbd475ee57e112d846cf2e819d534a1ae928c5a") },
  { id: "pdd_fl2va_8", label: "PDD FL2VA · 8", variant: "fl2va", steps: 8, videoShift: 12,
    file: pddFile("MiniMax-H3-FL2VA-Acc-8Step_pruned_comfy.safetensors", "e97b813a6f857b9dab310f31ec30a8334f63a3e7dcb5d07c0c91933d3447a897") },
  { id: "pdd_ref2va_8", label: "PDD Ref2VA · 8", variant: "ref2va", steps: 8, videoShift: 12,
    file: pddFile("MiniMax-H3-Ref2VA-Acc-8Step_pruned_comfy.safetensors", "6f18e1c2eccb14b37322607730f26b16bf1169b56cd098ea006cffaec43d1e39") },
];

/** PDD swaps output heads every step, which rules out replaying cached outputs. */
export function isPddPreset(id: VideoTurboPreset): boolean {
  return id.startsWith("pdd_");
}

/** Keep the chosen family while switching between native and reference tasks. */
export function h3TurboPreset(id: unknown, variant: VideoVariant): H3TurboPreset {
  const preset = H3_TURBO_PRESETS.find(p => p.id === id) ?? H3_TURBO_PRESETS[0];
  if (!preset.variant || preset.variant === variant) return preset;
  const counterpart: VideoTurboPreset = isPddPreset(preset.id)
    ? (variant === "ref2va" ? "pdd_ref2va_8" : "pdd_fl2va_8")
    : (variant === "ref2va" ? "lightx2v_ref2v_8" : "lightx2v_fl2v_8");
  return H3_TURBO_PRESETS.find(p => p.id === counterpart)!;
}

export const H3_TIERS: readonly H3Tier[] = [
  {
    id: "nvfp4",
    labelKey: "generation.video.stack.nvfp4",
    minComputeCapability: 12.0,
    diffusion: {
      fl2va: {
        filename: "minimax_h3_fl2va_pruned_nvfp4.safetensors",
        url: `${NVFP4_REPO}/minimax_h3_fl2va_pruned_nvfp4.safetensors`,
        category: "diffusion_models",
        sizeBytes: 12_528_636_800,
      },
      ref2va: {
        filename: "minimax_h3_ref2va_pruned_nvfp4.safetensors",
        url: `${NVFP4_REPO}/minimax_h3_ref2va_pruned_nvfp4.safetensors`,
        category: "diffusion_models",
        sizeBytes: 12_528_636_800,
      },
    },
  },
  {
    id: "int8",
    labelKey: "generation.video.stack.int8",
    diffusion: {
      fl2va: {
        filename: "minimax_h3_fl2va_pruned_int8_convrot.safetensors",
        url: `${COMFY_ORG}/diffusion_models/minimax_h3_fl2va_pruned_int8_convrot.safetensors`,
        category: "diffusion_models",
        sizeBytes: 20_970_379_616,
      },
      ref2va: {
        filename: "minimax_h3_ref2va_pruned_int8_convrot.safetensors",
        url: `${COMFY_ORG}/diffusion_models/minimax_h3_ref2va_pruned_int8_convrot.safetensors`,
        category: "diffusion_models",
        sizeBytes: 20_970_379_616,
      },
    },
  },
  {
    id: "fp8",
    labelKey: "generation.video.stack.fp8",
    diffusion: {
      fl2va: {
        filename: "minimax_h3_fl2va_pruned_fp8_scaled.safetensors",
        url: `${COMFY_ORG}/diffusion_models/minimax_h3_fl2va_pruned_fp8_scaled.safetensors`,
        category: "diffusion_models",
        sizeBytes: 20_958_205_608,
      },
      ref2va: {
        filename: "minimax_h3_ref2va_pruned_fp8_scaled.safetensors",
        url: `${COMFY_ORG}/diffusion_models/minimax_h3_ref2va_pruned_fp8_scaled.safetensors`,
        category: "diffusion_models",
        sizeBytes: 20_958_205_608,
      },
    },
  },
  {
    id: "bf16",
    labelKey: "generation.video.stack.bf16",
    diffusion: {
      fl2va: {
        filename: "minimax_h3_fl2va_pruned_bf16.safetensors",
        url: `${COMFY_ORG}/diffusion_models/minimax_h3_fl2va_pruned_bf16.safetensors`,
        category: "diffusion_models",
        sizeBytes: 40_225_724_176,
      },
      ref2va: {
        filename: "minimax_h3_ref2va_pruned_bf16.safetensors",
        url: `${COMFY_ORG}/diffusion_models/minimax_h3_ref2va_pruned_bf16.safetensors`,
        category: "diffusion_models",
        sizeBytes: 40_225_724_176,
      },
    },
  },
] as const;

/** Picked when nothing is installed and the GPU is not Blackwell. */
export const H3_DEFAULT_TIER: H3TierId = "int8";

export interface H3Stack {
  diffusion: H3ModelFile;
  textEncoder: H3ModelFile;
  videoVae: H3ModelFile;
  audioVae: H3ModelFile;
}

export function h3Tier(id: H3TierId): H3Tier | undefined {
  return H3_TIERS.find((tier) => tier.id === id);
}

/** The four files a tier needs for one variant, or null for an unknown tier. */
export function h3Stack(id: H3TierId, variant: VideoVariant): H3Stack | null {
  const tier = h3Tier(id);
  if (!tier) return null;
  return {
    diffusion: tier.diffusion[variant],
    textEncoder: H3_TEXT_ENCODER,
    videoVae: H3_VIDEO_VAE,
    audioVae: H3_AUDIO_VAE,
  };
}

/** Stack members in download order — DiT first, so the big file starts early. */
export function h3StackFiles(stack: H3Stack): H3ModelFile[] {
  return [stack.diffusion, stack.textEncoder, stack.videoVae, stack.audioVae];
}

export type H3Role = "fl2va" | "ref2va" | "textEncoder" | "videoVae" | "audioVae";

export interface H3StackEntry {
  role: H3Role;
  /** Locale key for the row's role label. */
  labelKey: string;
  file: H3ModelFile;
}

/**
 * Every file a tier can use, both DiT variants included, in download order.
 * `h3Stack()` stays variant-scoped and keeps driving what the generation store
 * considers ready; this is the full picture the Models UI lists.
 */
export function h3TierFiles(id: H3TierId): H3StackEntry[] {
  const tier = h3Tier(id);
  if (!tier) return [];
  return [
    {
      role: "fl2va",
      labelKey: "generation.video.role_fl2va",
      file: tier.diffusion.fl2va,
    },
    {
      role: "ref2va",
      labelKey: "generation.video.role_ref2va",
      file: tier.diffusion.ref2va,
    },
    {
      role: "textEncoder",
      labelKey: "generation.video.role_text_encoder",
      file: H3_TEXT_ENCODER,
    },
    { role: "videoVae", labelKey: "generation.video.role_video_vae", file: H3_VIDEO_VAE },
    { role: "audioVae", labelKey: "generation.video.role_audio_vae", file: H3_AUDIO_VAE },
  ];
}

/**
 * Which preset tier a DiT filename belongs to, or null when it is not a stack
 * file we know (including custom-tier or unrecognised files). Order matters: the
 * int8-over-NVFP4 mixed builds carry both markers, and they are NVFP4 first.
 * Never returns "custom" — the panel tracks that separately via
 * `generation.videoModelTier`.
 */
export function h3TierForDiffusionModel(filename: string | null | undefined): Exclude<H3TierId, "custom"> | null {
  const name = (filename ?? "").toLowerCase();
  if (!name) return null;
  if (name.includes("nvfp4")) return "nvfp4";
  if (name.includes("int8")) return "int8";
  if (name.includes("fp8")) return "fp8";
  if (name.includes("bf16") || name.includes("fp16")) return "bf16";
  return null;
}
