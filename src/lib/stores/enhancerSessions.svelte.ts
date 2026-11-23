import { userScopedKey } from "../utils/ipc.js";
import { triggerSync } from "../utils/syncTrigger.js";
import {
  appendTurn,
  emptySessions,
  hasAnyTurns,
  parseStoredSessions,
  parseStoredTabSessions,
  pruneTabSessions,
  type EnhancerFlow,
  type SessionTurn,
  type TabSessions,
} from "../utils/enhancerSession.js";

const STORAGE_KEY = "mooshieui.enhancerSessions.v1";
const PARKED_STORAGE_KEY = "mooshieui.enhancerTabSessions.v1";

function emptyDropped(): Record<EnhancerFlow, number> {
  return { nai: 0, enhance: 0, h3: 0, compose: 0 };
}

/**
 * One persistent conversation per enhancer flow, scoped to each generation
 * tab. Survives closing the modal, switching backend, switching tabs and
 * restarting; only `clear` empties a session.
 *
 * Stored like prompt history: `localStorage` under a per-user key, loaded once
 * at construction. Depends on no other store, so any flow can use it without
 * creating an import cycle.
 */
class EnhancerSessionsStore {
  sessions = $state<TabSessions>(emptySessions());
  /** How many saved turns the latest request had to leave out, per flow. */
  dropped = $state<Record<EnhancerFlow, number>>(emptyDropped());

  private activeTabId: string | null = null;
  private openTabIds = new Set<string>();
  private parked: Record<string, TabSessions> = {};
  private parkedDropped = new Map<string, Record<EnhancerFlow, number>>();

  constructor() {
    try {
      this.sessions = parseStoredSessions(localStorage.getItem(userScopedKey(STORAGE_KEY)));
      this.parked = parseStoredTabSessions(localStorage.getItem(userScopedKey(PARKED_STORAGE_KEY)));
    } catch (e) {
      console.warn("[enhancerSessions] could not load saved sessions", e);
    }
  }

  private isParkedTarget(tabId?: string): tabId is string {
    return !!tabId && !!this.activeTabId && tabId !== this.activeTabId;
  }

  /**
   * Follow the generation tabs: park the outgoing tab's sessions, restore the
   * incoming tab's, and forget tabs that were closed.
   */
  syncTabs(activeId: string, openIds: string[]): void {
    this.openTabIds = new Set(openIds);
    if (this.activeTabId !== null && this.activeTabId !== activeId) {
      if (hasAnyTurns(this.sessions)) this.parked[this.activeTabId] = this.sessions;
      else delete this.parked[this.activeTabId];
      this.parkedDropped.set(this.activeTabId, this.dropped);

      this.sessions = this.parked[activeId] ?? emptySessions();
      this.dropped = this.parkedDropped.get(activeId) ?? emptyDropped();
      delete this.parked[activeId];
      this.parkedDropped.delete(activeId);
      this.activeTabId = activeId;

      this.parked = pruneTabSessions(this.parked, openIds, activeId);
      for (const id of this.parkedDropped.keys()) {
        if (!this.openTabIds.has(id) || id === activeId) this.parkedDropped.delete(id);
      }
      this.save();
      this.writeParked();
      return;
    }

    this.activeTabId = activeId;
    const beforeCount = Object.keys(this.parked).length;
    this.parked = pruneTabSessions(this.parked, openIds, activeId);
    for (const id of this.parkedDropped.keys()) {
      if (!this.openTabIds.has(id) || id === activeId) this.parkedDropped.delete(id);
    }
    if (Object.keys(this.parked).length !== beforeCount) this.writeParked();
  }

  turns(flow: EnhancerFlow, tabId?: string): SessionTurn[] {
    if (this.isParkedTarget(tabId)) return this.parked[tabId]?.[flow] ?? [];
    return this.sessions[flow];
  }

  count(flow: EnhancerFlow, tabId?: string): number {
    return this.turns(flow, tabId).length;
  }

  noteDropped(flow: EnhancerFlow, n: number, tabId?: string): void {
    if (this.isParkedTarget(tabId)) {
      if (!this.openTabIds.has(tabId)) return;
      const current = this.parkedDropped.get(tabId) ?? emptyDropped();
      if (current[flow] !== n) this.parkedDropped.set(tabId, { ...current, [flow]: n });
      return;
    }
    if (this.dropped[flow] !== n) this.dropped = { ...this.dropped, [flow]: n };
  }

  /**
   * Record one completed exchange and return its timestamp, which is unique
   * within the session. Null when a side is empty and nothing was recorded.
   * Callers skip failed and cancelled requests.
   */
  record(flow: EnhancerFlow, user: string, assistant: string, tabId?: string): number | null {
    if (!user.trim() || !assistant.trim()) return null;
    if (this.isParkedTarget(tabId)) {
      if (!this.openTabIds.has(tabId)) return null;
      const sessions = this.parked[tabId] ?? emptySessions();
      const list = sessions[flow];
      const last = list[list.length - 1];
      const at = Math.max(Date.now(), (last?.at ?? 0) + 1);
      this.parked[tabId] = { ...sessions, [flow]: appendTurn(list, { user, assistant, at }) };
      this.writeParked();
      return at;
    }
    const last = this.sessions[flow][this.sessions[flow].length - 1];
    const at = Math.max(Date.now(), (last?.at ?? 0) + 1);
    this.sessions = {
      ...this.sessions,
      [flow]: appendTurn(this.sessions[flow], { user, assistant, at }),
    };
    this.save();
    return at;
  }

  /** Take back the newest exchange, for a rewrite that is being redone. */
  dropLast(flow: EnhancerFlow): void {
    if (this.sessions[flow].length === 0) return;
    this.sessions = { ...this.sessions, [flow]: this.sessions[flow].slice(0, -1) };
    this.save();
  }

  clear(flow: EnhancerFlow): void {
    this.sessions = { ...this.sessions, [flow]: [] };
    this.dropped = { ...this.dropped, [flow]: 0 };
    this.save();
  }

  collectPrefs(): unknown {
    return this.sessions;
  }

  applyServerPrefs(data: unknown): void {
    this.sessions = parseStoredSessions(JSON.stringify(data ?? null));
    this.writeLocal();
  }

  private save(): void {
    this.writeLocal();
    triggerSync();
  }

  private writeLocal(): void {
    try {
      localStorage.setItem(userScopedKey(STORAGE_KEY), JSON.stringify(this.sessions));
    } catch (e) {
      // Quota or private-mode failures: the in-memory session keeps working.
      console.error("[enhancerSessions] could not save sessions", e);
    }
  }

  private writeParked(): void {
    try {
      localStorage.setItem(userScopedKey(PARKED_STORAGE_KEY), JSON.stringify(this.parked));
    } catch (e) {
      console.error("[enhancerSessions] could not save parked tab sessions", e);
    }
  }
}

export const enhancerSessions = new EnhancerSessionsStore();
