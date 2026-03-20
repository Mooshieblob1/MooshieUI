import { listen } from "@tauri-apps/api/event";

export interface DownloadEntry {
  filename: string;
  downloaded: number;
  total: number;
  done: boolean;
  /** Bytes per second */
  speed: number;
}

class DownloadsStore {
  /** Currently active downloads, keyed by filename */
  active = $state<Map<string, DownloadEntry>>(new Map());

  /** Speed tracking: previous downloaded bytes and timestamp per file */
  private speedTracker = new Map<string, { bytes: number; time: number }>();

  /** Whether any download is in progress */
  get hasActive(): boolean {
    return this.active.size > 0;
  }

  /** Start listening for download:progress events from the backend */
  async init() {
    await listen("download:progress", (event: any) => {
      const data = event.payload as {
        filename: string;
        downloaded: number;
        total: number;
        done: boolean;
      };

      if (data.done) {
        this.speedTracker.delete(data.filename);
        const entry: DownloadEntry = { ...data, speed: 0 };
        this.active = new Map(this.active.set(data.filename, entry));
        setTimeout(() => {
          this.active.delete(data.filename);
          this.active = new Map(this.active);
        }, 2000);
      } else {
        const now = performance.now();
        const prev = this.speedTracker.get(data.filename);
        let speed = 0;
        if (prev) {
          const elapsed = (now - prev.time) / 1000;
          if (elapsed > 0.1) {
            speed = (data.downloaded - prev.bytes) / elapsed;
            this.speedTracker.set(data.filename, { bytes: data.downloaded, time: now });
          } else {
            // Too soon, keep previous speed
            speed = this.active.get(data.filename)?.speed ?? 0;
          }
        } else {
          this.speedTracker.set(data.filename, { bytes: data.downloaded, time: now });
        }
        const entry: DownloadEntry = { ...data, speed };
        this.active = new Map(this.active.set(data.filename, entry));
      }
    });
  }
}

export const downloads = new DownloadsStore();
