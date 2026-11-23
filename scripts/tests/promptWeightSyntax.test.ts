import assert from "node:assert/strict";
import test from "node:test";
import {
  translateNaiWeightSyntax,
  translatePromptWeightSyntax,
} from "../../src/lib/utils/promptWeightSyntax.ts";
import { adjustWeightText } from "../../src/lib/utils/promptWeightAdjust.ts";
import { parseNovelAiChunks } from "../../src/lib/utils/novelaiPngMetadata.ts";
import { getPromptClickableSegments } from "../../src/lib/utils/promptClickableRanges.ts";

const NAI_PROMPT =
  "1girl, 1.2::rain, night::, -1::hat::, {{smile}}, [blurry], 1.234::year 2025,::, 0.6::as109, k7::";

test("a NovelAI prompt goes out to NovelAI exactly as written", () => {
  assert.equal(translatePromptWeightSyntax(NAI_PROMPT, "novelai"), NAI_PROMPT);
});

test("a NovelAI prompt keeps its line layout and Text: block", () => {
  const prompt = "1girl, 1.5::rain::\n\nShe waits at a bus stop.\n\nText:\nMumei";
  assert.equal(translatePromptWeightSyntax(prompt, "novelai"), prompt);
});

test("A1111 weights are left for Rust to convert on the NovelAI path", () => {
  assert.equal(
    translatePromptWeightSyntax("(blue hair:1.2), 1.3::rain::", "novelai"),
    "(blue hair:1.2), 1.3::rain::",
  );
});

test("InvokeAI weights still become A1111 weights on the NovelAI path", () => {
  assert.equal(
    translatePromptWeightSyntax("(blue hair)1.2, smile+, 1.3::rain::", "novelai"),
    "(blue hair:1.2), (smile:1.10), 1.3::rain::",
  );
});

test("ComfyUI is the default backend and gets every syntax translated", () => {
  assert.equal(
    translatePromptWeightSyntax("1.2::rain, night::, {smile}, [blurry], (tag)0.8"),
    "(rain, night:1.20), (smile:1.05), (blurry:0.95), (tag:0.8)",
  );
});

test("a negative NovelAI weight keeps its sign for ComfyUI", () => {
  assert.equal(translateNaiWeightSyntax("-1::hat::"), "(hat:-1.00)");
  assert.equal(translateNaiWeightSyntax("1girl, -0.5::coat::, rain"), "1girl, (coat:-0.50), rain");
  assert.equal(translateNaiWeightSyntax("{-1::hat::}"), "((hat:-1.00):1.05)");
});

test("a hyphen that ends a word is not a minus sign", () => {
  assert.equal(translateNaiWeightSyntax("type-2::tag::"), "type-(tag:2.00)");
});

test("the weight buttons wrap plain text in the selected model's syntax", () => {
  assert.equal(adjustWeightText("blue hair", 0.05), "(blue hair:1.05)");
  assert.equal(adjustWeightText("blue hair", 0.05, { novelai: true }), "1.05::blue hair::");
  assert.equal(adjustWeightText("blue hair", -0.05, { novelai: true }), "0.95::blue hair::");
});

test("a NovelAI span around text ending in a digit is closed safely", () => {
  assert.equal(adjustWeightText("year 2025", 0.05, { novelai: true }), "1.05::year 2025,::");
  // Stepping it again keeps the guard, and stepping back to 1 drops it.
  assert.equal(adjustWeightText("1.05::year 2025,::", 0.05, { novelai: true }), "1.10::year 2025,::");
  assert.equal(adjustWeightText("1.05::year 2025,::", -0.05, { novelai: true }), "year 2025");
});

test("the weight buttons keep the syntax a tag already has", () => {
  assert.equal(adjustWeightText("(blue hair:1.20)", 0.05, { novelai: true }), "(blue hair:1.25)");
  assert.equal(adjustWeightText("1.20::rain, night::", 0.05), "1.25::rain, night::");
  assert.equal(adjustWeightText("1.05::rain::", -0.05), "rain");
});

test("the weight buttons step a negative NovelAI weight", () => {
  assert.equal(adjustWeightText("-1::hat::", 0.05), "-0.95::hat::");
  assert.equal(adjustWeightText("-1::hat::", -0.05), "-1.05::hat::");
  assert.equal(adjustWeightText("0.00::hat::", -0.05), "-0.05::hat::");
  // A1111 weights still stop at zero.
  assert.equal(adjustWeightText("(hat:0.00)", -0.05), "(hat:0.00)");
});

test("a negative NovelAI span is one clickable weighted range", () => {
  const raw = "1girl, -1::hat::";
  const weighted = getPromptClickableSegments(raw).filter((s) => s.kind === "weighted");
  assert.deepEqual(
    weighted.map((s) => raw.slice(s.start, s.end)),
    ["-1::hat::"],
  );
});

test("a NovelAI image is imported in the weight syntax NovelAI wrote", () => {
  const comment = JSON.stringify({
    uc: "ignored when the V4 block is present",
    v4_prompt: {
      caption: {
        base_caption: "2girls, 1.2::rain, night::, {smile}, -1::hat::",
        char_captions: [
          { char_caption: "girl, {{red hair}}, 0.8::coat::", centers: [{ x: 0.25, y: 0.5 }] },
        ],
      },
    },
    v4_negative_prompt: {
      caption: {
        base_caption: "lowres, [blurry], 1.3::bad anatomy::",
        char_captions: [{ char_caption: "1.5::hat::" }],
      },
    },
  });
  const params = parseNovelAiChunks({ Software: "NovelAI", Comment: comment });
  assert.ok(params);
  assert.equal(params.positive_prompt, "2girls, 1.2::rain, night::, {smile}, -1::hat::");
  assert.equal(params.negative_prompt, "lowres, [blurry], 1.3::bad anatomy::");
  const characters = JSON.parse(params.mooshie_novelai_characters);
  assert.equal(characters[0].prompt, "girl, {{red hair}}, 0.8::coat::");
  assert.equal(characters[0].negative_prompt, "1.5::hat::");
});
