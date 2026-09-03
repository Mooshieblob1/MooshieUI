import { civitaiBulkScan, civitaiBulkScanCancel } from "../utils/api.js";
import type { CivitaiBulkScanSummary } from "../utils/api.js";
import { ipcListen } from "../utils/ipc.js";

export interface CivitaiScanProgress {
  current: number;
  total: number;
  name: string;
  /** "hashing" | "found" | "not_found" | "skipped" | "error" | "done" | "cancelled" */
  status: string;
  done: boolean;
  // summary counts (only on the final done event)
  found?: number;
  not_found?: number;
  skipped?: number;
  errors?: number;
  cancelled?: boolean;
}

class CivitaiScanStore {
  running = $state(false);
  cancelling = $state(false);
  progress = $state<CivitaiScanProgress | null>(null);
  summary = $state<CivitaiBulkScanSummary | null>(null);
  error = $state<string | null>(null);
  #listening = false;

  /** Fraction 0-1 for a progress bar. Returns 0 when total is unknown. */
  get fraction(): number {
    if (!this.progress || !this.progress.total) return 0;
    return this.progress.current / this.progress.total;
  }

  /** Subscribe once to backend events, however many components mount. */
  listen(): void {
    if (this.#listening) return;
    this.#listening = true;
    void ipcListen("comfyui:civitai_scan", (event: { payload: unknown }) => {
      const data = event.payload as CivitaiScanProgress;
      this.progress = data;
      if (data.done) {
        this.running = false;
        this.cancelling = false;
        if (data.found !== undefined) {
          this.summary = {
            total: data.total,
            found: data.found,
            not_found: data.not_found ?? 0,
            skipped: data.skipped ?? 0,
            errors: data.errors ?? 0,
            cancelled: data.cancelled ?? false,
          };
        }
      }
    });
  }

  /** Launch the bulk scan. `force=true` re-hashes models that already have a sidecar. */
  async scan(force = false): Promise<void> {
    if (this.running) return;
    this.running = true;
    this.cancelling = false;
    this.progress = null;
    this.summary = null;
    this.error = null;
    this.listen();
    try {
      const result = await civitaiBulkScan(force);
      // Desktop mode returns a summary directly; browser mode returns null
      // (progress comes via SSE and the done handler above sets summary).
      if (result !== null) {
        this.summary = result;
        this.running = false;
        this.cancelling = false;
      }
    } catch (e) {
      this.error = String(e);
      this.running = false;
      this.cancelling = false;
    }
  }

  /** Request cancellation of the running scan. */
  async cancel(): Promise<void> {
    if (!this.running || this.cancelling) return;
    this.cancelling = true;
    try {
      await civitaiBulkScanCancel();
    } catch {
      // Backend may have already finished -- safe to ignore.
      this.cancelling = false;
    }
  }

  /** Reset state so the UI returns to the idle/button-only view. */
  dismiss(): void {
    if (this.running) return;
    this.progress = null;
    this.summary = null;
    this.error = null;
  }
}

export const civitaiScan = new CivitaiScanStore();
