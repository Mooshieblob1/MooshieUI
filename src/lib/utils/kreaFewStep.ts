/**
 * Krea 2 Turbo few-step distillation LoRAs (lvladikov, Krea 2 Community License).
 *
 * Stock Turbo runs at 8 steps. The 4-step adapter is the quality option; the
 * 2-step adapter is a preview option whose author still trains it and warns
 * that small faces and wide scenes can smear. Both are model-only LoRAs
 * (every key is `diffusion_model.*`), so the generic LoraLoader chain applies
 * them and the CLIP strength has no effect.
 *
 * Downloads are pinned to a repo commit because the 2-step file is replaced in
 * place upstream whenever a new checkpoint ships. The local filename carries
 * the checkpoint id so a saved LoRA list names the exact weights it used.
 */

export type KreaTurboSteps = 8 | 4 | 2;

export interface KreaFewStepLora {
  steps: Exclude<KreaTurboSteps, 8>;
  url: string;
  /** Filename we download to inside `models/loras/`. */
  filename: string;
  /** Matches a copy the user already has under the upstream name. */
  installedPattern: RegExp;
}

export const KREA_FEW_STEP_LORAS: readonly KreaFewStepLora[] = [
  {
    steps: 4,
    url: "https://huggingface.co/lvladikov/Krea2-Turbo-Distill-4step-LoRA/resolve/597eb1382f58a1fa38b5694ee19a364ee2690103/krea2_turbo_4step_rank_64_lora_comfyui.safetensors",
    filename: "krea2_turbo_4step_rank_64_lora_comfyui_chk00078000.safetensors",
    installedPattern: /krea2_turbo_4step_rank_64_lora_comfyui[^/\\]*\.safetensors$/i,
  },
  {
    steps: 2,
    url: "https://huggingface.co/lvladikov/Krea2-Turbo-Distill-2step-LoRA/resolve/28959824940c7644e2e66e443d8d0c77ed965570/krea2_turbo_2step_rank_64_lora_comfyui.safetensors",
    filename: "krea2_turbo_2step_rank_64_lora_comfyui_chk00041320.safetensors",
    installedPattern: /krea2_turbo_2step_rank_64_lora_comfyui[^/\\]*\.safetensors$/i,
  },
];

/** Upstream ComfyUI settings: Euler/simple, CFG 1 (Turbo is CFG-free), strength 1. */
export const KREA_TURBO_SAMPLER = "euler";
export const KREA_TURBO_SCHEDULER = "simple";
export const KREA_TURBO_CFG = 1.0;

export function kreaFewStepLoraFor(name: string): KreaFewStepLora | undefined {
  return KREA_FEW_STEP_LORAS.find((l) => l.installedPattern.test(name));
}

/** An installed copy, preferring the pinned file over one under the upstream name. */
export function installedKreaFewStepLora(lora: KreaFewStepLora, installed: readonly string[]): string | undefined {
  return (
    installed.find((name) => name.split(/[\\/]/).pop() === lora.filename) ??
    installed.find((name) => lora.installedPattern.test(name))
  );
}

/** Step mode implied by the LoRA list: the fewest-step adapter that is enabled, else stock 8. */
export function activeKreaTurboSteps(loras: readonly { name: string; enabled: boolean }[]): KreaTurboSteps {
  let steps: KreaTurboSteps = 8;
  for (const l of loras) {
    if (!l.enabled) continue;
    const match = kreaFewStepLoraFor(l.name);
    if (match && match.steps < steps) steps = match.steps;
  }
  return steps;
}
