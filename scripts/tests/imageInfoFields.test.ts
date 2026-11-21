import assert from "node:assert/strict";
import test from "node:test";
import {
  EMPTY_PREFS,
  formatLoraWeight,
  loraDisplayName,
  matchInstalledModel,
  moveField,
  orderFields,
  parseLoraField,
  sanitizePrefs,
  toggleField,
  visibleFields,
} from "../../src/lib/utils/imageInfoFields.ts";

test("the order is the same whatever order the keys arrive in", () => {
  const keys = ["zeta", "seed", "mooshie_x", "loras", "positive_prompt", "date", "model"];
  const expected = ["positive_prompt", "model", "loras", "seed", "date", "mooshie_x", "zeta"];
  assert.deepEqual(orderFields(keys, EMPTY_PREFS), expected);
  assert.deepEqual(orderFields([...keys].reverse(), EMPTY_PREFS), expected);
});

test("the user's order and hidden fields win", () => {
  const prefs = { order: ["seed", "zeta"], hidden: ["date"] };
  const meta = { positive_prompt: "1girl", seed: "1", date: "2026-11-17", zeta: "z", model: "m", vae: "" };
  assert.deepEqual(visibleFields(meta, prefs), ["seed", "zeta", "positive_prompt", "model"]);
});

test("move and toggle", () => {
  const list = orderFields(["positive_prompt", "model", "seed"], EMPTY_PREFS);
  const moved = moveField(list, "seed", -1, EMPTY_PREFS);
  assert.deepEqual(orderFields(["positive_prompt", "model", "seed"], moved), ["positive_prompt", "seed", "model"]);
  assert.equal(moveField(list, "positive_prompt", -1, EMPTY_PREFS), EMPTY_PREFS);
  const hidden = toggleField("model", EMPTY_PREFS);
  assert.deepEqual(hidden.hidden, ["model"]);
  assert.deepEqual(toggleField("model", hidden).hidden, []);
  assert.deepEqual(sanitizePrefs({ order: ["a", 1, "a"], hidden: "x" }), { order: ["a"], hidden: [] });
});

test("LoRAs are split one per entry with their weights", () => {
  const loras = parseLoraField("style/watercolor.safetensors:0.80:0.80, yae_miko_v2.safetensors:1.00:0.50, plain");
  assert.deepEqual(loras, [
    { name: "style/watercolor.safetensors", model: 0.8, clip: 0.8 },
    { name: "yae_miko_v2.safetensors", model: 1, clip: 0.5 },
    { name: "plain", model: null, clip: null },
  ]);
  assert.equal(loraDisplayName(loras[0].name), "watercolor");
  assert.equal(formatLoraWeight(loras[0]), "0.80");
  assert.equal(formatLoraWeight(loras[1]), "1.00 / 0.50");
  assert.equal(formatLoraWeight(loras[2]), null);
  assert.deepEqual(parseLoraField("<lora:detail:-0.5>, <lora:foo:1>"), [
    { name: "detail", model: -0.5, clip: null },
    { name: "foo", model: 1, clip: null },
  ]);
});

test("parseLoraField reads JSON object and string arrays from SwarmUI metadata", () => {
  assert.deepEqual(
    parseLoraField('[{"name":"ctd mofu/ctdmofu","strength_model":0.8,"strength_clip":0.6}]'),
    [{ name: "ctd mofu/ctdmofu", model: 0.8, clip: 0.6 }],
  );
  assert.deepEqual(parseLoraField('["ctd mofu/ctdmofu", "plain"]'), [
    { name: "ctd mofu/ctdmofu", model: null, clip: null },
    { name: "plain", model: null, clip: null },
  ]);
});

test("matchInstalledModel normalizes slashes and extensions against installed files", () => {
  const installedLoras = [
    "ctd mofu\\ctdmofu.safetensors",
    "styles\\dup.safetensors",
    "other\\dup.safetensors",
    "unique_dir\\rare_lora.safetensors",
  ];
  assert.equal(
    matchInstalledModel("ctd mofu/ctdmofu", installedLoras),
    "ctd mofu\\ctdmofu.safetensors",
  );
  assert.equal(
    matchInstalledModel("rare_lora", installedLoras),
    "unique_dir\\rare_lora.safetensors",
  );
  // Ambiguous basename falls back to imported name rather than guessing
  assert.equal(matchInstalledModel("dup", installedLoras), "dup");
  assert.equal(matchInstalledModel("missing_lora", installedLoras), "missing_lora");

  assert.equal(
    matchInstalledModel("redLilyIllu_v10", ["redLilyIllu_v10.safetensors"]),
    "redLilyIllu_v10.safetensors",
  );
});
