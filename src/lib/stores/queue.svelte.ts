import { reorderQueueItem, deleteQueueItem, interruptGeneration, clearAllQueues } from "../utils/api.js";
import { progress } from "./progress.svelte.js";
import { locale } from "./locale.svelte.js";
import type { QueuedPrompt } from "./progress.svelte.js";

export interface QueuePanelRow {
  promptId: string;
  /** 0-based index among the user's pending items (0 = running if active). */
  userPosition: number;
  summary: string;
  modelName: string;
  dimensions: string;
  batchLabel: string;
  /** Elapsed seconds; only set for the running item. */
  elapsedSecs?: number;
  running: boolean;
}

function summarise(params: QueuedPrompt["params"]): string {
  const text = params.positive_prompt;
  if (!text) return "(no prompt)";
  return text.length > 60 ? text.slice(0, 57) + "..." : text;
}

function modelLabel(params: QueuedPrompt["params"]): string {
  const raw = params.checkpoint;
  if (!raw) return "";
  const parts = raw.split(/[\\/]/);
  const filename = parts[parts.length - 1] ?? raw;
  // Strip common extensions.
  return filename.replace(/\.(safetensors|ckpt|pt|pth|bin)$/i, "");
}

function dimensionsLabel(params: QueuedPrompt["params"]): string {
  const { width, height } = params;
  if (typeof width === "number" && typeof height === "number") {
    return `${width}x${height}`;
  }
  return "";
}

function batchLabel(params: QueuedPrompt["params"]): string {
  const n = params.batch_size;
  if (typeof n === "number" && n > 1) return `x${n}`;
  return "";
}

class QueueStore {
  panelOpen = $state(false);
  errorMsg = $state<string | null>(null);

  togglePanel() {
    this.panelOpen = !this.panelOpen;
    if (this.panelOpen) {
      this.errorMsg = null;
    }
  }

  closePanel() {
    this.panelOpen = false;
  }

  get rows(): QueuePanelRow[] {
    const activeId = progress.activePromptId;
    let userPos = 0;
    return progress.pendingPrompts.map((p) => {
      const running = p.promptId === activeId;
      const elapsedSecs = running && p.startedAt != null
        ? Math.floor((Date.now() - p.startedAt) / 1000)
        : undefined;
      const row: QueuePanelRow = {
        promptId: p.promptId,
        userPosition: userPos,
        summary: summarise(p.params),
        modelName: modelLabel(p.params),
        dimensions: dimensionsLabel(p.params),
        batchLabel: batchLabel(p.params),
        elapsedSecs,
        running,
      };
      userPos++;
      return row;
    });
  }

  get pendingRows(): QueuePanelRow[] {
    return this.rows.filter((r) => !r.running);
  }

  get runningRow(): QueuePanelRow | undefined {
    return this.rows.find((r) => r.running);
  }

  async reorder(promptId: string, newPosition: number) {
    this.errorMsg = null;
    try {
      await reorderQueueItem(promptId, newPosition);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      if (msg.includes("already_running")) {
        this.errorMsg = locale.t("queue.panel.already_running");
      } else {
        this.errorMsg = locale.t("queue.panel.reorder_failed", { error: msg });
      }
    }
  }

  async cancel(promptId: string) {
    this.errorMsg = null;
    try {
      await deleteQueueItem(promptId);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      this.errorMsg = locale.t("queue.panel.cancel_failed", { error: msg });
    }
  }

  async interrupt() {
    this.errorMsg = null;
    try {
      const running = this.runningRow;
      if (running) {
        await interruptGeneration(running.promptId);
      } else {
        await interruptGeneration();
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      this.errorMsg = locale.t("queue.panel.cancel_failed", { error: msg });
    }
  }

  async clearPending() {
    this.errorMsg = null;
    const pending = this.pendingRows;
    for (const row of pending) {
      try {
        await deleteQueueItem(row.promptId);
      } catch {
        // best-effort; continue clearing the rest
      }
    }
  }
}

export const queue = new QueueStore();
