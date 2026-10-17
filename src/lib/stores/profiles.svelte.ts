import { triggerSync } from "../utils/syncTrigger.js";
import { userScopedKey } from "../utils/ipc.js";

const STORAGE_KEY = "mooshieui.profiles.v1";
const EXPORT_VERSION = 1;

/**
 * A named, saved set of generation settings. `settings` is the same snapshot a
 * generation tab parks (app-wide preferences left out), so a profile opens as
 * a tab and loading one never touches output format, save folders and the like.
 */
export interface Profile {
  id: string;
  name: string;
  settings: Record<string, unknown>;
  createdAt: number;
  updatedAt: number;
}

interface PersistedState {
  version: number;
  profiles: Profile[];
}

function genId(): string {
  return `prf_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

function sanitizeProfile(raw: any): Profile | null {
  if (!raw || typeof raw.id !== "string" || typeof raw.name !== "string") return null;
  const name = raw.name.trim();
  if (!name || !raw.settings || typeof raw.settings !== "object" || Array.isArray(raw.settings)) return null;
  const now = Date.now();
  return {
    id: raw.id,
    name,
    settings: raw.settings,
    createdAt: typeof raw.createdAt === "number" && raw.createdAt > 0 ? raw.createdAt : now,
    updatedAt: typeof raw.updatedAt === "number" && raw.updatedAt > 0 ? raw.updatedAt : now,
  };
}

function byName(a: Profile, b: Profile): number {
  return a.name.localeCompare(b.name);
}

/**
 * Saved profiles, synced per user like LoRA presets. Plain data only: the tab
 * strip captures and applies the snapshots through the generation tabs store.
 */
class ProfilesStore {
  profiles = $state<Profile[]>([]);

  constructor() {
    this.loadSettings();
  }

  private loadSettings() {
    try {
      const raw = localStorage.getItem(userScopedKey(STORAGE_KEY));
      if (!raw) return;
      const parsed = JSON.parse(raw) as Partial<PersistedState>;
      if (!Array.isArray(parsed.profiles)) return;
      this.profiles = (parsed.profiles.map(sanitizeProfile).filter(Boolean) as Profile[]).sort(byName);
    } catch (e) {
      console.error("profiles: load failed", e);
    }
  }

  private saveSettings() {
    try {
      const payload: PersistedState = { version: EXPORT_VERSION, profiles: this.profiles };
      localStorage.setItem(userScopedKey(STORAGE_KEY), JSON.stringify(payload));
      triggerSync();
    } catch (e) {
      console.error("profiles: save failed", e);
    }
  }

  getById(id: string): Profile | null {
    return this.profiles.find((p) => p.id === id) ?? null;
  }

  findByName(name: string): Profile | null {
    const key = name.trim().toLowerCase();
    return this.profiles.find((p) => p.name.toLowerCase() === key) ?? null;
  }

  /** Save `settings` under `name`, replacing a profile that already has that name. */
  save(name: string, settings: Record<string, unknown>): { profile: Profile; updated: boolean } {
    const trimmed = name.trim();
    const now = Date.now();
    const existing = this.findByName(trimmed);
    const profile: Profile = existing
      ? { ...existing, name: trimmed, settings, updatedAt: now }
      : { id: genId(), name: trimmed, settings, createdAt: now, updatedAt: now };
    const rest = this.profiles.filter((p) => p.id !== profile.id);
    this.profiles = [...rest, profile].sort(byName);
    this.saveSettings();
    return { profile, updated: !!existing };
  }

  remove(id: string): void {
    if (!this.getById(id)) return;
    this.profiles = this.profiles.filter((p) => p.id !== id);
    this.saveSettings();
  }

  collectPrefs(): unknown {
    return { profiles: this.profiles };
  }

  applyServerPrefs(data: any): void {
    try {
      if (!Array.isArray(data?.profiles)) return;
      const sanitized = (data.profiles.map(sanitizeProfile).filter(Boolean) as Profile[]).sort(byName);
      this.profiles = sanitized;
      localStorage.setItem(userScopedKey(STORAGE_KEY), JSON.stringify({ version: EXPORT_VERSION, profiles: sanitized }));
    } catch (e) {
      console.error("profiles: applyServerPrefs failed", e);
    }
  }
}

export const profiles = new ProfilesStore();
