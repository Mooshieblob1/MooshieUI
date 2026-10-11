import type { EnvKeyRef, KeyEnvField, KeyEnvStatus } from "../types/index.js";
import { apiKeyEnvStatus } from "../utils/api.js";

/** Prefix the backend reads as "take the key from this variable". */
export const ENV_PREFIX = "env:";

/** Whether a stored or typed key value names a variable rather than holding a key. */
export function isEnvRef(value: string | null | undefined): boolean {
  return (value ?? "").trim().startsWith(ENV_PREFIX);
}

/**
 * The value to save for a key field: the pasted key, or `env:NAME` when the
 * field is in variable mode. A leading `$` or a pasted `env:` is dropped from
 * the name so `$FAL_KEY` and `FAL_KEY` both work. Empty stays empty (a clear).
 */
export function toStoredKey(input: string, envMode: boolean): string {
  const value = input.trim();
  if (!value || !envMode) return value;
  const name = value.replace(/^env:/, "").replace(/^\$/, "").trim();
  return name ? `${ENV_PREFIX}${name}` : "";
}

/**
 * Which of the owner's API key fields read from an environment variable.
 *
 * Only the instance owner (desktop, localhost, admin) may name a variable, so
 * `available` is false for every named account and the option is not shown.
 */
class KeyEnvStore {
  status = $state<KeyEnvStatus>({ available: false, keys: {} });

  get available(): boolean {
    return this.status.available;
  }

  ref(field: KeyEnvField): EnvKeyRef | undefined {
    return this.status.keys[field];
  }

  #requested = false;

  /** Fetch once per page load; saves call `refresh()` themselves. */
  ensureLoaded(): void {
    if (this.#requested) return;
    this.#requested = true;
    void this.refresh();
  }

  async refresh(): Promise<void> {
    try {
      this.status = await apiKeyEnvStatus();
    } catch (e) {
      console.error("Failed to read API key environment status:", e);
    }
  }
}

export const keyEnv = new KeyEnvStore();
