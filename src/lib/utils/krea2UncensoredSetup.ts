/**
 * Installing and switching Krea 2 uncensored mode, shared by the model panel
 * and the "LoRA missing" notification. Kept apart from `krea2Encoder.ts`
 * because it touches stores, and the generation store imports that file.
 */
import { generation } from "../stores/generation.svelte.js";
import { models } from "../stores/models.svelte.js";
import { notifications } from "../stores/notifications.svelte.js";
import { downloadModel } from "./api.js";
import { userScopedKey } from "./ipc.js";
import {
  KREA2_REFUSAL_LORA,
  KREA2_UNCENSORED_ENCODER,
  installedKrea2RefusalLora,
  isKrea2Encoder,
  isKrea2UncensoredEncoder,
} from "./krea2Encoder.js";

export type Krea2UncensoredFile = typeof KREA2_UNCENSORED_ENCODER | typeof KREA2_REFUSAL_LORA;

/** The uncensored-mode files not installed yet. */
export function missingKrea2UncensoredFiles(): Krea2UncensoredFile[] {
  return [
    ...(models.textEncoders.some(isKrea2UncensoredEncoder) ? [] : [KREA2_UNCENSORED_ENCODER]),
    ...(installedKrea2RefusalLora(models.loras) ? [] : [KREA2_REFUSAL_LORA]),
  ];
}

/** Switch uncensored mode, selecting the matching installed encoder. */
export function setKrea2Uncensored(on: boolean): void {
  generation.krea2UncensoredEncoder = on;
  const encoder = models.textEncoders.find((f) => isKrea2Encoder(f) && isKrea2UncensoredEncoder(f) === on);
  if (encoder) {
    generation.clipModel = encoder;
    generation.clipType = "krea2";
  }
  generation.saveSettings();
}

/**
 * Download whatever uncensored mode is missing, in parallel, then turn it on.
 * Progress shows in the global download banner; `afterEach` lets the model
 * panel cache each file's hash as it lands.
 */
export async function installKrea2Uncensored(
  afterEach?: (file: Krea2UncensoredFile) => Promise<void>,
): Promise<void> {
  const missing = missingKrea2UncensoredFiles();
  await Promise.all(
    missing.map(async (file) => {
      await downloadModel(file.url, file.category, file.filename);
      await afterEach?.(file);
    }),
  );
  await models.refresh();
  setKrea2Uncensored(true);
}

/** Notification title key; NotificationBell keys its action button off it. */
export const KREA2_REFUSAL_LORA_NOTIF_TITLE = "notifications.krea2_refusal_lora.title";
const NOTIFIED_KEY = "mooshie-krea2-refusal-lora-notified";

/**
 * Tell a user who has the uncensored encoder but not the refusal-reduction
 * LoRA that uncensored mode gained it. Once per user: a cleared notification
 * stays cleared.
 */
export function notifyKrea2RefusalLoraMissing(): void {
  const key = userScopedKey(NOTIFIED_KEY);
  try {
    if (globalThis.localStorage?.getItem(key)) return;
  } catch {
    // Storage blocked: fall back to the duplicate check below.
  }
  if (notifications.notifications.some((n) => n.local && n.i18n && n.title === KREA2_REFUSAL_LORA_NOTIF_TITLE)) return;
  notifications.addLocalNotification({
    i18n: true,
    title: KREA2_REFUSAL_LORA_NOTIF_TITLE,
    body: "notifications.krea2_refusal_lora.body",
    params: { size_bytes: KREA2_REFUSAL_LORA.bytes },
    kind: "info",
  });
  try {
    globalThis.localStorage?.setItem(key, "1");
  } catch {
    // Non-critical
  }
}
