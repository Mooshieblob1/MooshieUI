import assert from "node:assert/strict";
import test from "node:test";
import {
  getPromptClickableSegments,
  type PromptClickableSegment,
} from "../src/lib/utils/promptClickableRanges.ts";

function summarize(segments: PromptClickableSegment[], raw: string) {
  return segments.map((segment) => ({
    kind: segment.kind,
    clickable: segment.clickable,
    text: raw.slice(segment.start, segment.end),
  }));
}

test("splits plain comma-separated tags into clickable tag ranges", () => {
  const raw = "blue eyes, green hair";
  const summary = summarize(getPromptClickableSegments(raw), raw);

  assert.deepEqual(summary, [
    { kind: "tag", clickable: true, text: "blue eyes" },
    { kind: "text", clickable: false, text: "," },
    { kind: "text", clickable: false, text: " " },
    { kind: "tag", clickable: true, text: "green hair" },
  ]);
});

test("treats weighted tags as one clickable weighted range", () => {
  const raw = "(blue eyes:1.20), green hair";
  const summary = summarize(getPromptClickableSegments(raw), raw);

  assert.deepEqual(summary, [
    { kind: "weighted", clickable: true, text: "(blue eyes:1.20)" },
    { kind: "text", clickable: false, text: "," },
    { kind: "text", clickable: false, text: " " },
    { kind: "tag", clickable: true, text: "green hair" },
  ]);
});

test("treats weighted groups as one clickable weighted range", () => {
  const raw = "((blue eyes, green hair):1.20), masterpiece";
  const summary = summarize(getPromptClickableSegments(raw), raw);

  assert.deepEqual(summary, [
    { kind: "weighted", clickable: true, text: "((blue eyes, green hair):1.20)" },
    { kind: "text", clickable: false, text: "," },
    { kind: "text", clickable: false, text: " " },
    { kind: "tag", clickable: true, text: "masterpiece" },
  ]);
});

test("treats brace and bracket weighting as weighted clickable ranges", () => {
  const raw = "{blue eyes}, [green hair], masterpiece";
  const summary = summarize(getPromptClickableSegments(raw), raw);

  assert.deepEqual(summary, [
    { kind: "weighted", clickable: true, text: "{blue eyes}" },
    { kind: "text", clickable: false, text: "," },
    { kind: "text", clickable: false, text: " " },
    { kind: "weighted", clickable: true, text: "[green hair]" },
    { kind: "text", clickable: false, text: "," },
    { kind: "text", clickable: false, text: " " },
    { kind: "tag", clickable: true, text: "masterpiece" },
  ]);
});

test("leaves scheduling and preset syntax inert while keeping normal tags clickable", () => {
  const raw = "<from:0.2>blue eyes</from>, @preset:cinematic, green hair";
  const summary = summarize(getPromptClickableSegments(raw), raw);

  assert.deepEqual(summary, [
    { kind: "text", clickable: false, text: "<from:0.2>blue eyes</from>" },
    { kind: "text", clickable: false, text: "," },
    { kind: "text", clickable: false, text: " " },
    { kind: "text", clickable: false, text: "@preset:cinematic" },
    { kind: "text", clickable: false, text: "," },
    { kind: "text", clickable: false, text: " " },
    { kind: "tag", clickable: true, text: "green hair" },
  ]);
});
