/**
 * Stable Cascade's second model. The user picks stage C (the model LoRAs and
 * fine-tunes target); stage B, which turns stage C's tiny latent into the
 * full-size image, is found or downloaded automatically and sent in
 * `architecture_files`. Slots mirror src-tauri/src/templates/cascade.rs.
 */

export const CASCADE_STAGE_B_SLOT = "stage_b";
export const CASCADE_STAGE_B_UNET_SLOT = "stage_b_unet";
export const CASCADE_STAGE_A_SLOT = "stage_a";

export interface CascadeFile {
  filename: string;
  url: string;
  category: string;
}

const REPO = "https://huggingface.co/stabilityai/stable-cascade/resolve/main";

/** Official all-in-one stage B (bf16 stage B + stage A), ComfyUI checkpoint format. */
export const CASCADE_STAGE_B_CHECKPOINT: CascadeFile = {
  filename: "stable_cascade_stage_b.safetensors",
  url: `${REPO}/comfyui_checkpoints/stable_cascade_stage_b.safetensors`,
  category: "checkpoints",
};

/** Stage A VAE, which decodes a bare stage B diffusion model. */
export const CASCADE_STAGE_A: CascadeFile = {
  filename: "stage_a.safetensors",
  url: `${REPO}/stage_a.safetensors`,
  category: "vae",
};

/** Stage C's CLIP-G text encoder, for a split (diffusion model) stage C. */
export const CASCADE_TEXT_ENCODER: CascadeFile = {
  filename: "stable_cascade_text_encoder.safetensors",
  url: `${REPO}/text_encoder/model.safetensors`,
  category: "text_encoders",
};

/** EfficientNet encoder: stage C's "VAE", used to encode img2img inputs. */
export const CASCADE_EFFNET: CascadeFile = {
  filename: "effnet_encoder.safetensors",
  url: `${REPO}/effnet_encoder.safetensors`,
  category: "vae",
};

function base(filename: string): string {
  return filename.replace(/\\/g, "/").split("/").pop()!.toLowerCase();
}

export function isCascadeStageB(filename: string | null | undefined): boolean {
  return !!filename && /stage[_-]?b/.test(base(filename));
}

export function isCascadeStageA(filename: string | null | undefined): boolean {
  return !!filename && /stage[_-]?a(\b|[_.-])/.test(base(filename));
}

export function isCascadeEffnet(filename: string | null | undefined): boolean {
  return !!filename && base(filename).includes("effnet");
}

/** Stage C text encoder: the Cascade copy, or SDXL's identical CLIP-G. */
export function isCascadeTextEncoder(filename: string | null | undefined): boolean {
  if (!filename) return false;
  const name = base(filename);
  return name.includes("cascade") || /clip[_-]?g\b|clip_g[_.]/.test(name);
}

/** Prefer the file already chosen, then the full model over the lite one. */
function prefer(files: string[], current: string | undefined): string | undefined {
  if (current && files.includes(current)) return current;
  return files.find((f) => !base(f).includes("lite")) ?? files[0];
}

export interface CascadeStageBPlan {
  /** Slots to send, when everything they name is installed. */
  files: Record<string, string> | null;
  /** What to download first. Empty when `files` is set. */
  download: CascadeFile[];
}

/**
 * Pick stage B from what is installed: the all-in-one checkpoint first,
 * then a bare stage B diffusion model with stage A. With nothing usable,
 * plan the smallest download that completes a setup.
 */
export function planCascadeStageB(
  installed: { checkpoints: string[]; diffusionModels: string[]; vaes: string[] },
  current: Record<string, string>,
): CascadeStageBPlan {
  const ckpt = prefer(installed.checkpoints.filter(isCascadeStageB), current[CASCADE_STAGE_B_SLOT]);
  if (ckpt) return { files: { [CASCADE_STAGE_B_SLOT]: ckpt }, download: [] };

  const unet = prefer(
    installed.diffusionModels.filter(isCascadeStageB),
    current[CASCADE_STAGE_B_UNET_SLOT],
  );
  if (unet) {
    const stageA = prefer(installed.vaes.filter(isCascadeStageA), current[CASCADE_STAGE_A_SLOT]);
    if (stageA) {
      return {
        files: { [CASCADE_STAGE_B_UNET_SLOT]: unet, [CASCADE_STAGE_A_SLOT]: stageA },
        download: [],
      };
    }
    return { files: null, download: [CASCADE_STAGE_A] };
  }
  return { files: null, download: [CASCADE_STAGE_B_CHECKPOINT] };
}

/** `current` with the Cascade slots replaced by `files`, other slots kept. */
export function withCascadeFiles(
  current: Record<string, string>,
  files: Record<string, string>,
): Record<string, string> {
  const next = { ...current };
  delete next[CASCADE_STAGE_B_SLOT];
  delete next[CASCADE_STAGE_B_UNET_SLOT];
  delete next[CASCADE_STAGE_A_SLOT];
  return { ...next, ...files };
}

export function sameFiles(a: Record<string, string>, b: Record<string, string>): boolean {
  const ka = Object.keys(a);
  return ka.length === Object.keys(b).length && ka.every((k) => a[k] === b[k]);
}
