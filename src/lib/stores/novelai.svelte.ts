import type { NovelAiSubscription } from "../types/index.js";
import {
  getCachedConfig,
  getConfig,
  novelaiSubscription,
  setNovelaiApiKey,
} from "../utils/api.js";

/** How long a failed subscription fetch suppresses automatic retries. */
const RETRY_AFTER_FAILURE_MS = 30_000;

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

  /**
   * Whether the character placement modal is open.
   *
   * Lives here rather than in the component that opens it because the modal is
   * mounted at the app root: the canvas needs the full window to place anything
   * precisely, and a `fixed` overlay rendered inside the prompt panel is bound
   * by that panel's scroll container. Runtime only, never persisted.
   */
  characterPositionOpen = $state(false);

  /** True once the config has been consulted, so the UI can avoid a flash. */
  loaded = $state(false);

  /**
   * The account record behind the Anlas and Opus readouts.
   *
   * Null until it is fetched, and fetched only on demand: it is a live call to
   * NovelAI, so it must not fire on every settings render.
   */
  subscription = $state<NovelAiSubscription | null>(null);
  subscriptionLoading = $state(false);
  subscriptionError = $state<string | null>(null);

  /**
   * Whether a fetch has been attempted for the current key.
   *
   * `ensureSubscription()` reads this so a component that mounts on every
   * generation-page render fetches once, and so a failed fetch is not retried
   * in a loop by an effect watching `subscription`.
   */
  private attempted = false;

  /**
   * Failure bookkeeping for `ensureSubscription()` retries. Deliberately
   * plain fields, not `$state`: the callers are `$effect`s, and a tracked
   * read here would re-run them on every failed fetch and loop.
   */
  private lastFetchFailed = false;
  private lastFailureAt = 0;

  /** Anlas is the monthly allowance plus any purchased balance. */
  get anlas(): number {
    const steps = this.subscription?.trainingStepsLeft;
    if (!steps) return 0;
    return steps.fixedTrainingStepsLeft + steps.purchasedTrainingSteps;
  }

  /** Opus is tier 3, and only while the subscription is active. */
  get isOpus(): boolean {
    const sub = this.subscription;
    return !!sub && sub.active && sub.tier >= 3;
  }

  /** What the Opus usage bar draws, or null when there is no bar to draw. */
  get opusAllowance() {
    return this.subscription?.opusAllowance ?? null;
  }

  /**
   * The Opus V5 generation allowance is used up.
   *
   * Only meaningful for V5 models, which are excluded from Opus unlimited:
   * callers combine this with `naiV5Variant` before feeding it to the cost
   * estimate. False when there is no allowance record at all, because a
   * missing record means a non-Opus account and `isOpus` already gates those.
   */
  get opusAllowanceEmpty(): boolean {
    return this.opusAllowance?.isEmpty ?? false;
  }

  /**
   * Seed from the config. Safe to call from several components; the underlying
   * `getConfig()` de-duplicates concurrent loads and serves a cache after that.
   */
  async refresh(): Promise<void> {
    // A pre-seed to avoid a flash, nothing more. Both this and the read below
    // go through `applyConfigured`, which only ever turns the flag on.
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
    const configured =
      config.novelai_api_key_configured === true || !!config.novelai_api_key?.trim();
    // Only ever turns the flag on. A config read cannot prove a key is gone:
    // `updateConfig` writes the frontend's own copy straight into the config
    // cache, and that copy carries a blanked key (the backend's
    // `preserve_secrets()` is what keeps the stored one), so an ordinary
    // settings save would otherwise report "no key" on the next mount and take
    // the NovelAI models, the Anlas readout and the Opus discount with it.
    // Clearing goes through `setApiKey("")`, which sets the flag directly.
    if (configured) this.apiKeyConfigured = true;
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
    // The key lives outside the ordinary config write, so the cached config in
    // `utils/api.ts` still describes the pre-save state. `refresh()` reads that
    // cache on every ModelSelector and SettingsPage mount, so without a forced
    // reload the next mount reports "no key" and the models vanish again.
    await getConfig({ force: true }).catch((e) => {
      console.error("Failed to reload config after NovelAI key change:", e);
    });
    this.loaded = true;
    // The old account record belongs to the old key, so it goes either way.
    this.subscription = null;
    this.subscriptionError = null;
    this.attempted = false;
    if (this.apiKeyConfigured) await this.refreshSubscription();
  }

  /**
   * Fetch the account record unless one has already been asked for.
   *
   * This is what the usage readout above the generate button calls, since it
   * mounts far more often than Settings does and must not turn every render
   * into a call to NovelAI.
   */
  async ensureSubscription(): Promise<void> {
    if (this.attempted && !this.lastFetchFailed) return;
    // A failed fetch (offline at startup, transient NovelAI error) may be
    // retried, but rate-limited so remounting components cannot hammer the
    // endpoint while it is down.
    if (this.attempted && Date.now() - this.lastFailureAt < RETRY_AFTER_FAILURE_MS) return;
    await this.refreshSubscription();
  }

  /**
   * Fetch the account record. Silently does nothing without a key, because the
   * backend can only answer with an error and there is nothing to report.
   */
  async refreshSubscription(): Promise<void> {
    if (!this.apiKeyConfigured || this.subscriptionLoading) return;
    this.attempted = true;
    this.subscriptionLoading = true;
    this.subscriptionError = null;
    try {
      this.subscription = await novelaiSubscription();
      this.lastFetchFailed = false;
    } catch (e) {
      this.subscription = null;
      this.subscriptionError = e instanceof Error ? e.message : String(e);
      this.lastFetchFailed = true;
      this.lastFailureAt = Date.now();
      // Into the ring buffer, so an exported log shows why the Anlas
      // readout is blank instead of nothing at all.
      console.error("NovelAI subscription fetch failed:", e);
    } finally {
      this.subscriptionLoading = false;
    }
  }
}

export const novelai = new NovelAiStore();
