import assert from "node:assert/strict";
import test from "node:test";
import {
  activeRecipeCharacters,
  applyRecipesDeterministic,
  detectRecipeCreationIntent,
  detectRecipeRefineIntent,
  formatRecipeChatSummary,
  matchGenerateTimeRecipes,
  matchTargetRecipeForRefine,
  matchTriggeredRecipes,
  parseGeneralAdaptedPrompt,
  parseRecipeDraft,
  parseRecipesImport,
  recipeCreateUserPrompt,
  recipeDistillSystemPrompt,
  recipePromptFingerprint,
  recipeRefineSystemPrompt,
  renderUserRecipesDirective,
  sanitizeEnhanceRecipes,
  serializeRecipesExport,
  slugifyRecipe,
  stripLeakedRecipeSlugs,
  stripNeverUcTags,
  type EnhanceRecipe,
} from "../../src/lib/utils/enhanceRecipes.ts";

const sampleRecipe = (overrides: Partial<EnhanceRecipe> = {}): EnhanceRecipe => ({
  id: "rcp_1",
  name: "Toned Linea Alba",
  slug: "linea-alba",
  intent: "Toned female midriff with a clearly defined vertical linea alba and no horizontal six-pack lines.",
  include: "toned, abs, midriff, navel",
  avoid: "six pack, washboard abs, bodybuilder",
  neverUc: "abs, muscular",
  context: "Skip on face crops or when the midriff is covered.",
  scope: "character",
  applyMode: "ai",
  triggers: ["linea alba", "vertical ab line"],
  createdAt: 100,
  updatedAt: 100,
  ...overrides,
});

test("slugifyRecipe normalizes names to lowercase hyphenated slugs", () => {
  assert.equal(slugifyRecipe("Toned Linea Alba!"), "toned-linea-alba");
  assert.equal(slugifyRecipe("  --Soft Rim Light__v2  "), "soft-rim-light-v2");
  assert.equal(slugifyRecipe("A".repeat(60)).length, 40);
});

test("sanitizeEnhanceRecipes drops invalid items and preserves backward-compatible neverUc", () => {
  const res = sanitizeEnhanceRecipes([
    null,
    { id: "ok", name: "  Rim Light ", slug: "", intent: " Warm rim light ", scope: "invalid", triggers: ["Rim", "rim", ""] },
    { id: "with_never", name: "Abs", slug: "abs", intent: "Toned", neverUc: " abs, muscular ", scope: "character", triggers: [] },
    { id: "bad", name: "   ", intent: "missing name" },
    { id: "ok", name: "Duplicate ID", intent: "should be dropped" },
  ]);
  assert.equal(res.length, 2);
  assert.equal(res[0].id, "ok");
  assert.equal(res[0].slug, "rim-light");
  assert.equal(res[0].scope, "any");
  assert.equal(res[0].neverUc, "");
  assert.deepEqual(res[0].triggers, ["rim"]);
  assert.equal(res[1].neverUc, "abs, muscular");
});

test("matchTriggeredRecipes matches pinned IDs, #slug, and trigger phrases without duplicates", () => {
  const r1 = sampleRecipe({ id: "r1", slug: "linea-alba", triggers: ["linea alba"] });
  const r2 = sampleRecipe({ id: "r2", name: "Rim Light", slug: "rim-light", triggers: ["backlit rim"] });
  const r3 = sampleRecipe({ id: "r3", name: "Film Grain", slug: "film", triggers: ["grain"] });

  const matched = matchTriggeredRecipes(
    [r1, r2, r3],
    ["draw her on the beach #linea-alba and backlit rim lighting", "keep #linea-alba"],
    new Set(["r2"]),
  );
  assert.deepEqual(
    matched.map((r) => r.id),
    ["r1", "r2"],
  );

  // Word boundaries prevent partial matches like "filmmaker" triggering "film" or "#linea-alba-extra"
  assert.equal(
    matchTriggeredRecipes([r1, r3], ["filmmaker using #linea-alba-extra"], new Set()).length,
    0,
  );
});

test("renderUserRecipesDirective renders NAI and General directives, neverUc, and caps length", () => {
  assert.equal(renderUserRecipesDirective([], "nai"), "");
  const r = sampleRecipe();
  const naiText = renderUserRecipesDirective([r], "nai");
  assert.match(naiText, /USER CONCEPT RECIPE RULES/);
  assert.match(naiText, /V5 tags vs prose separation/);
  assert.match(naiText, /Multi-character targeting/);
  assert.match(naiText, /UC conflict removal and hygiene/);
  assert.match(naiText, /anti-subtlety negatives/);
  assert.match(naiText, /rare anatomical terms/);
  assert.match(naiText, /Form over inked lines/);
  assert.match(naiText, /Never negate in UC: abs, muscular/);
  assert.match(naiText, /USER CONCEPT RECIPE: TONED LINEA ALBA \(#linea-alba\)/);
  assert.match(naiText, /CHAR box/);
  assert.match(naiText, /UC only while this recipe applies/);

  const genText = renderUserRecipesDirective([r], "general");
  assert.match(genText, /USER CONCEPT RECIPE RULES/);
  assert.match(genText, /Integrate this concept naturally/);
  assert.doesNotMatch(genText, /CHAR box/);

  const huge = Array.from({ length: 20 }, (_, i) =>
    sampleRecipe({ id: `r_${i}`, name: `Recipe ${i}`, slug: `recipe-${i}`, intent: "x".repeat(500) }),
  );
  assert.ok(renderUserRecipesDirective(huge, "nai").length <= 6000);
});

test("stripNeverUcTags removes plain, braced, and weighted exact matches while keeping compound tags", () => {
  const r = sampleRecipe({ neverUc: "abs, muscular" });
  assert.equal(stripNeverUcTags("", [r]), "");
  assert.equal(
    stripNeverUcTags("male, abs, {muscular}, [abs], 1.2::abs::, six pack, washboard abs, segmented abs", [r]),
    "male, six pack, washboard abs, segmented abs",
  );
  // Multi-tag weight span: removing first, middle, or last tag keeps `::` balanced
  assert.equal(
    stripNeverUcTags("1.2::abs, six pack::", [r]),
    "1.2::six pack::",
  );
  assert.equal(
    stripNeverUcTags("1.2::six pack, abs::", [r]),
    "1.2::six pack::",
  );
  assert.equal(
    stripNeverUcTags("1.2::male, abs, six pack::", [r]),
    "1.2::male, six pack::",
  );
  // Multi-tag brace span
  assert.equal(
    stripNeverUcTags("{abs, six pack}", [r]),
    "{six pack}",
  );
  // Compound tags containing `abs` or `muscular` are preserved
  assert.equal(
    stripNeverUcTags("horizontal ab cuts, bodybuilder abs, muscular man", [r]),
    "horizontal ab cuts, bodybuilder abs, muscular man",
  );
});

test("stripLeakedRecipeSlugs strips matched #slug tokens without breaking CHAR trailing commas", () => {
  const r = sampleRecipe({ slug: "linea-alba" });
  assert.equal(
    stripLeakedRecipeSlugs("1girl, beach, #linea-alba, standing", [r]),
    "1girl, beach, standing",
  );
  assert.equal(
    stripLeakedRecipeSlugs("1girl, beach, #linea-alba", [r]),
    "1girl, beach",
  );
  // Preserves intentional CHAR trailing commas
  assert.equal(
    stripLeakedRecipeSlugs("girl, blonde hair,\ntoned, #linea-alba,\nstanding", [r]),
    "girl, blonde hair,\ntoned,\nstanding",
  );
  // Leaves unrelated hashtags alone
  assert.equal(
    stripLeakedRecipeSlugs("1girl, #other-tag", [r]),
    "1girl, #other-tag",
  );
});

test("parseRecipeDraft handles raw and fenced JSON including neverUc and rejects invalid output", () => {
  const fenced = "```json\n" + JSON.stringify({
    name: "Soft Linea Alba",
    slug: "Soft Linea Alba",
    scope: "character",
    intent: "Toned 11-line midriff.",
    include: "toned, abs, midriff, navel",
    avoid: "six pack, bodybuilder",
    neverUc: "abs, muscular",
    context: "Drop when covered.",
    triggers: ["linea alba"],
  }) + "\n```";
  const parsed = parseRecipeDraft(fenced);
  assert.ok(parsed);
  assert.equal(parsed.slug, "soft-linea-alba");
  assert.equal(parsed.scope, "character");
  assert.equal(parsed.neverUc, "abs, muscular");
  assert.deepEqual(parsed.triggers, ["linea alba"]);

  assert.equal(parseRecipeDraft("not json at all"), null);
  assert.equal(parseRecipeDraft("{}"), null);
});

test("serializeRecipesExport and parseRecipesImport roundtrip recipes", () => {
  const r = sampleRecipe();
  const json = serializeRecipesExport([r]);
  const imported = parseRecipesImport(json);
  assert.ok(imported);
  assert.equal(imported.length, 1);
  assert.equal(imported[0].name, r.name);
  assert.equal(imported[0].slug, r.slug);
  assert.equal(imported[0].neverUc, r.neverUc);

  const rawArray = parseRecipesImport(JSON.stringify([{ name: "Bare", intent: "Works without id" }]));
  assert.ok(rawArray);
  assert.equal(rawArray.length, 1);
  assert.ok(rawArray[0].id.startsWith("rcp_"));
  assert.equal(rawArray[0].neverUc, "");

  assert.equal(parseRecipesImport("{\"kind\":\"wrong\",\"recipes\":[]}"), null);
});

test("detectRecipeCreationIntent matches explicit recipe requests without false positives", () => {
  for (const yes of [
    "make this into a recipe",
    "Turn the last prompt into a concept recipe",
    "save this as a recipe",
    "create a recipe for toned linea alba without horizontal ab lines",
    "build me a new recipe about soft rim lighting",
  ]) {
    assert.equal(detectRecipeCreationIntent(yes), true, `expected true for: ${yes}`);
  }

  for (const no of [
    "1girl reading a recipe book in a cozy kitchen",
    "she bakes a cake from a recipe",
    "draw her at the beach",
    "",
  ]) {
    assert.equal(detectRecipeCreationIntent(no), false, `expected false for: ${no}`);
  }
});

test("detectRecipeRefineIntent and matchTargetRecipeForRefine resolve recipes accurately", () => {
  assert.equal(detectRecipeRefineIntent("update the linea alba recipe to never negate abs"), true);
  assert.equal(detectRecipeRefineIntent("fix my #linea-alba recipe so it stops drawing a dark line"), true);
  assert.equal(detectRecipeRefineIntent("recipe #linea-alba: drop crease"), true);
  assert.equal(detectRecipeRefineIntent("1girl baking from an old recipe"), false);

  const r1 = sampleRecipe({ id: "r1", name: "Linea Alba", slug: "linea-alba", triggers: ["linea alba"] });
  const r2 = sampleRecipe({ id: "r2", name: "Rim Light", slug: "rim-light", triggers: ["backlit rim"] });

  assert.equal(matchTargetRecipeForRefine("fix #rim-light recipe", [r1, r2], new Set(["r1"]))?.id, "r2");
  assert.equal(matchTargetRecipeForRefine("update Rim Light recipe", [r1, r2], new Set(["r1"]))?.id, "r2");
  assert.equal(matchTargetRecipeForRefine("fix my recipe", [r1, r2], new Set(["r1"]))?.id, "r1");
  assert.equal(matchTargetRecipeForRefine("fix my recipe", [r1, r2], new Set(["r1", "r2"])), null);
  assert.equal(matchTargetRecipeForRefine("fix my recipe", [r1], new Set())?.id, "r1");
});

test("recipeCreateUserPrompt handles description-only, chat-only, and combined inputs", () => {
  const descOnly = recipeCreateUserPrompt("toned midriff with linea alba", []);
  assert.match(descOnly, /Concept Recipe JSON/);
  assert.match(descOnly, /toned midriff with linea alba/);

  const chatOnly = recipeCreateUserPrompt("", [{ user: "toned abs", reply: "ok", prompt: "1girl, toned" }]);
  assert.match(chatOnly, /Distill the recurring concept/);
  assert.match(chatOnly, /Turn 1 User: toned abs/);

  const both = recipeCreateUserPrompt("make this into a recipe", [{ user: "toned abs", reply: "", prompt: "1girl" }]);
  assert.match(both, /Recent conversation:/);
  assert.match(both, /User request:\nmake this into a recipe/);
});

test("formatRecipeChatSummary includes name, slug, target, avoid, and neverUc", () => {
  const summary = formatRecipeChatSummary(sampleRecipe());
  assert.match(summary, /"Toned Linea Alba" \(#linea-alba\)/);
  assert.match(summary, /Target: Toned female midriff/);
  assert.match(summary, /Avoid \/ UC: six pack/);
  assert.match(summary, /Never negate in UC: abs, muscular/);
});

test("sanitizeEnhanceRecipes defaults applyMode to ai and preserves tags", () => {
  const result = sanitizeEnhanceRecipes([
    {
      id: "r1",
      name: "One",
      slug: "one",
      intent: "Intent one",
    },
    {
      id: "r2",
      name: "Two",
      slug: "two",
      intent: "Intent two",
      applyMode: "tags",
    },
    {
      id: "r3",
      name: "Three",
      slug: "three",
      intent: "Intent three",
      applyMode: "invalid-mode",
    },
  ]);
  assert.equal(result.length, 3);
  assert.equal(result[0].applyMode, "ai");
  assert.equal(result[1].applyMode, "tags");
  assert.equal(result[2].applyMode, "ai");
});

test("matchGenerateTimeRecipes matches pinned IDs and #slug mentions without plain triggers", () => {
  const pinned = sampleRecipe({ id: "rcp_pin", slug: "pinned-look", triggers: ["sunlight"] });
  const explicitSlug = sampleRecipe({ id: "rcp_slug", slug: "rim-glow", triggers: ["backlight"] });
  const plainOnly = sampleRecipe({ id: "rcp_plain", slug: "cel-shade", triggers: ["linea alba"] });
  const matched = matchGenerateTimeRecipes(
    [pinned, explicitSlug, plainOnly],
    ["1girl, linea alba, #rim-glow"],
    new Set(["rcp_pin"]),
  );
  assert.deepEqual(
    matched.map((r) => r.id),
    ["rcp_pin", "rcp_slug"],
  );
});

test("applyRecipesDeterministic routes NAI scene and character tags while preserving prose and Text", () => {
  const charRecipe = sampleRecipe({
    id: "rcp_char",
    slug: "linea-alba",
    scope: "character",
    include: "toned, abs, navel",
    avoid: "six pack, bodybuilder",
    neverUc: "abs, muscular",
  });
  const sceneRecipe = sampleRecipe({
    id: "rcp_scene",
    slug: "moody-jungle",
    scope: "scene",
    include: "dramatic shadows, dappled light",
    avoid: "flat lighting",
    neverUc: "shadow",
  });
  const result = applyRecipesDeterministic(
    {
      positive: "1girl, solo, #moody-jungle\n\nShe stands in a sunlit clearing.\n\nText: Keep this line",
      negative: "lowres, abs, shadow",
      characters: [
        {
          prompt: "blonde hair, #linea-alba",
          negative_prompt: "blurry, muscular",
        },
      ],
    },
    [charRecipe, sceneRecipe],
    true,
  );
  assert.ok(!result.positive.includes("#moody-jungle"));
  assert.match(result.positive, /^1girl, solo, dramatic shadows, dappled light\n\nShe stands in a sunlit clearing\.\n\nText: Keep this line$/);
  assert.ok(!result.negative.includes("abs"));
  assert.ok(!result.negative.includes("shadow"));
  assert.match(result.negative, /six pack/);
  assert.match(result.negative, /flat lighting/);
  assert.equal(result.characters?.length, 1);
  assert.ok(!result.characters![0].prompt.includes("#linea-alba"));
  assert.match(result.characters![0].prompt, /toned/);
  assert.match(result.characters![0].prompt, /navel/);
  assert.ok(!result.characters![0].negative_prompt.includes("muscular"));
  assert.match(result.characters![0].negative_prompt, /blurry/);
});

test("applyRecipesDeterministic merges tags cleanly in General mode", () => {
  const recipe = sampleRecipe({
    scope: "character",
    include: "toned, navel",
    avoid: "six pack",
    neverUc: "abs, muscular",
  });
  const result = applyRecipesDeterministic(
    {
      positive: "1girl, standing, #linea-alba",
      negative: "lowres, abs, muscular",
    },
    [recipe],
    false,
  );
  assert.ok(!result.positive.includes("#linea-alba"));
  assert.match(result.positive, /1girl, standing, toned, navel/);
  assert.ok(!result.negative.includes("abs"));
  assert.ok(!result.negative.includes("muscular"));
  assert.match(result.negative, /six pack/);
});

test("activeRecipeCharacters filters disabled and empty slots while preserving original indices", () => {
  const slots = activeRecipeCharacters([
    { prompt: "   ", negative_prompt: "  ", enabled: true },
    { prompt: "disabled char", negative_prompt: "hat", enabled: false },
    { prompt: " blonde hair ", negative_prompt: " short hair ", enabled: true },
    { prompt: "", negative_prompt: "glasses" },
  ]);
  assert.deepEqual(slots, [
    { index: 2, prompt: "blonde hair", negative_prompt: "short hair" },
    { index: 3, prompt: "", negative_prompt: "glasses" },
  ]);
});

test("applyRecipesDeterministic leaves enabled: false character slots untouched", () => {
  const recipe = sampleRecipe({
    scope: "character",
    include: "toned, navel",
    neverUc: "abs, muscular",
  });
  const result = applyRecipesDeterministic(
    {
      positive: "1girl, solo",
      negative: "lowres",
      characters: [
        { prompt: "disabled, #linea-alba", negative_prompt: "abs", enabled: false },
        { prompt: "blonde hair", negative_prompt: "muscular", enabled: true },
      ],
    },
    [recipe],
    true,
  );
  assert.equal(result.characters?.length, 2);
  assert.equal(result.characters![0].prompt, "disabled, #linea-alba");
  assert.equal(result.characters![0].negative_prompt, "abs");
  assert.equal(result.characters![1].prompt, "blonde hair, toned, navel");
  assert.equal(result.characters![1].negative_prompt, "");
});

test("recipePromptFingerprint is prompt-only, ignores coords/blank/disabled slots, and invalidates on prompt edits", () => {
  const baseRecipe = sampleRecipe();
  const baseFields = {
    positive: "1girl, solo",
    negative: "lowres",
    characters: [{ prompt: "blonde hair", negative_prompt: "short hair", enabled: true }],
  };
  const fpA = recipePromptFingerprint(baseFields, [baseRecipe]);
  const fpSame = recipePromptFingerprint(
    {
      positive: "  1girl, solo  ",
      negative: "lowres",
      characters: [
        { prompt: "   ", negative_prompt: "", enabled: true },
        { prompt: "hidden char", negative_prompt: "abs", enabled: false },
        // Extra fields like center coordinates are ignored by fingerprinting.
        { prompt: "blonde hair", negative_prompt: "short hair", enabled: true, center: { x: 0.1, y: 0.9 } } as {
          prompt: string;
          negative_prompt: string;
          enabled: boolean;
        },
      ],
    },
    [sampleRecipe()],
  );
  assert.equal(fpA, fpSame);
  assert.notEqual(
    fpA,
    recipePromptFingerprint(
      {
        ...baseFields,
        characters: [
          { prompt: "blonde hair", negative_prompt: "short hair", enabled: true },
          { prompt: "second char", negative_prompt: "", enabled: true },
        ],
      },
      [baseRecipe],
    ),
  );
  assert.notEqual(
    fpA,
    recipePromptFingerprint({ ...baseFields, positive: "1girl, park" }, [baseRecipe]),
  );
  assert.notEqual(
    fpA,
    recipePromptFingerprint({ ...baseFields, negative: "worst quality" }, [baseRecipe]),
  );
  assert.notEqual(
    fpA,
    recipePromptFingerprint(
      { ...baseFields, characters: [{ prompt: "black hair", negative_prompt: "short hair" }] },
      [baseRecipe],
    ),
  );
  assert.notEqual(
    fpA,
    recipePromptFingerprint(baseFields, [sampleRecipe({ updatedAt: 200 })]),
  );
  assert.notEqual(
    fpA,
    recipePromptFingerprint(baseFields, [sampleRecipe({ applyMode: "tags" })]),
  );
});

test("parseGeneralAdaptedPrompt parses JSON objects and rejects invalid output", () => {
  assert.deepEqual(
    parseGeneralAdaptedPrompt('{"positive":"1girl, dappled light","negative":"flat lighting"}'),
    { positive: "1girl, dappled light", negative: "flat lighting" },
  );
  assert.deepEqual(
    parseGeneralAdaptedPrompt('```json\n{"positive":"1girl, rim light","negative":""}\n```'),
    { positive: "1girl, rim light", negative: "" },
  );
  assert.equal(parseGeneralAdaptedPrompt('{"positive":"   ","negative":"lowres"}'), null);
  assert.equal(parseGeneralAdaptedPrompt("not json"), null);
});

test("authoring and refining system prompts include anti-over-negation and real-tag guardrails", () => {
  const distill = recipeDistillSystemPrompt();
  assert.match(distill, /"neverUc"/);
  assert.match(distill, /anti-subtlety negatives/i);
  assert.match(distill, /pseudo-tags/i);
  assert.match(distill, /heavy numeric weights/i);
  assert.match(distill, /Form over inked lines/i);
  assert.match(distill, /hedging words/i);

  const refine = recipeRefineSystemPrompt();
  assert.match(refine, /"neverUc"/);
  assert.match(refine, /anti-subtlety terms/i);
  assert.match(refine, /pseudo-tags/i);
  assert.match(refine, /heavy numeric weights/i);
  assert.match(refine, /literal line\/cut words/i);
  assert.match(refine, /lighting\/shading cues/i);
});
