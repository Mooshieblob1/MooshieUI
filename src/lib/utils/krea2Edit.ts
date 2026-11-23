/**
 * Krea 2 Image Edit: the edit LoRAs behind each recipe (see
 * src-tauri/src/templates/krea2_edit.rs).
 *
 * - Identity Edit V2 and General Edit V2 (conradlocke/krea2-identity-edit):
 *   free for personal and non-commercial use; commercial use needs a licence
 *   from the author. They need the comfyui-krea2edit nodes, which the app
 *   installs with its other pinned node packs.
 * - Style Reference (ostris, mirrored by Comfy-Org/Krea-2): Krea 2 Community
 *   License, core ComfyUI nodes only.
 *
 * Downloads are pinned to a repo commit and checked against their SHA-256.
 * Any size of a method's LoRA that is already installed is used, largest
 * first, so a full-size copy someone downloaded by hand works too.
 */

export type Krea2EditMethod = "identity" | "general" | "style";

export const KREA2_EDIT_METHODS: readonly Krea2EditMethod[] = ["general", "identity", "style"];

export interface Krea2EditLora {
  method: Krea2EditMethod;
  /** Download used by the Image Edit panel and the Model Hub. */
  url: string;
  filename: string;
  sha256: string;
  bytes: number;
  /** Installed copies that count, largest first. */
  installedNames: readonly string[];
  /** Model card `ref_boost` (1 = off). */
  defaultRefBoost: number;
  /** Model card placement for a single reference: beside lets a person be restaged freely. */
  defaultBeside: boolean;
}

const CONRAD_REV = "f33b26291c640e8ec50b0da420bfee3c9a660daa";
const COMFY_KREA2_REV = "588feaf955b08b9ac4723eeb751752ab045dba71";

export const KREA2_EDIT_LORAS: Record<Krea2EditMethod, Krea2EditLora> = {
  identity: {
    method: "identity",
    url: `https://huggingface.co/conradlocke/krea2-identity-edit/resolve/${CONRAD_REV}/krea2_identity_edit_v2_r128.safetensors`,
    filename: "krea2_identity_edit_v2_r128.safetensors",
    sha256: "2082515f1eb66532f5efb5343eb15048a29e3abe9f356e35e784cc60d00b45f7",
    bytes: 914_188_768,
    installedNames: [
      "krea2_identity_edit_v2.safetensors",
      "krea2_identity_edit_v2_r128.safetensors",
      "krea2_identity_edit_v2_r64.safetensors",
    ],
    defaultRefBoost: 2,
    defaultBeside: true,
  },
  general: {
    method: "general",
    url: `https://huggingface.co/conradlocke/krea2-identity-edit/resolve/${CONRAD_REV}/krea2_general_edit_v2_r128.safetensors`,
    filename: "krea2_general_edit_v2_r128.safetensors",
    sha256: "fbca6ab74b9bef3db8de1d4a49b93d4fe7a778f5d4ae6995e0769ea8de736f00",
    bytes: 914_188_760,
    installedNames: [
      "krea2_general_edit_v2.safetensors",
      "krea2_general_edit_v2_r128.safetensors",
      "krea2_general_edit_v2_r64.safetensors",
    ],
    defaultRefBoost: 4,
    defaultBeside: false,
  },
  style: {
    method: "style",
    url: `https://huggingface.co/Comfy-Org/Krea-2/resolve/${COMFY_KREA2_REV}/loras/krea2_style_reference.safetensors`,
    filename: "krea2_style_reference.safetensors",
    sha256: "f50df5a9e62e4be8aa926a63dd5bb1a64770c4004f763c1208007ae13daa82b8",
    bytes: 457_111_760,
    installedNames: ["krea2_style_reference.safetensors"],
    defaultRefBoost: 1,
    defaultBeside: false,
  },
};

/** Most reference images a Krea 2 edit takes (KREA2_MAX_REFERENCES in krea2_edit.rs). */
export const KREA2_EDIT_MAX_REFERENCES = 2;

export function parseKrea2EditMethod(value: unknown): Krea2EditMethod {
  return value === "identity" || value === "style" ? value : "general";
}

function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return (parts[parts.length - 1] ?? path).toLowerCase();
}

/**
 * The installed LoRA (as the models list names it, subfolder included) for a
 * method, or null when none is installed.
 */
export function installedKrea2EditLora(
  loras: readonly string[],
  method: Krea2EditMethod,
): string | null {
  for (const name of KREA2_EDIT_LORAS[method].installedNames) {
    const hit = loras.find((l) => baseName(l) === name);
    if (hit) return hit;
  }
  return null;
}

/** Krea 2 edits stay at or under 2 MP (model card); scale a size down to fit, multiples of 16. */
export function fitKrea2EditSize(width: number, height: number): { width: number; height: number } {
  const maxPixels = 2 * 1024 * 1024;
  const scale = Math.min(1, Math.sqrt(maxPixels / Math.max(1, width * height)));
  const snap = (v: number) => Math.max(16, Math.floor((v * scale) / 16) * 16);
  return { width: snap(width), height: snap(height) };
}
