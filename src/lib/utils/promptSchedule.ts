import type { PromptSegment } from "../types/index.js";

/**
 * Regex patterns for scheduling tag types:
 *
 * MooshieUI XML syntax:
 * - <from:0.5>text</from>  — apply from 50% to 100%
 * - <to:0.8>text</to>      — apply from 0% to 80%
 * - <range:0.2:0.8>text</range> — apply from 20% to 80%
 *
 * SwarmUI syntax:
 * - <fromto[0.5]:before, after>  — "before" from 0% to 50%, "after" from 50% to 100%
 *   Separators: comma, | or ||
 */
const SCHEDULE_REGEX =
  /<(from|to|range):(\d+(?:\.\d+)?)(?::(\d+(?:\.\d+)?))?>([ \s\S]*?)<\/\1>/g;

const SWARM_FROMTO_REGEX =
  /<fromto\[(\d+(?:\.\d+)?)\]:([^>]+)>/g;

/**
 * Combined regex matching both syntaxes.
 * Used for highlight rendering and tag detection (single-pass over text).
 * Numeric values must be valid decimals (e.g. 0.5, 1) — not malformed like 1.2.3.
 */
const COMBINED_REGEX =
  /<(?:(from|to|range):(\d+(?:\.\d+)?)(?::(\d+(?:\.\d+)?))?>([ \s\S]*?)<\/\1>|fromto\[(\d+(?:\.\d+)?)\]:([^>]+)>)/g;

export interface ParsedPrompt {
  baseText: string;
  segments: PromptSegment[];
}

/**
 * Split a SwarmUI fromto content string by the most unique separator.
 * Priority: || > | > ,
 */
function splitSwarmContent(content: string): [string, string] | null {
  let parts: string[];
  if (content.includes("||")) {
    parts = content.split("||").map((s) => s.trim());
  } else if (content.includes("|")) {
    parts = content.split("|").map((s) => s.trim());
  } else {
    parts = content.split(",").map((s) => s.trim());
  }
  if (parts.length !== 2 || !parts[0] || !parts[1]) return null;
  return [parts[0], parts[1]];
}

/**
 * Parse a prompt string for timestep scheduling tags (both MooshieUI and SwarmUI syntax).
 *
 * Returns the base text (tags stripped, inner text kept) and an array of segments.
 * Invalid blocks (bad range values, empty text) are left as literal text.
 */
export function parseScheduledPrompt(raw: string): ParsedPrompt {
  const segments: PromptSegment[] = [];
  let baseText = "";
  let lastIndex = 0;

  COMBINED_REGEX.lastIndex = 0;

  let match: RegExpExecArray | null;
  while ((match = COMBINED_REGEX.exec(raw)) !== null) {
    const fullMatch = match[0];
    const matchStart = match.index;

    // Append text before this match to baseText
    baseText += raw.slice(lastIndex, matchStart);
    lastIndex = matchStart + fullMatch.length;

    // Determine which syntax matched
    if (match[1]) {
      // MooshieUI XML syntax: groups 1-4
      const type = match[1];
      const val1Str = match[2];
      const val2Str = match[3];
      const innerText = match[4];

      const text = innerText.trim();
      if (!text) {
        baseText += fullMatch;
        continue;
      }

      const val1 = parseFloat(val1Str);
      let start: number;
      let end: number;

      if (type === "from") {
        start = val1;
        end = 1.0;
      } else if (type === "to") {
        start = 0.0;
        end = val1;
      } else {
        start = val1;
        end = val2Str !== undefined ? parseFloat(val2Str) : 1.0;
      }

      if (isNaN(start) || isNaN(end) || start < 0 || start > 1 || end < 0 || end > 1 || start >= end) {
        baseText += fullMatch;
        continue;
      }

      segments.push({ text, start, end });
      // Do NOT add innerText to baseText — it should only apply during [start, end]
    } else if (match[5]) {
      // SwarmUI fromto syntax: groups 5-6
      const timestepStr = match[5];
      const content = match[6];
      const timestep = parseFloat(timestepStr);

      if (isNaN(timestep) || timestep <= 0 || timestep >= 1) {
        baseText += fullMatch;
        continue;
      }

      const parts = splitSwarmContent(content);
      if (!parts) {
        baseText += fullMatch;
        continue;
      }

      const [before, after] = parts;
      segments.push({ text: before, start: 0, end: timestep });
      segments.push({ text: after, start: timestep, end: 1.0 });
      // Keep both texts in baseText for metadata
      baseText += `${before}, ${after}`;
    }
  }

  // Append any remaining text after the last match
  baseText += raw.slice(lastIndex);

  // Clean up extra commas/whitespace from removed blocks
  baseText = baseText
    .replace(/,\s*,/g, ",")
    .replace(/^\s*,\s*/, "")
    .replace(/\s*,\s*$/, "")
    .trim();

  return { baseText, segments };
}

// ---------------------------------------------------------------------------
// Highlight rendering for the backdrop overlay
// ---------------------------------------------------------------------------

/** Escape HTML entities to prevent XSS in the backdrop div. */
function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/** Color config per tag type — gold/yellow matching --accent (#ffcc00) */
const TAG_COLORS: Record<string, { bg: string; border: string; glow: string }> = {
  from: {
    bg: "rgba(255, 204, 0, 0.10)",
    border: "rgba(255, 204, 0, 0.40)",
    glow: "0 0 10px rgba(255, 204, 0, 0.30), 0 0 4px rgba(255, 204, 0, 0.15)",
  },
  to: {
    bg: "rgba(255, 204, 0, 0.10)",
    border: "rgba(255, 204, 0, 0.40)",
    glow: "0 0 10px rgba(255, 204, 0, 0.30), 0 0 4px rgba(255, 204, 0, 0.15)",
  },
  range: {
    bg: "rgba(255, 204, 0, 0.10)",
    border: "rgba(255, 204, 0, 0.40)",
    glow: "0 0 10px rgba(255, 204, 0, 0.30), 0 0 4px rgba(255, 204, 0, 0.15)",
  },
  fromto: {
    bg: "rgba(255, 204, 0, 0.10)",
    border: "rgba(255, 204, 0, 0.40)",
    glow: "0 0 10px rgba(255, 204, 0, 0.30), 0 0 4px rgba(255, 204, 0, 0.15)",
  },
};

/**
 * Render prompt text as HTML with styled highlights for scheduling blocks.
 * Used by the backdrop overlay behind the textarea.
 */
export function renderHighlightedPrompt(raw: string): string {
  let html = "";
  let lastIndex = 0;

  COMBINED_REGEX.lastIndex = 0;

  let match: RegExpExecArray | null;
  while ((match = COMBINED_REGEX.exec(raw)) !== null) {
    const fullMatch = match[0];
    const matchStart = match.index;

    html += escapeHtml(raw.slice(lastIndex, matchStart));
    lastIndex = matchStart + fullMatch.length;

    let isValid = false;
    let tagType = "from";

    if (match[1]) {
      // MooshieUI XML syntax
      tagType = match[1];
      const val1 = parseFloat(match[2]);
      const val2Str = match[3];
      let start: number, end: number;
      if (tagType === "from") { start = val1; end = 1.0; }
      else if (tagType === "to") { start = 0.0; end = val1; }
      else { start = val1; end = val2Str !== undefined ? parseFloat(val2Str) : 1.0; }
      isValid = !isNaN(start) && !isNaN(end) && start >= 0 && start <= 1 && end >= 0 && end <= 1 && start < end && (match[4]?.trim().length ?? 0) > 0;
    } else if (match[5]) {
      // SwarmUI fromto syntax
      tagType = "fromto";
      const ts = parseFloat(match[5]);
      const parts = splitSwarmContent(match[6]);
      isValid = !isNaN(ts) && ts > 0 && ts < 1 && parts !== null;
    }

    if (!isValid) {
      html += escapeHtml(fullMatch);
      continue;
    }

    const colors = TAG_COLORS[tagType] ?? TAG_COLORS.from;

    html += `<span style="display:inline;color:transparent;background:${colors.bg};border:1px solid ${colors.border};border-radius:4px;box-shadow:${colors.glow};padding:1px 3px;margin:0 1px;">`;
    html += escapeHtml(fullMatch);
    html += `</span>`;
  }

  html += escapeHtml(raw.slice(lastIndex));
  return html;
}

/**
 * Check if a prompt string contains any valid scheduling tags.
 */
export function hasSchedulingTags(raw: string): boolean {
  COMBINED_REGEX.lastIndex = 0;
  return COMBINED_REGEX.test(raw);
}
