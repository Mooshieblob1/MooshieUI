export type ShortcutScope =
  | "global"
  | "generate"
  | "gallery"
  | "lightbox"
  | "canvas"
  | "prompt"
  | "comic";

export const SHORTCUT_SCOPES: readonly ShortcutScope[] = [
  "global",
  "generate",
  "gallery",
  "lightbox",
  "canvas",
  "prompt",
  "comic",
];

export interface ShortcutDef {
  id: string;
  scope: ShortcutScope;
  labelKey: string;
  defaultCombo: string;
  readOnly?: boolean;
  captureInput?: boolean;
}

export type KeyEventLike = Pick<
  KeyboardEvent,
  "key" | "code" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey"
>;

export interface ShortcutConflict {
  conflictingId: string;
  def: ShortcutDef;
  combo: string;
}

export const DEFAULT_SHORTCUTS: readonly ShortcutDef[] = [
  // Global
  {
    id: "global.cheat_sheet",
    scope: "global",
    labelKey: "shortcuts.action.cheat_sheet",
    defaultCombo: "Shift+?",
  },

  // Generate
  {
    id: "generate.submit",
    scope: "generate",
    labelKey: "shortcuts.action.generate",
    defaultCombo: "Ctrl+Enter",
    captureInput: true,
  },

  // Lightbox
  {
    id: "lightbox.prev",
    scope: "lightbox",
    labelKey: "shortcuts.action.lightbox_prev",
    defaultCombo: "ArrowLeft",
  },
  {
    id: "lightbox.next",
    scope: "lightbox",
    labelKey: "shortcuts.action.lightbox_next",
    defaultCombo: "ArrowRight",
  },
  {
    id: "lightbox.rate_1",
    scope: "lightbox",
    labelKey: "shortcuts.action.lightbox_rate_1",
    defaultCombo: "1",
  },
  {
    id: "lightbox.rate_2",
    scope: "lightbox",
    labelKey: "shortcuts.action.lightbox_rate_2",
    defaultCombo: "2",
  },
  {
    id: "lightbox.rate_3",
    scope: "lightbox",
    labelKey: "shortcuts.action.lightbox_rate_3",
    defaultCombo: "3",
  },
  {
    id: "lightbox.rate_4",
    scope: "lightbox",
    labelKey: "shortcuts.action.lightbox_rate_4",
    defaultCombo: "4",
  },
  {
    id: "lightbox.rate_5",
    scope: "lightbox",
    labelKey: "shortcuts.action.lightbox_rate_5",
    defaultCombo: "5",
  },
  {
    id: "lightbox.rate_clear",
    scope: "lightbox",
    labelKey: "shortcuts.action.lightbox_rate_clear",
    defaultCombo: "0",
  },
  {
    id: "lightbox.favourite",
    scope: "lightbox",
    labelKey: "shortcuts.action.lightbox_favourite",
    defaultCombo: "F",
  },
  {
    id: "lightbox.nsfw",
    scope: "lightbox",
    labelKey: "shortcuts.action.lightbox_nsfw",
    defaultCombo: "N",
  },
  {
    id: "lightbox.close",
    scope: "lightbox",
    labelKey: "shortcuts.action.lightbox_close",
    defaultCombo: "Escape",
    readOnly: true,
  },

  // Gallery
  {
    id: "gallery.select_all",
    scope: "gallery",
    labelKey: "shortcuts.action.gallery_select_all",
    defaultCombo: "Ctrl+A",
  },
  {
    id: "gallery.delete_selected",
    scope: "gallery",
    labelKey: "shortcuts.action.gallery_delete",
    defaultCombo: "Delete",
  },
  {
    id: "gallery.clear_selection",
    scope: "gallery",
    labelKey: "shortcuts.action.gallery_clear",
    defaultCombo: "Escape",
    readOnly: true,
  },
  {
    id: "gallery.toggle_select",
    scope: "gallery",
    labelKey: "shortcuts.action.gallery_toggle_select",
    defaultCombo: "Space",
  },
  {
    id: "gallery.open_lightbox",
    scope: "gallery",
    labelKey: "shortcuts.action.gallery_open_lightbox",
    defaultCombo: "Enter",
  },

  // Canvas
  {
    id: "canvas.tool_brush",
    scope: "canvas",
    labelKey: "canvas.brush",
    defaultCombo: "B",
  },
  {
    id: "canvas.tool_eraser",
    scope: "canvas",
    labelKey: "canvas.eraser",
    defaultCombo: "E",
  },
  {
    id: "canvas.tool_rect",
    scope: "canvas",
    labelKey: "canvas.rectangle",
    defaultCombo: "U",
  },
  {
    id: "canvas.tool_lasso",
    scope: "canvas",
    labelKey: "canvas.lasso",
    defaultCombo: "Q",
  },
  {
    id: "canvas.tool_eyedropper",
    scope: "canvas",
    labelKey: "canvas.eyedropper",
    defaultCombo: "I",
  },
  {
    id: "canvas.tool_move",
    scope: "canvas",
    labelKey: "canvas.move",
    defaultCombo: "V",
  },
  {
    id: "canvas.tool_pan",
    scope: "canvas",
    labelKey: "canvas.pan",
    defaultCombo: "H",
  },
  {
    id: "canvas.swap_colors",
    scope: "canvas",
    labelKey: "shortcuts.action.canvas_swap_colors",
    defaultCombo: "X",
  },
  {
    id: "canvas.reset_colors",
    scope: "canvas",
    labelKey: "shortcuts.action.canvas_reset_colors",
    defaultCombo: "D",
  },
  {
    id: "canvas.brush_smaller",
    scope: "canvas",
    labelKey: "shortcuts.action.canvas_brush_smaller",
    defaultCombo: "[",
  },
  {
    id: "canvas.brush_larger",
    scope: "canvas",
    labelKey: "shortcuts.action.canvas_brush_larger",
    defaultCombo: "]",
  },
  {
    id: "canvas.undo",
    scope: "canvas",
    labelKey: "canvas.undo",
    defaultCombo: "Ctrl+Z",
  },
  {
    id: "canvas.redo",
    scope: "canvas",
    labelKey: "canvas.redo",
    defaultCombo: "Ctrl+Y",
  },
  {
    id: "canvas.clear_layer",
    scope: "canvas",
    labelKey: "shortcuts.action.canvas_clear_layer",
    defaultCombo: "Delete",
  },
  {
    id: "canvas.pan_hold",
    scope: "canvas",
    labelKey: "shortcuts.action.canvas_pan_hold",
    defaultCombo: "Space",
  },

  // Prompt
  {
    id: "prompt.toggle_comment",
    scope: "prompt",
    labelKey: "shortcuts.action.prompt_comment",
    defaultCombo: "Ctrl+/",
    captureInput: true,
  },
  {
    id: "prompt.find",
    scope: "prompt",
    labelKey: "shortcuts.action.prompt_find",
    defaultCombo: "Ctrl+F",
    captureInput: true,
  },
  {
    id: "prompt.replace",
    scope: "prompt",
    labelKey: "shortcuts.action.prompt_replace",
    defaultCombo: "Ctrl+H",
    captureInput: true,
  },
  {
    id: "prompt.weight_up",
    scope: "prompt",
    labelKey: "shortcuts.action.prompt_weight_up",
    defaultCombo: "Ctrl+ArrowUp",
    captureInput: true,
  },
  {
    id: "prompt.weight_down",
    scope: "prompt",
    labelKey: "shortcuts.action.prompt_weight_down",
    defaultCombo: "Ctrl+ArrowDown",
    captureInput: true,
  },
  {
    id: "prompt.wrap_braces",
    scope: "prompt",
    labelKey: "shortcuts.action.prompt_wrap_braces",
    defaultCombo: "{}",
    readOnly: true,
    captureInput: true,
  },
  {
    id: "prompt.wrap_brackets",
    scope: "prompt",
    labelKey: "shortcuts.action.prompt_wrap_brackets",
    defaultCombo: "[]",
    readOnly: true,
    captureInput: true,
  },
  {
    id: "prompt.accept_autocomplete",
    scope: "prompt",
    labelKey: "shortcuts.action.prompt_accept_autocomplete",
    defaultCombo: "Tab",
    readOnly: true,
    captureInput: true,
  },

  // Comic
  {
    id: "comic.undo",
    scope: "comic",
    labelKey: "shortcuts.action.comic_undo",
    defaultCombo: "Ctrl+Z",
  },
];

export const DEFAULT_SHORTCUT_MAP: ReadonlyMap<string, ShortcutDef> = new Map(
  DEFAULT_SHORTCUTS.map((def) => [def.id, def]),
);

const IGNORED_MODIFIER_KEYS = new Set([
  "Control",
  "Shift",
  "Alt",
  "Meta",
  "OS",
  "AltGraph",
  "CapsLock",
  "NumLock",
  "ScrollLock",
  "Dead",
  "Process",
  "Unidentified",
]);

const KEY_ALIASES: Record<string, string> = {
  " ": "Space",
  space: "Space",
  spacebar: "Space",
  esc: "Escape",
  escape: "Escape",
  del: "Delete",
  delete: "Delete",
  enter: "Enter",
  return: "Enter",
  tab: "Tab",
  backspace: "Backspace",
  arrowleft: "ArrowLeft",
  left: "ArrowLeft",
  arrowright: "ArrowRight",
  right: "ArrowRight",
  arrowup: "ArrowUp",
  up: "ArrowUp",
  arrowdown: "ArrowDown",
  down: "ArrowDown",
  home: "Home",
  end: "End",
  pageup: "PageUp",
  pagedown: "PageDown",
  insert: "Insert",
};

function normalizeKeyToken(rawKey: string, code?: string): string | null {
  if (!rawKey) return null;
  if (IGNORED_MODIFIER_KEYS.has(rawKey)) return null;
  if (rawKey === " " || code === "Space") return "Space";
  const lower = rawKey.toLowerCase();
  if (lower in KEY_ALIASES) return KEY_ALIASES[lower];
  if (/^[a-z]$/i.test(rawKey)) return rawKey.toUpperCase();
  if (/^f([1-9]|1[0-9]|2[0-4])$/i.test(rawKey)) return rawKey.toUpperCase();
  return rawKey;
}

/**
 * Convert a KeyboardEvent (or partial key-event object) into a canonical combo
 * string like `"Ctrl+Enter"`, `"Shift+?"`, `"B"`, or `"Space"`.
 * Returns `null` when only modifier keys are pressed.
 */
export function normalizeCombo(e: KeyEventLike): string | null {
  const key = normalizeKeyToken(e.key, e.code);
  if (!key) return null;
  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  parts.push(key);
  return parts.join("+");
}

/**
 * Normalize a stored or user-supplied combo string into canonical modifier
 * order (`Ctrl+Alt+Shift+Key`).
 */
export function canonicalCombo(combo: string): string {
  if (combo === " ") return "Space";
  const trimmed = combo.trim();
  if (!trimmed) return "";
  if (trimmed === "{}" || trimmed === "[]") return trimmed;

  // Handle trailing "+" key like "Ctrl++" or standalone "+"
  const tokens: string[] = [];
  let buf = "";
  for (let i = 0; i < trimmed.length; i++) {
    const ch = trimmed[i];
    if (ch === "+" && buf.length > 0) {
      tokens.push(buf);
      buf = "";
    } else {
      buf += ch;
    }
  }
  if (buf.length > 0) tokens.push(buf);
  if (tokens.length === 0) return "";

  let hasCtrl = false;
  let hasAlt = false;
  let hasShift = false;
  let mainKey: string | null = null;

  for (let i = 0; i < tokens.length; i++) {
    const tok = tokens[i];
    const lower = tok.toLowerCase();
    const isLast = i === tokens.length - 1;
    if (!isLast && (lower === "ctrl" || lower === "control" || lower === "cmd" || lower === "meta")) {
      hasCtrl = true;
    } else if (!isLast && (lower === "alt" || lower === "option")) {
      hasAlt = true;
    } else if (!isLast && lower === "shift") {
      hasShift = true;
    } else {
      mainKey = normalizeKeyToken(tok);
    }
  }

  if (!mainKey) return "";
  const out: string[] = [];
  if (hasCtrl) out.push("Ctrl");
  if (hasAlt) out.push("Alt");
  if (hasShift) out.push("Shift");
  out.push(mainKey);
  return out.join("+");
}

const DISPLAY_KEY_MAP: Record<string, string> = {
  ArrowLeft: "←",
  ArrowRight: "→",
  ArrowUp: "↑",
  ArrowDown: "↓",
  Escape: "Esc",
  Delete: "Del",
};

/** Split a combo string into displayable keycap parts. */
export function splitComboParts(combo: string, isMac = false): string[] {
  const canonical = canonicalCombo(combo);
  if (!canonical) return [];
  if (canonical === "{}" || canonical === "[]") return [canonical];

  const rawParts: string[] = [];
  let buf = "";
  for (let i = 0; i < canonical.length; i++) {
    const ch = canonical[i];
    if (ch === "+" && buf.length > 0) {
      rawParts.push(buf);
      buf = "";
    } else {
      buf += ch;
    }
  }
  if (buf.length > 0) rawParts.push(buf);

  return rawParts.map((part) => {
    if (part === "Ctrl") return isMac ? "Cmd" : "Ctrl";
    return DISPLAY_KEY_MAP[part] ?? part;
  });
}

/** Format a combo string for UI labels and tooltips. */
export function formatCombo(combo: string, isMac = false): string {
  return splitComboParts(combo, isMac).join("+");
}

/**
 * Match a KeyboardEvent against a canonical or stored combo string.
 */
export function matchesCombo(e: KeyEventLike, combo: string): boolean {
  const target = canonicalCombo(combo);
  if (!target || target === "{}" || target === "[]") return false;

  const pressed = normalizeCombo(e);
  if (!pressed) return false;
  if (pressed === target) return true;

  // Layout tolerance for "?" (on some keyboard layouts ? may not report shiftKey)
  if ((target === "Shift+?" && pressed === "?") || (target === "?" && pressed === "Shift+?")) {
    return true;
  }

  // Layout tolerance for Ctrl+/ using physical Slash key when unshifted
  if (
    target === "Ctrl+/" &&
    (e.ctrlKey || e.metaKey) &&
    !e.altKey &&
    !e.shiftKey &&
    e.code === "Slash"
  ) {
    return true;
  }

  return false;
}

export function scopesOverlap(a: ShortcutScope, b: ShortcutScope): boolean {
  if (a === b) return true;
  if (a === "global" || b === "global") return true;
  if (a === "generate" && (b === "prompt" || b === "canvas")) return true;
  if (b === "generate" && (a === "prompt" || a === "canvas")) return true;
  return false;
}

export function effectiveComboFor(
  def: ShortcutDef,
  bindings: Readonly<Record<string, string>>,
): string {
  if (def.readOnly) return def.defaultCombo;
  const custom = bindings[def.id];
  if (typeof custom === "string") {
    const normalized = canonicalCombo(custom);
    if (normalized) return normalized;
  }
  return def.defaultCombo;
}

/**
 * Detect whether assigning `combo` to `actionId` collides with another
 * shortcut in the same scope or an overlapping scope.
 */
export function detectConflict(
  actionId: string,
  combo: string,
  bindings: Readonly<Record<string, string>>,
  defs: readonly ShortcutDef[] = DEFAULT_SHORTCUTS,
): ShortcutConflict | null {
  const target = defs.find((d) => d.id === actionId);
  if (!target || target.readOnly) return null;

  const candidate = canonicalCombo(combo);
  if (!candidate) return null;

  for (const other of defs) {
    if (other.id === actionId) continue;
    if (!scopesOverlap(target.scope, other.scope)) continue;
    const otherCombo = canonicalCombo(effectiveComboFor(other, bindings));
    if (!otherCombo) continue;
    if (
      otherCombo === candidate ||
      (candidate === "Shift+?" && otherCombo === "?") ||
      (candidate === "?" && otherCombo === "Shift+?")
    ) {
      return {
        conflictingId: other.id,
        def: other,
        combo: otherCombo,
      };
    }
  }

  return null;
}

/**
 * Filter persisted shortcut overrides to known, editable shortcuts whose
 * binding is valid and differs from the default.
 */
export function sanitizeShortcutBindings(
  raw: unknown,
  defs: readonly ShortcutDef[] = DEFAULT_SHORTCUTS,
): Record<string, string> {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return {};
  const byId = new Map(defs.map((d) => [d.id, d]));
  const out: Record<string, string> = {};
  for (const [id, val] of Object.entries(raw as Record<string, unknown>)) {
    const def = byId.get(id);
    if (!def || def.readOnly || typeof val !== "string") continue;
    const normalized = canonicalCombo(val);
    if (!normalized || normalized === canonicalCombo(def.defaultCombo)) continue;
    out[id] = normalized;
  }
  return out;
}

/**
 * True when `target` is a text input, textarea, select, or contenteditable
 * element so single-key shortcuts should yield to typing.
 */
export function isTypingTarget(target: unknown): boolean {
  if (!target || typeof target !== "object") return false;
  const el = target as {
    isContentEditable?: boolean;
    tagName?: string;
    getAttribute?: (name: string) => string | null;
    closest?: (selector: string) => unknown;
  };
  if (el.isContentEditable) return true;
  const tag = typeof el.tagName === "string" ? el.tagName.toUpperCase() : "";
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  if (typeof el.getAttribute === "function" && el.getAttribute("contenteditable") === "true") {
    return true;
  }
  if (typeof el.closest === "function" && el.closest("[contenteditable='true']")) {
    return true;
  }
  return false;
}
