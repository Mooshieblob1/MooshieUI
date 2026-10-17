import { triggerSync } from "../utils/syncTrigger.js";
import { userScopedKey } from "../utils/ipc.js";
import { locale } from "./locale.svelte.js";
import { generation } from "./generation.svelte.js";

const STORAGE_KEY = "mooshieui.profiles.v1";
const EXPORT_VERSION = 1;

/** A named snapshot of every generation setting (model, LoRAs, prompts, sampler, size...). */
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
  if (!raw.settings || typeof raw.settings !== "object") return null;
  const now = Date.now();
  return {
    id: raw.id,
    name: raw.name.trim() || locale.t("profiles.untitled"),
    settings: raw.settings,
    createdAt: typeof raw.createdAt === "number" && raw.createdAt > 0 ? raw.createdAt : now,
    updatedAt: typeof raw.updatedAt === "number" && raw.updatedAt > 0 ? raw.updatedAt : now,
  };
}

function snapshot(): Record<string, unknown> {
  return JSON.parse(JSON.stringify(generation.collectPrefs()));
}

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
      this.profiles = parsed.profiles.map(sanitizeProfile).filter(Boolean) as Profile[];
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

  /** Save the current settings under `name`, overwriting a profile with the same name. */
  saveCurrent(name: string): Profile {
    const now = Date.now();
    const existing = this.findByName(name);
    if (existing) {
      const updated: Profile = { ...existing, settings: snapshot(), updatedAt: now };
      this.profiles = this.profiles.map((p) => (p.id === existing.id ? updated : p));
      this.saveSettings();
      return updated;
    }
    const profile: Profile = {
      id: genId(),
      name: name.trim() || locale.t("profiles.untitled"),
      settings: snapshot(),
      createdAt: now,
      updatedAt: now,
    };
    this.profiles = [...this.profiles, profile].sort((a, b) => a.name.localeCompare(b.name));
    this.saveSettings();
    return profile;
  }

  /** Replace every generation setting with the profile's. */
  async load(id: string): Promise<boolean> {
    const profile = this.getById(id);
    if (!profile) return false;
    await generation.applyServerPrefs(JSON.parse(JSON.stringify(profile.settings)));
    triggerSync();
    return true;
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
      const sanitized = data.profiles.map(sanitizeProfile).filter(Boolean) as Profile[];
      this.profiles = sanitized;
      localStorage.setItem(userScopedKey(STORAGE_KEY), JSON.stringify({ version: EXPORT_VERSION, profiles: sanitized }));
    } catch (e) {
      console.error("profiles: applyServerPrefs failed", e);
    }
  }
}

export const profiles = new ProfilesStore();
