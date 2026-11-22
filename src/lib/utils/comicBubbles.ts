// Pure comic speech bubble geometry, text wrapping, Japanese tategaki layout,
// and Canvas 2D renderer. Zero DOM globals so Node tests can validate all
// layout, wrapping, vertical punctuation, and path geometry directly.

import { dialogueLines, newId, type Rect } from "./comicLayout.js";

export type BubbleShape = "ellipse" | "thought" | "shout" | "caption";
export type BubbleFont = "sans" | "serif" | "comic" | "mono";

export const BUBBLE_SHAPES: readonly BubbleShape[] = ["ellipse", "thought", "shout", "caption"];
export const BUBBLE_FONTS: readonly BubbleFont[] = ["sans", "serif", "comic", "mono"];

export const MIN_BUBBLE_W = 0.06;
export const MIN_BUBBLE_H = 0.04;
export const DEFAULT_BUBBLE_FONT_SIZE = 22;

export interface SpeechBubble {
  id: string;
  panelId?: string;
  x: number;
  y: number;
  w: number;
  h: number;
  tailX: number;
  tailY: number;
  shape: BubbleShape;
  text: string;
  font: BubbleFont;
  fontSize: number;
  vertical: boolean;
}

export type BubbleCorner = "nw" | "ne" | "sw" | "se";

export type PathCmd =
  | { op: "M"; x: number; y: number }
  | { op: "L"; x: number; y: number }
  | { op: "Q"; cx: number; cy: number; x: number; y: number }
  | { op: "Z" };

export interface BubbleDot {
  cx: number;
  cy: number;
  r: number;
}

export interface BubbleGeometry {
  path: string;
  commands: PathCmd[];
  dots: BubbleDot[];
  textBox: Rect;
  cx: number;
  cy: number;
  hasTail: boolean;
  tailAnchor: { x: number; y: number };
}

export interface VerticalGlyph {
  char: string;
  /** Center X relative to textBox left edge. */
  x: number;
  /** Center Y relative to textBox top edge. */
  y: number;
  /** Clockwise rotation in radians (0 for upright CJK, Math.PI / 2 for rotated punctuation/Latin). */
  rotate: number;
}

export interface VerticalLayout {
  glyphs: VerticalGlyph[];
  columns: number;
  lineHeight: number;
  colPitch: number;
  contentWidth: number;
  contentHeight: number;
}

/** Minimal structural subset of CanvasRenderingContext2D (keeps module free of DOM types). */
export interface BubbleCanvasContext {
  fillStyle: string | CanvasGradient | CanvasPattern;
  strokeStyle: string | CanvasGradient | CanvasPattern;
  lineWidth: number;
  lineJoin: CanvasLineJoin;
  lineCap: CanvasLineCap;
  font: string;
  textAlign: CanvasTextAlign;
  textBaseline: CanvasTextBaseline;
  save(): void;
  restore(): void;
  beginPath(): void;
  moveTo(x: number, y: number): void;
  lineTo(x: number, y: number): void;
  quadraticCurveTo(cpx: number, cpy: number, x: number, y: number): void;
  arc(x: number, y: number, radius: number, startAngle: number, endAngle: number): void;
  closePath(): void;
  fill(): void;
  stroke(): void;
  translate(x: number, y: number): void;
  rotate(angle: number): void;
  fillText(text: string, x: number, y: number): void;
  measureText(text: string): { width: number };
}

function finiteOr(v: unknown, fallback: number): number {
  return typeof v === "number" && Number.isFinite(v) ? v : fallback;
}

export function clampBubble(b: SpeechBubble): SpeechBubble {
  const w = Math.min(1, Math.max(MIN_BUBBLE_W, finiteOr(b.w, 0.28)));
  const h = Math.min(1, Math.max(MIN_BUBBLE_H, finiteOr(b.h, 0.14)));
  const x = Math.min(1 - w, Math.max(0, finiteOr(b.x, 0.36)));
  const y = Math.min(1 - h, Math.max(0, finiteOr(b.y, 0.38)));
  const tailX = Math.min(1, Math.max(0, finiteOr(b.tailX, x + w * 0.5)));
  const tailY = Math.min(1, Math.max(0, finiteOr(b.tailY, Math.min(0.98, y + h + 0.06))));
  const shape: BubbleShape = BUBBLE_SHAPES.includes(b.shape) ? b.shape : "ellipse";
  const font: BubbleFont = BUBBLE_FONTS.includes(b.font) ? b.font : "sans";
  const fontSize = Math.min(96, Math.max(8, Math.round(finiteOr(b.fontSize, DEFAULT_BUBBLE_FONT_SIZE))));
  return {
    id: typeof b.id === "string" && b.id ? b.id : newId("b"),
    ...(b.panelId ? { panelId: b.panelId } : {}),
    x,
    y,
    w,
    h,
    tailX,
    tailY,
    shape,
    text: typeof b.text === "string" ? b.text : "",
    font,
    fontSize,
    vertical: Boolean(b.vertical),
  };
}

export function newBubble(partial?: Partial<SpeechBubble>): SpeechBubble {
  const x = finiteOr(partial?.x, 0.36);
  const y = finiteOr(partial?.y, 0.38);
  const w = finiteOr(partial?.w, 0.28);
  const h = finiteOr(partial?.h, 0.14);
  return clampBubble({
    id: partial?.id || newId("b"),
    ...(partial?.panelId ? { panelId: partial.panelId } : {}),
    x,
    y,
    w,
    h,
    tailX: finiteOr(partial?.tailX, x + w * 0.5),
    tailY: finiteOr(partial?.tailY, Math.min(0.98, y + h + 0.06)),
    shape: partial?.shape ?? "ellipse",
    text: partial?.text ?? "",
    font: partial?.font ?? "sans",
    fontSize: partial?.fontSize ?? DEFAULT_BUBBLE_FONT_SIZE,
    vertical: partial?.vertical ?? false,
  });
}

export function resizeBubble(orig: SpeechBubble, corner: BubbleCorner, dx: number, dy: number): SpeechBubble {
  let left = orig.x;
  let right = orig.x + orig.w;
  let top = orig.y;
  let bottom = orig.y + orig.h;

  if (corner === "nw" || corner === "sw") {
    left = Math.max(0, Math.min(right - MIN_BUBBLE_W, orig.x + dx));
  } else {
    right = Math.min(1, Math.max(left + MIN_BUBBLE_W, orig.x + orig.w + dx));
  }

  if (corner === "nw" || corner === "ne") {
    top = Math.max(0, Math.min(bottom - MIN_BUBBLE_H, orig.y + dy));
  } else {
    bottom = Math.min(1, Math.max(top + MIN_BUBBLE_H, orig.y + orig.h + dy));
  }

  return clampBubble({
    ...orig,
    x: left,
    y: top,
    w: right - left,
    h: bottom - top,
  });
}

export function sanitizeBubbles(raw: unknown): SpeechBubble[] {
  if (!Array.isArray(raw)) return [];
  const seen = new Set<string>();
  const out: SpeechBubble[] = [];
  for (const item of raw) {
    if (!item || typeof item !== "object") continue;
    const obj = item as Partial<SpeechBubble>;
    const clamped = clampBubble(obj as SpeechBubble);
    if (seen.has(clamped.id)) continue;
    seen.add(clamped.id);
    out.push(clamped);
  }
  return out;
}

export function bubbleFontFamily(font: BubbleFont): string {
  switch (font) {
    case "serif":
      return "'Noto Serif JP', 'Hiragino Mincho ProN', 'Yu Mincho', Georgia, serif";
    case "comic":
      return "'Comic Sans MS', 'Bangers', 'Anime Ace', 'Noto Sans JP', cursive, sans-serif";
    case "mono":
      return "'JetBrains Mono', 'Noto Sans Mono CJK JP', ui-monospace, monospace";
    case "sans":
    default:
      return "'Inter', 'Noto Sans JP', 'Hiragino Kaku Gothic ProN', 'Meiryo', system-ui, sans-serif";
  }
}

export function bubbleFontPx(fontSize: number, pageW: number, pageH: number): number {
  const scale = Math.sqrt(Math.max(1, pageW) * Math.max(1, pageH)) / 1000;
  return Math.max(8, fontSize * scale);
}

const CJK_OR_FULLWIDTH_RE = /[\u3000-\u303f\u3040-\u30ff\u3400-\u9fff\uf900-\ufaff\uac00-\ud7af\uff00-\uffef]/;

/** Pure width estimate for SVG/Node environments without a Canvas 2D measureText context. */
export function estimateTextWidth(text: string, fontSize: number, font: BubbleFont = "sans"): number {
  const monoFactor = font === "mono" ? 0.62 : 0;
  let width = 0;
  for (const ch of text) {
    if (ch === " ") width += fontSize * 0.3;
    else if (CJK_OR_FULLWIDTH_RE.test(ch)) width += fontSize * 1.0;
    else if (monoFactor > 0) width += fontSize * monoFactor;
    else if (/[ilI.,;:'"!|]/.test(ch)) width += fontSize * 0.34;
    else if (/[mwMW@]/.test(ch)) width += fontSize * 0.78;
    else width += fontSize * 0.56;
  }
  return width;
}

const KINSOKU_NO_START = new Set([
  "。", "、", "．", "，", "！", "？", "!", "?", "」", "』", "）", ")", "]", "】", "》", "〉", "ー", "〜", "～", "…", "っ", "ゃ", "ゅ", "ょ", "ッ", "ャ", "ュ", "ョ",
]);

/** Tokenize a line into words/CJK characters so Latin wraps at spaces and CJK wraps per character. */
function tokenizeWrapLine(line: string): string[] {
  const tokens: string[] = [];
  let buf = "";
  let inSpace = false;

  const flush = () => {
    if (buf) {
      tokens.push(buf);
      buf = "";
    }
  };

  for (const ch of line) {
    if (/\s/.test(ch)) {
      if (!inSpace) {
        flush();
        inSpace = true;
      }
      buf += ch;
    } else if (CJK_OR_FULLWIDTH_RE.test(ch)) {
      if (inSpace) {
        flush();
        inSpace = false;
      } else if (buf) {
        flush();
      }
      if (KINSOKU_NO_START.has(ch) && tokens.length > 0 && !/^\s+$/.test(tokens[tokens.length - 1])) {
        tokens[tokens.length - 1] += ch;
      } else {
        tokens.push(ch);
      }
    } else {
      if (inSpace) {
        flush();
        inSpace = false;
      }
      buf += ch;
    }
  }
  flush();
  return tokens;
}

export function wrapBubbleText(
  text: string,
  maxWidth: number,
  measureWidth: (s: string) => number,
): string[] {
  if (!text.trim()) return [];
  const limit = Math.max(1, maxWidth);
  const paragraphs = text.replace(/\r\n?/g, "\n").split("\n");
  const lines: string[] = [];

  for (const para of paragraphs) {
    const trimmed = para.trim();
    if (!trimmed) {
      lines.push("");
      continue;
    }
    if (measureWidth(trimmed) <= limit) {
      lines.push(trimmed);
      continue;
    }
    const tokens = tokenizeWrapLine(trimmed);
    let current = "";
    for (const token of tokens) {
      if (/^\s+$/.test(token)) {
        if (current && measureWidth(current + " ") <= limit) {
          current += " ";
        }
        continue;
      }
      const candidate = current ? current + token : token;
      if (measureWidth(candidate) <= limit) {
        current = candidate;
      } else {
        if (current.trimEnd()) {
          lines.push(current.trimEnd());
          current = "";
        }
        if (measureWidth(token) <= limit) {
          current = token;
        } else {
          // Break oversized single word character-by-character.
          let chunk = "";
          for (const ch of token) {
            if (chunk && measureWidth(chunk + ch) > limit) {
              lines.push(chunk);
              chunk = ch;
            } else {
              chunk += ch;
            }
          }
          current = chunk;
        }
      }
    }
    if (current.trimEnd()) {
      lines.push(current.trimEnd());
    }
  }

  return lines;
}

/** Characters rotated 90 degrees clockwise in Japanese tategaki. */
const VERTICAL_ROTATE_CHARS = new Set([
  "ー", "〜", "～", "…", "‥", "―", "─", "−", "-", "–", "—",
  "「", "」", "『", "』", "（", "）", "(", ")", "[", "]", "{", "}",
  "【", "】", "《", "》", "〈", "〉", "〔", "〕", "［", "］", "｛", "｝",
  "=", "＝", ":", "：", ";", "；", "<", ">", "＜", "＞", "~",
]);

/** Full-width Japanese punctuation mapped from ASCII equivalents for clean vertical alignment. */
const VERTICAL_CHAR_MAP: Record<string, string> = {
  "(": "（",
  ")": "）",
  "[": "［",
  "]": "］",
  "{": "｛",
  "}": "｝",
  "~": "〜",
};

const KUTOTEN_CHARS = new Set(["。", "、", "．", "，"]);
const SMALL_KANA_CHARS = new Set([
  "ぁ", "ぃ", "ぅ", "ぇ", "ぉ", "ゃ", "ゅ", "ょ", "っ", "ゎ",
  "ァ", "ィ", "ゥ", "ェ", "ォ", "ャ", "ュ", "ョ", "ッ", "ヮ", "ヵ", "ヶ",
]);

/**
 * Lay out text in Japanese tategaki: glyph-by-glyph top-to-bottom within columns,
 * columns ordered right-to-left, with punctuation mapped/rotated for vertical reading.
 */
export function layoutVerticalText(
  text: string,
  boxWidth: number,
  boxHeight: number,
  fontSize: number,
): VerticalLayout {
  const charStep = fontSize * 1.08;
  const colPitch = fontSize * 1.24;
  if (!text.trim()) {
    return { glyphs: [], columns: 0, lineHeight: charStep, colPitch, contentWidth: 0, contentHeight: 0 };
  }

  const maxRows = Math.max(1, Math.floor(Math.max(charStep, boxHeight) / charStep));
  const rawParagraphs = text.replace(/\r\n?/g, "\n").split("\n");
  const columns: string[][] = [];

  for (const para of rawParagraphs) {
    const chars = Array.from(para.trimEnd());
    if (chars.length === 0) {
      columns.push([]);
      continue;
    }
    let col: string[] = [];
    for (const ch of chars) {
      if (col.length >= maxRows && !KINSOKU_NO_START.has(ch)) {
        columns.push(col);
        col = [];
      }
      col.push(ch);
    }
    if (col.length > 0) columns.push(col);
  }

  // Drop trailing empty columns if any, keep at least 1 if text non-empty.
  while (columns.length > 1 && columns[columns.length - 1].length === 0) {
    columns.pop();
  }

  const numCols = Math.max(1, columns.length);
  const maxColLen = columns.reduce((m, c) => Math.max(m, c.length), 0);
  const contentWidth = (numCols - 1) * colPitch + fontSize;
  const contentHeight = maxColLen * charStep;

  // Column 0 sits rightmost; each subsequent column steps left by colPitch.
  const rightMostX = boxWidth / 2 + ((numCols - 1) * colPitch) / 2;
  const topY = (boxHeight - contentHeight) / 2 + charStep * 0.5;

  const glyphs: VerticalGlyph[] = [];
  for (let c = 0; c < columns.length; c++) {
    const colChars = columns[c];
    const baseX = rightMostX - c * colPitch;
    for (let r = 0; r < colChars.length; r++) {
      const rawCh = colChars[r];
      const char = VERTICAL_CHAR_MAP[rawCh] ?? rawCh;
      let x = baseX;
      let y = topY + r * charStep;
      let rotate = 0;

      if (VERTICAL_ROTATE_CHARS.has(rawCh) || VERTICAL_ROTATE_CHARS.has(char) || /^[A-Za-z0-9]$/.test(rawCh)) {
        rotate = Math.PI / 2;
      } else if (KUTOTEN_CHARS.has(char)) {
        x += fontSize * 0.28;
        y -= fontSize * 0.28;
      } else if (SMALL_KANA_CHARS.has(char)) {
        x += fontSize * 0.08;
        y -= fontSize * 0.05;
      }

      glyphs.push({ char, x, y, rotate });
    }
  }

  return {
    glyphs,
    columns: numCols,
    lineHeight: charStep,
    colPitch,
    contentWidth,
    contentHeight,
  };
}

function fmt(n: number): string {
  return n.toFixed(2);
}

export function commandsToSvgPath(commands: PathCmd[]): string {
  return commands
    .map((c) => {
      switch (c.op) {
        case "M":
          return `M${fmt(c.x)} ${fmt(c.y)}`;
        case "L":
          return `L${fmt(c.x)} ${fmt(c.y)}`;
        case "Q":
          return `Q${fmt(c.cx)} ${fmt(c.cy)} ${fmt(c.x)} ${fmt(c.y)}`;
        case "Z":
          return "Z";
      }
    })
    .join(" ");
}

export function replayPathCommands(ctx: BubbleCanvasContext, commands: PathCmd[]): void {
  ctx.beginPath();
  for (const c of commands) {
    switch (c.op) {
      case "M":
        ctx.moveTo(c.x, c.y);
        break;
      case "L":
        ctx.lineTo(c.x, c.y);
        break;
      case "Q":
        ctx.quadraticCurveTo(c.cx, c.cy, c.x, c.y);
        break;
      case "Z":
        ctx.closePath();
        break;
    }
  }
}

export function bubblePathData(bubble: SpeechBubble, pageW: number, pageH: number): BubbleGeometry {
  const bx = bubble.x * pageW;
  const by = bubble.y * pageH;
  const bw = bubble.w * pageW;
  const bh = bubble.h * pageH;
  const cx = bx + bw / 2;
  const cy = by + bh / 2;
  const rx = Math.max(1, bw / 2);
  const ry = Math.max(1, bh / 2);
  const tx = bubble.tailX * pageW;
  const ty = bubble.tailY * pageH;

  const dx = tx - cx;
  const dy = ty - cy;
  const normDist = Math.hypot(dx / rx, dy / ry);
  const theta = Math.atan2(dy / ry, dx / rx);
  const tailAnchor = {
    x: cx + rx * Math.cos(theta),
    y: cy + ry * Math.sin(theta),
  };
  const hasTail = bubble.shape !== "caption" && normDist > 1.06;

  const commands: PathCmd[] = [];
  const dots: BubbleDot[] = [];
  let textBox: Rect;

  if (bubble.shape === "caption") {
    const r = Math.min(6, bw * 0.05, bh * 0.08);
    commands.push(
      { op: "M", x: bx + r, y: by },
      { op: "L", x: bx + bw - r, y: by },
      { op: "Q", cx: bx + bw, cy: by, x: bx + bw, y: by + r },
      { op: "L", x: bx + bw, y: by + bh - r },
      { op: "Q", cx: bx + bw, cy: by + bh, x: bx + bw - r, y: by + bh },
      { op: "L", x: bx + r, y: by + bh },
      { op: "Q", cx: bx, cy: by + bh, x: bx, y: by + bh - r },
      { op: "L", x: bx, y: by + r },
      { op: "Q", cx: bx, cy: by, x: bx + r, y: by },
      { op: "Z" },
    );
    textBox = {
      x: bx + bw * 0.08,
      y: by + bh * 0.1,
      w: bw * 0.84,
      h: bh * 0.8,
    };
  } else if (bubble.shape === "thought") {
    const lobes = 14;
    const innerScale = 0.84;
    const outerScale = 1.18;
    for (let i = 0; i < lobes; i++) {
      const a0 = (i / lobes) * Math.PI * 2;
      const a1 = ((i + 1) / lobes) * Math.PI * 2;
      const am = (a0 + a1) / 2;
      const x0 = cx + rx * innerScale * Math.cos(a0);
      const y0 = cy + ry * innerScale * Math.sin(a0);
      const x1 = cx + rx * innerScale * Math.cos(a1);
      const y1 = cy + ry * innerScale * Math.sin(a1);
      const ctrlX = cx + rx * outerScale * Math.cos(am);
      const ctrlY = cy + ry * outerScale * Math.sin(am);
      if (i === 0) commands.push({ op: "M", x: x0, y: y0 });
      commands.push({ op: "Q", cx: ctrlX, cy: ctrlY, x: x1, y: y1 });
    }
    commands.push({ op: "Z" });

    if (hasTail) {
      const minR = Math.min(rx, ry);
      const ax = cx + rx * 0.96 * Math.cos(theta);
      const ay = cy + ry * 0.96 * Math.sin(theta);
      const specs = [
        { t: 0.24, r: Math.max(3, minR * 0.12) },
        { t: 0.56, r: Math.max(2.2, minR * 0.08) },
        { t: 0.88, r: Math.max(1.5, minR * 0.048) },
      ];
      for (const s of specs) {
        dots.push({
          cx: ax + (tx - ax) * s.t,
          cy: ay + (ty - ay) * s.t,
          r: s.r,
        });
      }
    }

    textBox = {
      x: cx - bw * 0.34,
      y: cy - bh * 0.33,
      w: bw * 0.68,
      h: bh * 0.66,
    };
  } else if (bubble.shape === "shout") {
    const spikes = 16;
    const total = spikes * 2;
    let tailSpikeIdx = -1;
    if (hasTail) {
      let bestDiff = Infinity;
      for (let i = 0; i < total; i += 2) {
        const a = (i / total) * Math.PI * 2;
        const diff = Math.abs(Math.atan2(Math.sin(a - theta), Math.cos(a - theta)));
        if (diff < bestDiff) {
          bestDiff = diff;
          tailSpikeIdx = i;
        }
      }
    }

    for (let i = 0; i < total; i++) {
      const a = (i / total) * Math.PI * 2;
      let px: number;
      let py: number;
      if (i === tailSpikeIdx) {
        px = tx;
        py = ty;
      } else {
        const isOuter = i % 2 === 0;
        const scale = isOuter ? (i % 4 === 0 ? 1.1 : 1.02) : 0.78;
        px = cx + rx * scale * Math.cos(a);
        py = cy + ry * scale * Math.sin(a);
      }
      commands.push({ op: i === 0 ? "M" : "L", x: px, y: py });
    }
    commands.push({ op: "Z" });

    textBox = {
      x: cx - bw * 0.31,
      y: cy - bh * 0.3,
      w: bw * 0.62,
      h: bh * 0.6,
    };
  } else {
    // Classic smooth speech ellipse with seamless perimeter-anchored curved tail.
    const steps = 56;
    if (!hasTail) {
      for (let i = 0; i < steps; i++) {
        const a = (i / steps) * Math.PI * 2;
        const px = cx + rx * Math.cos(a);
        const py = cy + ry * Math.sin(a);
        commands.push({ op: i === 0 ? "M" : "L", x: px, y: py });
      }
      commands.push({ op: "Z" });
    } else {
      const spread = Math.min(0.3, Math.max(0.14, 16 / Math.max(1, Math.min(rx, ry))));
      const startA = theta + spread;
      const endA = theta + Math.PI * 2 - spread;
      for (let i = 0; i <= steps; i++) {
        const a = startA + ((endA - startA) * i) / steps;
        const px = cx + rx * Math.cos(a);
        const py = cy + ry * Math.sin(a);
        commands.push({ op: i === 0 ? "M" : "L", x: px, y: py });
      }
      const ax1 = cx + rx * Math.cos(endA);
      const ay1 = cy + ry * Math.sin(endA);
      const ax2 = cx + rx * Math.cos(startA);
      const ay2 = cy + ry * Math.sin(startA);
      const tvx = tx - tailAnchor.x;
      const tvy = ty - tailAnchor.y;
      const tLen = Math.max(1, Math.hypot(tvx, tvy));
      const bend = Math.min(14, tLen * 0.14);
      const pxNorm = -tvy / tLen;
      const pyNorm = tvx / tLen;

      commands.push({
        op: "Q",
        cx: (ax1 + tx) / 2 + pxNorm * bend,
        cy: (ay1 + ty) / 2 + pyNorm * bend,
        x: tx,
        y: ty,
      });
      commands.push({
        op: "Q",
        cx: (ax2 + tx) / 2 + pxNorm * bend,
        cy: (ay2 + ty) / 2 + pyNorm * bend,
        x: ax2,
        y: ay2,
      });
      commands.push({ op: "Z" });
    }

    textBox = {
      x: cx - bw * 0.36,
      y: cy - bh * 0.36,
      w: bw * 0.72,
      h: bh * 0.72,
    };
  }

  return {
    path: commandsToSvgPath(commands),
    commands,
    dots,
    textBox,
    cx,
    cy,
    hasTail,
    tailAnchor,
  };
}

/** Convert a panel's dialogue lines into positioned speech bubbles inside `rect`. */
export function bubblesFromDialogue(panelId: string, rect: Rect, dialogueText: string): SpeechBubble[] {
  const lines = dialogueLines(dialogueText);
  if (lines.length === 0) return [];

  const padX = Math.min(0.03, rect.w * 0.08);
  const padY = Math.min(0.03, rect.h * 0.08);
  const availW = Math.max(MIN_BUBBLE_W, rect.w - padX * 2);
  const availH = Math.max(MIN_BUBBLE_H, rect.h - padY * 2);
  const n = lines.length;

  return lines.map((line, i) => {
    const isJapanese = /[\u3040-\u30ff]/.test(line);
    const w = Math.min(availW, Math.max(MIN_BUBBLE_W, isJapanese ? Math.min(0.22, rect.w * 0.42) : Math.min(0.32, rect.w * 0.65)));
    const slotH = Math.min(0.22, Math.max(MIN_BUBBLE_H, availH / Math.max(2, n + 0.5)));
    const h = isJapanese ? Math.min(availH * 0.85, Math.max(slotH, Math.min(0.26, rect.h * 0.55))) : slotH;

    // Stagger bubbles across upper/left-to-right slots so they don't pile in the center.
    const xFrac = n === 1 ? 0.5 : (i % 2 === 0 ? 0.25 : 0.75);
    const x = rect.x + padX + Math.max(0, (availW - w) * xFrac);
    const ySpan = Math.max(0, availH * 0.65 - h);
    const y = rect.y + padY + (n === 1 ? 0 : (ySpan * i) / (n - 1));

    const tailX = Math.min(rect.x + rect.w - 0.02, Math.max(rect.x + 0.02, rect.x + rect.w * 0.5));
    const tailY = Math.min(rect.y + rect.h - 0.02, Math.max(y + h + 0.03, rect.y + rect.h * 0.62));

    return newBubble({
      panelId,
      x,
      y,
      w,
      h,
      tailX,
      tailY,
      shape: "ellipse",
      text: line,
      font: "sans",
      fontSize: DEFAULT_BUBBLE_FONT_SIZE,
      vertical: isJapanese,
    });
  });
}

export function drawBubbleOnCanvas(
  ctx: BubbleCanvasContext,
  bubble: SpeechBubble,
  pageW: number,
  pageH: number,
): void {
  const geom = bubblePathData(bubble, pageW, pageH);
  const strokeWidth = Math.max(2, Math.round(Math.max(pageW, pageH) / 550));

  ctx.save();
  ctx.fillStyle = "#ffffff";
  ctx.strokeStyle = "#111827";
  ctx.lineWidth = strokeWidth;
  ctx.lineJoin = "round";
  ctx.lineCap = "round";

  replayPathCommands(ctx, geom.commands);
  ctx.fill();
  ctx.stroke();

  for (const dot of geom.dots) {
    ctx.beginPath();
    ctx.arc(dot.cx, dot.cy, dot.r, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
  }

  const trimmed = bubble.text.trim();
  if (trimmed) {
    const pxFont = bubbleFontPx(bubble.fontSize, pageW, pageH);
    ctx.fillStyle = "#111827";
    ctx.font = `${pxFont}px ${bubbleFontFamily(bubble.font)}`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";

    if (bubble.vertical) {
      const layout = layoutVerticalText(bubble.text, geom.textBox.w, geom.textBox.h, pxFont);
      for (const g of layout.glyphs) {
        ctx.save();
        ctx.translate(geom.textBox.x + g.x, geom.textBox.y + g.y);
        if (g.rotate !== 0) ctx.rotate(g.rotate);
        ctx.fillText(g.char, 0, 0);
        ctx.restore();
      }
    } else {
      const lines = wrapBubbleText(bubble.text, geom.textBox.w, (s) => ctx.measureText(s).width);
      if (lines.length > 0) {
        const lineHeight = pxFont * 1.22;
        const totalH = lines.length * lineHeight;
        const startY = geom.textBox.y + (geom.textBox.h - totalH) / 2 + lineHeight / 2;
        const centerX = geom.textBox.x + geom.textBox.w / 2;
        for (let i = 0; i < lines.length; i++) {
          if (lines[i]) ctx.fillText(lines[i], centerX, startY + i * lineHeight);
        }
      }
    }
  }

  ctx.restore();
}
