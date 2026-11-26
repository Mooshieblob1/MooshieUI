import type { NovelAiSubscription } from "../types/index.js";
import {
  deleteNovelaiKey,
  getCachedConfig,
  getConfig,
  listNovelaiKeys,
  novelaiSubscription,
  saveNovelaiKey,
  setNovelaiApiKey,
  useNovelaiKey,
  type NovelAiSavedKey,
} from "../utils/api.js";

/** How long a failed subscription fetch suppresses automatic retries. */
const RETRY_AFTER_FAILURE_MS = 30_000;

/**
 * Background retries after a failed fetch: 30 s, then doubling, at most
 * 5 minutes apart and 5 tries in a row. NovelAI's Cloudflare front turns
 * requests away now and then (a 403 block page), and that clears on its own.
 */
const BACKGROUND_RETRY_BASE_MS = 30_000;
const BACKGROUND_RETRY_MAX_MS = 5 * 60_000;
const BACKGROUND_RETRY_LIMIT = 5;

/** Local per-device opt-in for experimental multi-character Precise Reference isolation. */
const EXPERIMENTAL_MULTI_CHAR_PR_KEY = "mooshieui.experimental.nai_multi_char_pr";

/** Failures that retrying cannot fix: a bad key or an empty balance. */
function isPermanentFailure(message: string): boolean {
  return /^API error \((401|402)\)/.test(message);
}

/** The backend's "API error (403): ..." prefix is noise on screen. */
function shortError(message: string): string {
  return message.replace(/^API error \(\d+\):\s*/, "");
}

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

  /**
   * Whether the experimental multi-character Precise Reference isolation UI is
   * shown. Off by default; toggled in Settings, Appearance.
   */
  experimentalMultiCharPr = $state(false);

  /** Session-only bounded cache of background-cutout base64 PNGs keyed by source image base64. */
  sheetCutouts = $state<Record<string, string>>({});
  private sheetCutoutKeys: string[] = [];

  getSheetCutout(src: string): string | undefined {
    return this.sheetCutouts[src];
  }

  setSheetCutout(src: string, cutout: string): void {
    const next = { ...this.sheetCutouts, [src]: cutout };
    if (!this.sheetCutoutKeys.includes(src)) {
      this.sheetCutoutKeys.push(src);
      while (this.sheetCutoutKeys.length > 24) {
        const evicted = this.sheetCutoutKeys.shift();
        if (evicted) delete next[evicted];
      }
    }
    this.sheetCutouts = next;
  }

  /** Load experimental NovelAI feature toggles from localStorage. */
  loadExperimentalFlags() {
    try {
      this.experimentalMultiCharPr =
        localStorage.getItem(EXPERIMENTAL_MULTI_CHAR_PR_KEY) === "1";
    } catch {
      this.experimentalMultiCharPr = false;
    }
  }

  /** Enable or disable the experimental multi-character Precise Reference toggle. */
  setExperimentalMultiCharPr(value: boolean) {
    this.experimentalMultiCharPr = value;
    try {
      if (value) localStorage.setItem(EXPERIMENTAL_MULTI_CHAR_PR_KEY, "1");
      else localStorage.removeItem(EXPERIMENTAL_MULTI_CHAR_PR_KEY);
    } catch {
      /* private mode */
    }
  }

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
  private consecutiveFailures = 0;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;

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

  /**
   * When the subscription is paid through: NovelAI's `expiresAt`, which is the
   * next renewal for a renewing plan and the end date for a cancelled one.
   * Null without an active subscription.
   */
  get paidThrough(): Date | null {
    const sub = this.subscription;
    if (!sub?.active || !sub.expiresAt) return null;
    const date = new Date(sub.expiresAt * 1000);
    return Number.isNaN(date.getTime()) ? null : date;
  }

  /** Whole days until `paidThrough`, never below 0. */
  get daysPaidThrough(): number | null {
    const date = this.paidThrough;
    if (!date) return null;
    return Math.max(0, Math.ceil((date.getTime() - Date.now()) / 86_400_000));
  }

  /**
   * The owner's saved keys for switching accounts. Empty for named accounts,
   * which the backend refuses: they keep one key each.
   */
  savedKeys = $state<NovelAiSavedKey[]>([]);
  /** The backend answered the list, so this account may keep saved keys. */
  savedKeysAvailable = $state(false);

  async loadSavedKeys(): Promise<void> {
    try {
      this.savedKeys = await listNovelaiKeys();
      this.savedKeysAvailable = true;
    } catch {
      this.savedKeys = [];
      this.savedKeysAvailable = false;
    }
  }

  private savedKeysRequested = false;

  /** Load the saved keys once, for the quick switcher above Generate. */
  ensureSavedKeys(): void {
    if (this.savedKeysRequested) return;
    this.savedKeysRequested = true;
    void this.loadSavedKeys();
  }

  async saveNamedKey(label: string, key: string): Promise<void> {
    this.savedKeys = await saveNovelaiKey(label, key);
  }

  async deleteNamedKey(label: string): Promise<void> {
    this.savedKeys = await deleteNovelaiKey(label);
  }

  /** Switch to a saved key; the account record is fetched again for it. */
  async useNamedKey(label: string): Promise<void> {
    this.savedKeys = await useNovelaiKey(label);
    this.apiKeyConfigured = true;
    await this.afterKeyChange();
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
    await this.afterKeyChange();
    // The saved-key list marks which one is in use, and that just changed.
    if (this.savedKeys.length > 0) void this.loadSavedKeys();
  }

  private async afterKeyChange(): Promise<void> {
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
    this.cancelRetry();
    this.consecutiveFailures = 0;
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
    this.cancelRetry();
    this.attempted = true;
    this.subscriptionLoading = true;
    try {
      this.subscription = await novelaiSubscription();
      this.subscriptionError = null;
      this.lastFetchFailed = false;
      this.consecutiveFailures = 0;
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      // The last good record stays: a balance a few minutes old beats a
      // blank one, and the Opus discount in the cost estimate depends on it.
      this.subscriptionError = shortError(message);
      this.lastFetchFailed = true;
      this.lastFailureAt = Date.now();
      this.consecutiveFailures += 1;
      // Into the ring buffer, so an exported log shows why the Anlas
      // readout is stale or blank.
      console.error("NovelAI subscription fetch failed:", e);
      if (!isPermanentFailure(message)) this.scheduleRetry();
    } finally {
      this.subscriptionLoading = false;
    }
  }

  private scheduleRetry() {
    if (this.consecutiveFailures > BACKGROUND_RETRY_LIMIT) return;
    const delay = Math.min(
      BACKGROUND_RETRY_BASE_MS * 2 ** (this.consecutiveFailures - 1),
      BACKGROUND_RETRY_MAX_MS,
    );
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      void this.refreshSubscription();
    }, delay);
  }

  private cancelRetry() {
    if (this.retryTimer) clearTimeout(this.retryTimer);
    this.retryTimer = null;
  }
}

export const novelai = new NovelAiStore();
novelai.loadExperimentalFlags();
