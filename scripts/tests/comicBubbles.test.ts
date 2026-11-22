import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_BUBBLE_FONT_SIZE,
  MIN_BUBBLE_H,
  MIN_BUBBLE_W,
  bubblePathData,
  bubblesFromDialogue,
  clampBubble,
  drawBubbleOnCanvas,
  estimateTextWidth,
  layoutVerticalText,
  newBubble,
  resizeBubble,
  sanitizeBubbles,
  wrapBubbleText,
  type BubbleCanvasContext,
} from "../../src/lib/utils/comicBubbles.ts";

test("newBubble and clampBubble enforce minimum size and 0..1 bounds", () => {
  const b = newBubble({ x: 0.98, y: 0.99, w: 0.01, h: 0.005, tailX: 1.5, tailY: -0.4 });
  assert.ok(b.w >= MIN_BUBBLE_W - 1e-9);
  assert.ok(b.h >= MIN_BUBBLE_H - 1e-9);
  assert.ok(b.x >= 0 && b.x + b.w <= 1 + 1e-9);
  assert.ok(b.y >= 0 && b.y + b.h <= 1 + 1e-9);
  assert.equal(b.tailX, 1);
  assert.equal(b.tailY, 0);
  assert.equal(b.fontSize, DEFAULT_BUBBLE_FONT_SIZE);

  const bad = clampBubble({
    id: "",
    x: Number.NaN,
    y: -2,
    w: 2,
    h: 0.001,
    tailX: Number.POSITIVE_INFINITY,
    tailY: 0.5,
    shape: "unknown" as never,
    text: 123 as never,
    font: "unknown" as never,
    fontSize: 999,
    vertical: false,
  });
  assert.ok(bad.id.length > 0);
  assert.equal(bad.w, 1);
  assert.equal(bad.h, MIN_BUBBLE_H);
  assert.equal(bad.x, 0);
  assert.equal(bad.y, 0);
  assert.equal(bad.shape, "ellipse");
  assert.equal(bad.font, "sans");
  assert.equal(bad.fontSize, 96);
  assert.equal(bad.text, "");
});

test("resizeBubble keeps opposite corner anchored and respects min size", () => {
  const orig = newBubble({ x: 0.2, y: 0.2, w: 0.3, h: 0.2 });
  const nw = resizeBubble(orig, "nw", 0.5, 0.5);
  assert.ok(Math.abs(nw.x + nw.w - 0.5) < 1e-9);
  assert.ok(Math.abs(nw.y + nw.h - 0.4) < 1e-9);
  assert.ok(nw.w >= MIN_BUBBLE_W - 1e-9);
  assert.ok(nw.h >= MIN_BUBBLE_H - 1e-9);

  const se = resizeBubble(orig, "se", 0.2, 0.1);
  assert.equal(se.x, 0.2);
  assert.equal(se.y, 0.2);
  assert.ok(Math.abs(se.w - 0.5) < 1e-9);
  assert.ok(Math.abs(se.h - 0.3) < 1e-9);
});

test("wrapBubbleText wraps Latin at spaces, breaks oversized words, and wraps CJK cleanly", () => {
  const measure = (s: string) => estimateTextWidth(s, 20, "sans");
  const lines = wrapBubbleText("Hello comic world this wraps cleanly", 120, measure);
  assert.ok(lines.length >= 2);
  for (const line of lines) {
    assert.ok(measure(line) <= 125, `Line exceeded width: ${line}`);
  }

  const cjk = wrapBubbleText("こんにちは世界！今日はいい天気ですね。", 65, measure);
  assert.ok(cjk.length >= 2);
  assert.ok(!cjk.some((l) => l.startsWith("！") || l.startsWith("。")), "No kinsoku start punctuation");

  const longWord = wrapBubbleText("Supercalifragilisticexpialidocious", 70, measure);
  assert.ok(longWord.length >= 3);
  assert.equal(longWord.join(""), "Supercalifragilisticexpialidocious");

  assert.deepEqual(wrapBubbleText("   ", 100, measure), []);
});

test("layoutVerticalText arranges tategaki top-to-bottom, columns right-to-left, and rotates punctuation", () => {
  const empty = layoutVerticalText("  ", 200, 300, 24);
  assert.equal(empty.glyphs.length, 0);
  assert.equal(empty.columns, 0);

  const layout = layoutVerticalText("「あーい…」\n（う〜ん）", 300, 400, 24);
  assert.equal(layout.columns, 2);

  // First column: 「, あ, ー, い, …, 」 (6 glyphs)
  const col0 = layout.glyphs.slice(0, 6);
  const col1 = layout.glyphs.slice(6);
  assert.equal(col1.length, 5);

  // Top-to-bottom within column 0.
  for (let i = 1; i < col0.length; i++) {
    assert.ok(col0[i].y > col0[i - 1].y, "Glyphs in column must descend top-to-bottom");
  }
  // Columns right-to-left: column 0 is to the right of column 1.
  assert.ok(col0[0].x > col1[0].x, "Column 0 must sit right of column 1");

  // Punctuation rotation check: 「, ー, …, 」, （, 〜, ） all rotate 90 deg clockwise.
  const rotatedChars = new Set(["「", "ー", "…", "」", "（", "〜", "）"]);
  for (const g of layout.glyphs) {
    if (rotatedChars.has(g.char)) {
      assert.ok(Math.abs(g.rotate - Math.PI / 2) < 1e-9, `Expected rotation for ${g.char}`);
    } else {
      assert.equal(g.rotate, 0, `Expected upright glyph for ${g.char}`);
    }
  }

  // Automatic column wrapping when boxHeight fits only 3 chars.
  const autoWrap = layoutVerticalText("あいうえお", 300, 80, 24);
  assert.equal(autoWrap.columns, 2);
  assert.ok(autoWrap.glyphs[0].x > autoWrap.glyphs[3].x);
});

test("bubblePathData builds ellipse, thought, shout, and caption geometries with tail handling", () => {
  const pageW = 1448;
  const pageH = 2048;

  const ellipseWithTail = newBubble({ x: 0.2, y: 0.2, w: 0.3, h: 0.15, tailX: 0.35, tailY: 0.45, shape: "ellipse" });
  const gEllipse = bubblePathData(ellipseWithTail, pageW, pageH);
  assert.ok(gEllipse.hasTail);
  assert.ok(gEllipse.path.startsWith("M") && gEllipse.path.endsWith("Z"));
  assert.ok(gEllipse.path.includes("Q"), "Curved tail uses quadratic segments");
  assert.equal(gEllipse.dots.length, 0);

  const ellipseNoTail = newBubble({ x: 0.2, y: 0.2, w: 0.3, h: 0.15, tailX: 0.35, tailY: 0.27, shape: "ellipse" });
  assert.equal(bubblePathData(ellipseNoTail, pageW, pageH).hasTail, false);

  const thought = newBubble({ x: 0.2, y: 0.2, w: 0.3, h: 0.15, tailX: 0.35, tailY: 0.45, shape: "thought" });
  const gThought = bubblePathData(thought, pageW, pageH);
  assert.ok(gThought.hasTail);
  assert.equal(gThought.dots.length, 3);
  assert.ok(gThought.dots[0].r > gThought.dots[1].r && gThought.dots[1].r > gThought.dots[2].r);

  const shout = newBubble({ x: 0.2, y: 0.2, w: 0.3, h: 0.15, tailX: 0.65, tailY: 0.45, shape: "shout" });
  const gShout = bubblePathData(shout, pageW, pageH);
  assert.ok(gShout.hasTail);
  assert.ok(gShout.commands.some((c) => c.op === "L" && Math.abs(c.x - 0.65 * pageW) < 1e-6));

  const caption = newBubble({ x: 0.1, y: 0.1, w: 0.4, h: 0.1, tailX: 0.8, tailY: 0.8, shape: "caption" });
  const gCaption = bubblePathData(caption, pageW, pageH);
  assert.equal(gCaption.hasTail, false);
  assert.equal(gCaption.dots.length, 0);
});

test("bubblesFromDialogue converts lines into panel-bounded bubbles and detects Japanese tategaki", () => {
  const rect = { x: 0.05, y: 0.1, w: 0.4, h: 0.4 };
  assert.deepEqual(bubblesFromDialogue("p1", rect, "  \n  "), []);

  const en = bubblesFromDialogue("p1", rect, "Wait!\nDon't go!");
  assert.equal(en.length, 2);
  assert.equal(en[0].panelId, "p1");
  assert.equal(en[0].text, "Wait!");
  assert.equal(en[0].vertical, false);
  assert.equal(en[1].text, "Don't go!");
  for (const b of en) {
    assert.ok(b.x >= rect.x - 1e-9 && b.x + b.w <= rect.x + rect.w + 1e-9);
    assert.ok(b.y >= rect.y - 1e-9 && b.y + b.h <= rect.y + rect.h + 1e-9);
  }

  const jp = bubblesFromDialogue("p2", rect, "おはよう！\n今日も頑張ろう");
  assert.equal(jp.length, 2);
  assert.equal(jp[0].vertical, true);
  assert.equal(jp[1].vertical, true);
});

test("drawBubbleOnCanvas renders horizontal, vertical, and thought shapes without DOM globals", () => {
  const calls: string[] = [];
  const drawnText: Array<{ text: string; rotated: boolean }> = [];
  let currentRotation = 0;

  const ctx: BubbleCanvasContext = {
    fillStyle: "",
    strokeStyle: "",
    lineWidth: 1,
    lineJoin: "miter",
    lineCap: "butt",
    font: "",
    textAlign: "start",
    textBaseline: "alphabetic",
    save() { calls.push("save"); },
    restore() { calls.push("restore"); currentRotation = 0; },
    beginPath() { calls.push("beginPath"); },
    moveTo() {},
    lineTo() {},
    quadraticCurveTo() {},
    arc() { calls.push("arc"); },
    closePath() {},
    fill() { calls.push("fill"); },
    stroke() { calls.push("stroke"); },
    translate() {},
    rotate(a: number) { currentRotation = a; calls.push("rotate"); },
    fillText(text: string) { drawnText.push({ text, rotated: currentRotation !== 0 }); },
    measureText(text: string) { return { width: text.length * 16 }; },
  };

  const thought = newBubble({ shape: "thought", text: "Hmm, maybe...", tailX: 0.5, tailY: 0.7 });
  drawBubbleOnCanvas(ctx, thought, 1448, 2048);
  assert.ok(calls.filter((c) => c === "arc").length === 3, "Draws 3 thought dots");
  assert.ok(drawnText.some((d) => d.text.includes("Hmm")));

  drawnText.length = 0;
  calls.length = 0;
  const jp = newBubble({ shape: "ellipse", text: "あー！", vertical: true });
  drawBubbleOnCanvas(ctx, jp, 1448, 2048);
  assert.ok(drawnText.some((d) => d.text === "あ" && !d.rotated));
  assert.ok(drawnText.some((d) => d.text === "ー" && d.rotated));

  const sanitized = sanitizeBubbles([thought, { bad: true }, null, { ...thought }]);
  assert.equal(sanitized.length, 2); // second duplicate id filtered out, malformed clamped or skipped
});
