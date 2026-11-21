import assert from "node:assert/strict";
import test from "node:test";
import { parseSegmentDetailPrompt } from "../../src/lib/utils/promptSegmentDetail.ts";

test("parseSegmentDetailPrompt strips SwarmUI //cid=... suffix from segment tags", () => {
  const parsed = parseSegmentDetailPrompt("1girl, smile, <segment:face,0.4,0.3//cid=11> ");
  assert.equal(parsed.baseText, "1girl, smile");
  assert.deepEqual(parsed.segments, [
    { target: "face", creativity: 0.4, threshold: 0.3, prompt: "" },
  ]);
});

test("parseSegmentDetailPrompt handles whitespace before //cid=... and closed form", () => {
  const parsed = parseSegmentDetailPrompt(
    "1girl, <segment:face,0.4,0.3 //cid=11>blue eyes</segment>, outdoor",
  );
  assert.equal(parsed.baseText, "1girl, outdoor");
  assert.deepEqual(parsed.segments, [
    { target: "face", creativity: 0.4, threshold: 0.3, prompt: "blue eyes" },
  ]);
});

test("parseSegmentDetailPrompt handles yolo target with //cid=... suffix", () => {
  const parsed = parseSegmentDetailPrompt(
    "1girl, <segment:yolo-face_yolov8n.pt-1,0.5,0.25//cid=3>",
  );
  assert.equal(parsed.baseText, "1girl");
  assert.deepEqual(parsed.segments, [
    { target: "yolo-face_yolov8n.pt-1", creativity: 0.5, threshold: 0.25, prompt: "" },
  ]);
});

test("parseSegmentDetailPrompt keeps plain tags and invalid ranges behaving as before", () => {
  const plain = parseSegmentDetailPrompt("1girl, <segment:face,0.4,0.3>");
  assert.equal(plain.baseText, "1girl");
  assert.deepEqual(plain.segments, [
    { target: "face", creativity: 0.4, threshold: 0.3, prompt: "" },
  ]);

  const invalid = parseSegmentDetailPrompt("1girl, <segment:face,2.5,0.3>");
  assert.equal(invalid.baseText, "1girl, <segment:face,2.5,0.3>");
  assert.deepEqual(invalid.segments, []);
});
