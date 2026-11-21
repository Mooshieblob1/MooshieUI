import assert from "node:assert/strict";
import test from "node:test";
import { mergeComfyTags, sanitizePromptForSend } from "../../src/lib/utils/promptSanitize.ts";

const twoGirls = "2girls, hatsune miku, long hair, smile, kagamine rin, long hair, smile";

test("quality tags in front keep every repeated character tag", () => {
  assert.equal(
    mergeComfyTags(twoGirls, "masterpiece, best quality", "before"),
    `masterpiece, best quality, ${twoGirls}`,
  );
});

test("a fragment after the prompt keeps the prompt's repeats too", () => {
  assert.equal(mergeComfyTags(twoGirls, "ciloranko", "after"), `${twoGirls}, ciloranko`);
});

test("incoming tags the prompt already has are skipped, any case", () => {
  assert.equal(
    mergeComfyTags("1girl, Best Quality, smile", "masterpiece, best quality", "before"),
    "masterpiece, 1girl, Best Quality, smile",
  );
});

test("repeats inside the incoming fragment collapse to one", () => {
  assert.equal(mergeComfyTags("1girl", "worst quality, lowres, worst quality", "after"), "1girl, worst quality, lowres");
});

test("nothing new leaves the prompt byte for byte", () => {
  assert.equal(mergeComfyTags("1girl ,  smile", "smile", "after"), "1girl ,  smile");
  assert.equal(mergeComfyTags("1girl ,  smile", "", "before"), "1girl ,  smile");
});

test("an empty prompt becomes the fragment", () => {
  assert.equal(mergeComfyTags("", "masterpiece, best quality", "before"), "masterpiece, best quality");
});

test("sanitizePromptForSend strips BREAK by default and preserves it when keepBreak is true", () => {
  const raw = "1girl, blue hair\nBREAK\nbeach, <break> sunset, BREAKFAST";
  assert.equal(sanitizePromptForSend(raw), "1girl, blue hair, beach,   sunset, BREAKFAST");
  assert.equal(
    sanitizePromptForSend(raw, { keepBreak: true }),
    "1girl, blue hair, BREAK, beach,  BREAK  sunset, BREAKFAST",
  );
  assert.equal(
    mergeComfyTags(sanitizePromptForSend("1girl\nBREAK\n2girls", { keepBreak: true }), "masterpiece", "before"),
    "masterpiece, 1girl, BREAK, 2girls",
  );
});
