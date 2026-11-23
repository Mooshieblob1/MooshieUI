import { deleteWildcard, listWildcards, saveWildcard } from "../utils/api.js";
import { userScopedKey } from "../utils/ipc.js";
import {
  expandWildcards,
  parseWildcardFiles,
  seededRandom,
  type WildcardSource,
  type WildcardTable,
} from "../utils/wildcards.js";

const ORDER_KEY = "mooshieui.wildcards.order.v1";

interface PersistedOrder {
  ordered: string[];
  counters: Record<string, number>;
}

/**
 * Wildcard files from `<app data>/wildcards` and which names step in order.
 *
 * A leaf store: the generation hub imports it to expand `__name__` in toParams,
 * so it must not import any feature store.
 */
class WildcardStore {
  files = $state<WildcardSource[]>([]);
  dir = $state("");
  truncated = $state(false);
  loaded = $state(false);
  loading = $state(false);
  error = $state<string | null>(null);
  table = $state.raw<WildcardTable>(new Map());
  /** Names that take the next option each generation instead of a random one. */
  ordered = $state<string[]>([]);
  counters = $state<Record<string, number>>({});
  /** In-order names used by the last built request, advanced once it is sent. */
  private pendingAdvance: string[] = [];
  private loadPromise: Promise<void> | null = null;

  constructor() {
    this.reloadOrder();
  }

  /** Re-read ordered wildcard flags and sequential counters from localStorage. */
  reloadOrder() {
    this.ordered = [];
    this.counters = {};
    try {
      const raw = localStorage.getItem(userScopedKey(ORDER_KEY));
      if (raw) {
        const parsed = JSON.parse(raw) as Partial<PersistedOrder>;
        if (Array.isArray(parsed.ordered)) this.ordered = parsed.ordered.filter((n) => typeof n === "string");
        if (parsed.counters && typeof parsed.counters === "object") {
          this.counters = Object.fromEntries(
            Object.entries(parsed.counters).filter(([, v]) => typeof v === "number" && Number.isFinite(v)),
          );
        }
      }
    } catch {
      /* storage unavailable or corrupt: start empty */
    }
  }

  get names(): string[] {
    return [...this.table.keys()].sort();
  }

  /** Load the folder once; later calls reuse the first load unless forced. */
  load(force = false): Promise<void> {
    if (this.loadPromise && !force) return this.loadPromise;
    this.loading = true;
    this.loadPromise = (async () => {
      try {
        const listing = await listWildcards();
        this.dir = listing.dir;
        this.files = listing.files;
        this.truncated = listing.truncated;
        this.table = parseWildcardFiles(listing.files);
        this.error = null;
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e);
      } finally {
        this.loaded = true;
        this.loading = false;
      }
    })();
    return this.loadPromise;
  }

  async save(path: string, content: string) {
    await saveWildcard(path, content);
    await this.load(true);
  }

  async remove(path: string) {
    await deleteWildcard(path);
    await this.load(true);
  }

  /**
   * Replace `__name__` tokens. Seeded, so a fixed seed repeats its picks.
   * Records the in-order names it used; call `commitAdvance()` once the
   * request is actually sent.
   */
  expand(text: string, seed: number): { text: string; unknown: string[] } {
    if (!text.includes("__")) return { text, unknown: [] };
    const result = expandWildcards(text, this.table, {
      // A different stream from the {a|b} expander, which uses the same seed.
      random: seededRandom((seed ^ 0x5bd1e995) >>> 0),
      ordered: new Set(this.ordered),
      counters: this.counters,
    });
    for (const name of result.orderedUsed) {
      if (!this.pendingAdvance.includes(name)) this.pendingAdvance.push(name);
    }
    return { text: result.text, unknown: result.unknown };
  }

  /** Forget in-order names recorded by an earlier build (call before building). */
  beginBuild() {
    this.pendingAdvance = [];
  }

  /** Step every in-order name used by the request that was just sent. */
  commitAdvance() {
    if (this.pendingAdvance.length === 0) return;
    const next = { ...this.counters };
    for (const name of this.pendingAdvance) next[name] = (next[name] ?? 0) + 1;
    this.counters = next;
    this.pendingAdvance = [];
    this.persist();
  }

  isOrdered(name: string): boolean {
    return this.ordered.includes(name);
  }

  setOrdered(name: string, on: boolean) {
    this.ordered = on ? [...new Set([...this.ordered, name])] : this.ordered.filter((n) => n !== name);
    this.persist();
  }

  /** Position (1-based) of the option the next in-order run will use. */
  nextPosition(name: string): number {
    const count = this.table.get(name)?.length ?? 0;
    if (count === 0) return 0;
    return ((this.counters[name] ?? 0) % count) + 1;
  }

  resetCounter(name: string) {
    const next = { ...this.counters };
    delete next[name];
    this.counters = next;
    this.persist();
  }

  private persist() {
    try {
      const state: PersistedOrder = { ordered: this.ordered, counters: this.counters };
      localStorage.setItem(userScopedKey(ORDER_KEY), JSON.stringify(state));
    } catch {
      /* storage unavailable: order lasts for this session */
    }
  }
}

export const wildcards = new WildcardStore();
