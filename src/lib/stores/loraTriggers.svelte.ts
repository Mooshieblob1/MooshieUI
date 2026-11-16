import { triggerSync } from "../utils/syncTrigger.js";
import { userScopedKey } from "../utils/ipc.js";
import { normalizeTrigger } from "../utils/loraTriggerText.js";

/**
 * The user's own trigger text per LoRA file (`@artist`, `yae miko, pink hair`,
 * anything), written into the prompt whenever that LoRA is switched on and
 * taken out when it goes off. Kept on this machine, so it works offline and
 * does not depend on CivitAI knowing the LoRA.
 *
 * Leaf store: `generation` reads it while toggling LoRAs. It rides along in
 * the `lora_presets` category of the shared prefs (see `loraPresets`).
 */

const STORAGE_KEY = "mooshieui.loraTriggers.v1";

function sanitize(raw: unknown): Record<string, string> {
  const out: Record<string, string> = {};
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return out;
  for (const [name, text] of Object.entries(raw as Record<string, unknown>)) {
    if (typeof text !== "string" || !name) continue;
    const trigger = normalizeTrigger(text);
    if (trigger) out[name] = trigger;
  }
  return out;
}

class LoraTriggersStore {
  /** LoRA filename -> trigger text, as written into the prompt. */
  triggers = $state.raw<Record<string, string>>({});

  constructor() {
    try {
      const raw = localStorage.getItem(userScopedKey(STORAGE_KEY));
      if (raw) this.triggers = sanitize(JSON.parse(raw));
    } catch (e) {
      console.error("lora-triggers: load failed", e);
    }
  }

  get(name: string): string {
    return this.triggers[name] ?? "";
  }

  /** Set or clear (empty text) a LoRA's trigger. Returns the stored text. */
  set(name: string, text: string): string {
    if (!name) return "";
    const trigger = normalizeTrigger(text);
    if ((this.triggers[name] ?? "") === trigger) return trigger;
    const next = { ...this.triggers };
    if (trigger) next[name] = trigger;
    else delete next[name];
    this.triggers = next;
    this.persist();
    triggerSync();
    return trigger;
  }

  private persist() {
    try {
      localStorage.setItem(userScopedKey(STORAGE_KEY), JSON.stringify(this.triggers));
    } catch (e) {
      console.error("lora-triggers: save failed", e);
    }
  }

  collectPrefs(): Record<string, string> {
    return this.triggers;
  }

  /** Apply the shared copy. Missing data leaves the local triggers alone. */
  applyServerPrefs(data: unknown) {
    if (!data || typeof data !== "object") return;
    this.triggers = sanitize(data);
    this.persist();
  }
}

export const loraTriggers = new LoraTriggersStore();
