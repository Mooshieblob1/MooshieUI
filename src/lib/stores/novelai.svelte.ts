import { getCachedConfig, getConfig, setNovelaiApiKey } from "../utils/api.js";

/**
 * Whether a NovelAI API key is stored, kept reactive.
 *
 * The config itself is a plain cache in `utils/api.ts` with no change event, so
 * the model dropdown has no way to notice a key being added in Settings. This
 * store is the one piece of NovelAI config the rest of the UI reacts to: the
 * four NovelAI models are only offered once a key exists, because selecting one
 * without a key can only ever produce an error.
 *
 * The key value itself is deliberately not held here. The backend redacts it
 * out of the config sent to browser clients, so the UI only ever knows that one
 * is set.
 */
class NovelAiStore {
  apiKeyConfigured = $state(false);

  /** True once the config has been consulted, so the UI can avoid a flash. */
  loaded = $state(false);

  /**
   * Seed from the config. Safe to call from several components; the underlying
   * `getConfig()` de-duplicates concurrent loads and serves a cache after that.
   */
  async refresh(): Promise<void> {
    const cached = getCachedConfig();
    if (cached) this.applyConfigured(cached);
    try {
      this.applyConfigured(await getConfig());
    } catch (e) {
      console.error("Failed to read NovelAI key state:", e);
    } finally {
      this.loaded = true;
    }
  }

  private applyConfigured(config: {
    novelai_api_key?: string | null;
    novelai_api_key_configured?: boolean;
  }) {
    // Desktop sees the real key, browser mode only the boolean. Either is
    // enough to answer the one question this store exists to answer.
    this.apiKeyConfigured =
      config.novelai_api_key_configured === true || !!config.novelai_api_key?.trim();
  }

  /**
   * Store or clear the key.
   *
   * Not folded into the ordinary config save: `preserve_secrets()` treats a
   * blank incoming key as a stale echo and keeps the stored one, so clearing
   * has to go through the dedicated command.
   */
  async setApiKey(key: string): Promise<void> {
    this.apiKeyConfigured = await setNovelaiApiKey(key);
    this.loaded = true;
  }
}

export const novelai = new NovelAiStore();
