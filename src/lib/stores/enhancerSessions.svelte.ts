import { userScopedKey } from "../utils/ipc.js";
import {
  appendTurn,
  emptySessions,
  parseStoredSessions,
  type EnhancerFlow,
  type SessionTurn,
} from "../utils/enhancerSession.js";

const STORAGE_KEY = "mooshieui.enhancerSessions.v1";

/**
 * One persistent conversation per enhancer flow. Survives closing the modal,
 * switching backend and restarting; only `clear` empties a session.
 *
 * Stored like prompt history: `localStorage` under a per-user key, loaded once
 * at construction. Depends on no other store, so any flow can use it without
 * creating an import cycle.
 */
class EnhancerSessionsStore {
  sessions = $state<Record<EnhancerFlow, SessionTurn[]>>(emptySessions());
  /** How many saved turns the latest request had to leave out, per flow. */
  dropped = $state<Record<EnhancerFlow, number>>({ nai: 0, enhance: 0, h3: 0, compose: 0 });

  constructor() {
    try {
      this.sessions = parseStoredSessions(localStorage.getItem(userScopedKey(STORAGE_KEY)));
    } catch (e) {
      console.warn("[enhancerSessions] could not load saved sessions", e);
    }
  }

  turns(flow: EnhancerFlow): SessionTurn[] {
    return this.sessions[flow];
  }

  count(flow: EnhancerFlow): number {
    return this.sessions[flow].length;
  }

  noteDropped(flow: EnhancerFlow, n: number): void {
    if (this.dropped[flow] !== n) this.dropped = { ...this.dropped, [flow]: n };
  }

  /** Record one completed exchange. Callers skip failed, cancelled and invalid answers. */
  record(flow: EnhancerFlow, user: string, assistant: string): void {
    if (!user.trim() || !assistant.trim()) return;
    this.sessions = {
      ...this.sessions,
      [flow]: appendTurn(this.sessions[flow], { user, assistant, at: Date.now() }),
    };
    this.save();
  }

  clear(flow: EnhancerFlow): void {
    this.sessions = { ...this.sessions, [flow]: [] };
    this.dropped = { ...this.dropped, [flow]: 0 };
    this.save();
  }

  private save(): void {
    try {
      localStorage.setItem(userScopedKey(STORAGE_KEY), JSON.stringify(this.sessions));
    } catch (e) {
      // Quota or private-mode failures: the in-memory session keeps working.
      console.error("[enhancerSessions] could not save sessions", e);
    }
  }
}

export const enhancerSessions = new EnhancerSessionsStore();
