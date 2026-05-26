import type { LoraEntry } from "../types/index.js";
import { triggerSync } from "../utils/syncTrigger.js";

const STORAGE_KEY = "mooshieui.loraPresets.v1";
const EXPORT_VERSION = 1;

export type LoraPresetApplyMode = "replace" | "merge";

export interface LoraPreset {
  id: string;
  name: string;
  loras: LoraEntry[];
  createdAt: number;
  updatedAt: number;
}

interface PersistedState {
  version: number;
  presets: LoraPreset[];
}

function genId(): string {
  return `lps_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

function sanitizeLora(raw: any): LoraEntry | null {
  if (!raw || typeof raw.name !== "string") return null;
  const name = raw.name.trim();
  if (!name) return null;
  const strengthModel = typeof raw.strength_model === "number" ? raw.strength_model : 1.0;
  const strengthClip = typeof raw.strength_clip === "number" ? raw.strength_clip : 1.0;
  return {
    name,
    strength_model: Number.isFinite(strengthModel) ? strengthModel : 1.0,
    strength_clip: Number.isFinite(strengthClip) ? strengthClip : 1.0,
    enabled: raw.enabled !== false,
  };
}

function sanitizePreset(raw: any): LoraPreset | null {
  if (!raw || typeof raw.id !== "string" || typeof raw.name !== "string") return null;
  const now = Date.now();
  const loras = Array.isArray(raw.loras)
    ? (raw.loras.map(sanitizeLora).filter(Boolean) as LoraEntry[])
    : [];
  return {
    id: raw.id,
    name: raw.name.trim() || "Untitled LoRA preset",
    loras,
    createdAt: typeof raw.createdAt === "number" && raw.createdAt > 0 ? raw.createdAt : now,
    updatedAt: typeof raw.updatedAt === "number" && raw.updatedAt > 0 ? raw.updatedAt : now,
  };
}

function normalizeName(name: string): string {
  return name.trim().toLowerCase();
}

function cloneLora(lora: LoraEntry): LoraEntry {
  return {
    name: lora.name,
    strength_model: lora.strength_model,
    strength_clip: lora.strength_clip,
    enabled: lora.enabled,
  };
}

class LoraPresetsStore {
  presets = $state<LoraPreset[]>([]);

  constructor() {
    this.loadSettings();
  }

  private loadSettings() {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as Partial<PersistedState>;
      if (!Array.isArray(parsed.presets)) return;
      this.presets = parsed.presets.map(sanitizePreset).filter(Boolean) as LoraPreset[];
    } catch (e) {
      console.error("lora-presets: load failed", e);
    }
  }

  private saveSettings() {
    try {
      const payload: PersistedState = { version: EXPORT_VERSION, presets: this.presets };
      localStorage.setItem(STORAGE_KEY, JSON.stringify(payload));
      triggerSync();
    } catch (e) {
      console.error("lora-presets: save failed", e);
    }
  }

  getById(id: string): LoraPreset | null {
    return this.presets.find((preset) => preset.id === id) ?? null;
  }

  create(name: string, loras: LoraEntry[]): LoraPreset {
    const now = Date.now();
    const preset: LoraPreset = {
      id: genId(),
      name: name.trim() || "Untitled LoRA preset",
      loras: loras.map(cloneLora),
      createdAt: now,
      updatedAt: now,
    };
    this.presets = [preset, ...this.presets];
    this.saveSettings();
    return preset;
  }

  update(id: string, patch: Partial<Omit<LoraPreset, "id" | "createdAt">>): void {
    let changed = false;
    this.presets = this.presets.map((preset) => {
      if (preset.id !== id) return preset;
      changed = true;
      return {
        ...preset,
        name: typeof patch.name === "string" ? patch.name.trim() || preset.name : preset.name,
        loras: Array.isArray(patch.loras) ? patch.loras.map(cloneLora) : preset.loras,
        updatedAt: Date.now(),
      };
    });
    if (changed) this.saveSettings();
  }

  remove(id: string): void {
    if (!this.getById(id)) return;
    this.presets = this.presets.filter((preset) => preset.id !== id);
    this.saveSettings();
  }

  applyToLoras(
    currentLoras: LoraEntry[],
    presetId: string,
    mode: LoraPresetApplyMode,
    availableLoraNames: string[],
  ): { loras: LoraEntry[]; missingNames: string[] } | null {
    const preset = this.getById(presetId);
    if (!preset) return null;

    const available = new Set(availableLoraNames.map(normalizeName));
    const missingNames = preset.loras
      .filter((lora) => !available.has(normalizeName(lora.name)))
      .map((lora) => lora.name);

    if (mode === "replace") {
      return {
        loras: preset.loras.map(cloneLora),
        missingNames,
      };
    }

    const merged = currentLoras.map(cloneLora);
    const byName = new Map(merged.map((lora, index) => [normalizeName(lora.name), index]));
    for (const lora of preset.loras) {
      const key = normalizeName(lora.name);
      const existingIndex = byName.get(key);
      if (existingIndex === undefined) {
        byName.set(key, merged.length);
        merged.push(cloneLora(lora));
      } else {
        merged[existingIndex] = cloneLora(lora);
      }
    }
    return {
      loras: merged,
      missingNames,
    };
  }

  collectPrefs(): unknown {
    return { presets: this.presets };
  }

  applyServerPrefs(data: any): void {
    try {
      if (!Array.isArray(data?.presets)) return;
      const sanitized = data.presets.map(sanitizePreset).filter(Boolean) as LoraPreset[];
      this.presets = sanitized;
      localStorage.setItem(STORAGE_KEY, JSON.stringify({ version: EXPORT_VERSION, presets: sanitized }));
    } catch (e) {
      console.error("lora-presets: applyServerPrefs failed", e);
    }
  }
}

export const loraPresets = new LoraPresetsStore();
