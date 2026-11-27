import assert from "node:assert/strict";
import test from "node:test";
import {
  NAI_EFFORT_HIGH,
  NAI_EFFORT_MEDIUM,
  NAI_REF_CHARACTER,
  NAI_REF_CHARACTER_MAIN_OUTFIT,
  NAI_REF_CHARACTER_STYLE,
  NAI_REF_CHARACTER_STYLE_ANY_OUTFIT,
  NAI_REF_OUTFIT,
  NAI_REF_STYLE,
  NOVELAI_MODELS,
  effectiveNovelAiModelForAction,
  findNovelAiModel,
  novelAiActiveDirectorReferenceCount,
  novelAiActiveDirectorReferenceSummary,
  novelAiEffortForcedByPreciseReference,
  novelAiReferenceBaseType,
  novelAiReferenceLabelKey,
  novelAiReferencePreferSameOutfit,
  novelAiReferenceSupportsSameOutfit,
  novelAiSetPreferSameOutfit,
  novelAiUsesMediumEffort,
} from "../../src/lib/utils/novelaiModels.ts";
import { estimateNovelAiCost } from "../../src/lib/utils/novelaiCost.ts";

test("NOVELAI_MODELS capability flags match V5, V4.5, and V4 Precise Reference rules", () => {
  assert.equal(NOVELAI_MODELS.length, 4);

  for (const id of ["nai-diffusion-5-full", "nai-diffusion-5-curated"]) {
    const model = findNovelAiModel(id);
    assert.ok(model, id);
    assert.equal(model.preciseReference, true);
    assert.equal(model.preciseReferenceInpainting, false);
    assert.equal(model.preciseReferenceFidelity, false);
    assert.equal(model.preciseReferenceStrengthMax, 5);
    assert.equal(model.outfitReferences, true);
    assert.equal(model.vibeTransfer, false);
  }

  const v45 = findNovelAiModel("nai-diffusion-4-5-full");
  assert.ok(v45);
  assert.equal(v45.preciseReference, true);
  assert.equal(v45.preciseReferenceInpainting, true);
  assert.equal(v45.preciseReferenceFidelity, true);
  assert.equal(v45.preciseReferenceStrengthMax, 1);
  assert.equal(v45.outfitReferences, false);
  assert.equal(v45.vibeTransfer, true);

  const v4 = findNovelAiModel("nai-diffusion-4-full");
  assert.ok(v4);
  assert.equal(v4.preciseReference, false);
  assert.equal(v4.preciseReferenceInpainting, false);
  assert.equal(v4.preciseReferenceFidelity, false);
  assert.equal(v4.preciseReferenceStrengthMax, 1);
  assert.equal(v4.outfitReferences, false);
  assert.equal(v4.vibeTransfer, true);
});

test("Precise Reference description helpers handle all six wire strings", () => {
  assert.equal(novelAiReferenceBaseType(NAI_REF_CHARACTER), NAI_REF_CHARACTER);
  assert.equal(novelAiReferenceBaseType(NAI_REF_CHARACTER_MAIN_OUTFIT), NAI_REF_CHARACTER);
  assert.equal(novelAiReferenceBaseType(NAI_REF_STYLE), NAI_REF_STYLE);
  assert.equal(novelAiReferenceBaseType(NAI_REF_OUTFIT), NAI_REF_OUTFIT);
  assert.equal(novelAiReferenceBaseType(NAI_REF_CHARACTER_STYLE), NAI_REF_CHARACTER_STYLE);
  assert.equal(novelAiReferenceBaseType(NAI_REF_CHARACTER_STYLE_ANY_OUTFIT), NAI_REF_CHARACTER_STYLE);

  assert.equal(novelAiReferenceSupportsSameOutfit(NAI_REF_CHARACTER), true);
  assert.equal(novelAiReferenceSupportsSameOutfit(NAI_REF_CHARACTER_MAIN_OUTFIT), true);
  assert.equal(novelAiReferenceSupportsSameOutfit(NAI_REF_CHARACTER_STYLE), true);
  assert.equal(novelAiReferenceSupportsSameOutfit(NAI_REF_CHARACTER_STYLE_ANY_OUTFIT), true);
  assert.equal(novelAiReferenceSupportsSameOutfit(NAI_REF_STYLE), false);
  assert.equal(novelAiReferenceSupportsSameOutfit(NAI_REF_OUTFIT), false);

  assert.equal(novelAiReferencePreferSameOutfit(NAI_REF_CHARACTER), false);
  assert.equal(novelAiReferencePreferSameOutfit(NAI_REF_CHARACTER_MAIN_OUTFIT), true);
  assert.equal(novelAiReferencePreferSameOutfit(NAI_REF_CHARACTER_STYLE), true);
  assert.equal(novelAiReferencePreferSameOutfit(NAI_REF_CHARACTER_STYLE_ANY_OUTFIT), false);
  assert.equal(novelAiReferencePreferSameOutfit(NAI_REF_STYLE), false);
  assert.equal(novelAiReferencePreferSameOutfit(NAI_REF_OUTFIT), false);

  assert.equal(novelAiSetPreferSameOutfit(NAI_REF_CHARACTER, true), NAI_REF_CHARACTER_MAIN_OUTFIT);
  assert.equal(novelAiSetPreferSameOutfit(NAI_REF_CHARACTER_MAIN_OUTFIT, false), NAI_REF_CHARACTER);
  assert.equal(novelAiSetPreferSameOutfit(NAI_REF_CHARACTER_STYLE, false), NAI_REF_CHARACTER_STYLE_ANY_OUTFIT);
  assert.equal(novelAiSetPreferSameOutfit(NAI_REF_CHARACTER_STYLE_ANY_OUTFIT, true), NAI_REF_CHARACTER_STYLE);
  assert.equal(novelAiSetPreferSameOutfit(NAI_REF_STYLE, true), NAI_REF_STYLE);
  assert.equal(novelAiSetPreferSameOutfit(NAI_REF_OUTFIT, true), NAI_REF_OUTFIT);
});

test("active reference counting respects V5 infill drop and V4.5 outfit skip", () => {
  const v5 = findNovelAiModel("nai-diffusion-5-full");
  const v45 = findNovelAiModel("nai-diffusion-4-5-full");
  const v4 = findNovelAiModel("nai-diffusion-4-full");

  const refs = [
    { image: "img1", description: NAI_REF_CHARACTER },
    { image: "img2", description: NAI_REF_OUTFIT },
    { image: "", description: NAI_REF_STYLE },
  ];

  assert.equal(novelAiActiveDirectorReferenceCount(v5, refs, false), 2);
  assert.equal(novelAiActiveDirectorReferenceCount(v5, refs, true), 0);
  assert.equal(novelAiActiveDirectorReferenceCount(v45, refs, false), 1);
  assert.equal(novelAiActiveDirectorReferenceCount(v45, refs, true), 1);
  assert.equal(novelAiActiveDirectorReferenceCount(v4, refs, false), 0);
});

test("disabled Precise References are skipped across counts, summaries, and Medium effort forcing", () => {
  const v5Full = findNovelAiModel("nai-diffusion-5-full");
  const refs = [
    { image: "img1", description: NAI_REF_CHARACTER_MAIN_OUTFIT, enabled: false },
    { image: "img2", description: NAI_REF_OUTFIT, enabled: true },
    { image: "img3", description: NAI_REF_STYLE },
  ];

  const summary = novelAiActiveDirectorReferenceSummary(v5Full, refs, false);
  assert.equal(summary.count, 2);
  assert.deepEqual(summary.modes, [NAI_REF_OUTFIT, NAI_REF_STYLE]);
  assert.equal(novelAiReferenceLabelKey(summary.modes[0]), "generation.novelai.reference.desc_outfit");
  assert.equal(novelAiEffortForcedByPreciseReference(v5Full, [refs[0]], false), false);
  assert.equal(novelAiEffortForcedByPreciseReference(v5Full, refs, false), true);
});

test("V5 Full Medium effort is forced to High only when active Precise References apply", () => {
  const v5Full = findNovelAiModel("nai-diffusion-5-full");
  const v5Curated = findNovelAiModel("nai-diffusion-5-curated");
  const refs = [{ image: "img1", description: NAI_REF_OUTFIT }];

  assert.equal(novelAiEffortForcedByPreciseReference(v5Full, [], false), false);
  assert.equal(novelAiEffortForcedByPreciseReference(v5Full, refs, false), true);
  assert.equal(novelAiEffortForcedByPreciseReference(v5Full, refs, true), false);
  assert.equal(novelAiEffortForcedByPreciseReference(v5Curated, refs, false), false);

  assert.equal(novelAiUsesMediumEffort(v5Full, NAI_EFFORT_MEDIUM, false), true);
  assert.equal(novelAiUsesMediumEffort(v5Full, NAI_EFFORT_MEDIUM, true), false);
  assert.equal(novelAiUsesMediumEffort(v5Full, NAI_EFFORT_HIGH, false), false);
});

test("estimateNovelAiCost adds 5 Anlas per active reference per image on V5", () => {
  // Opus covers base image when allowance remains, but Precise Reference is never free.
  assert.equal(
    estimateNovelAiCost({
      width: 832,
      height: 1216,
      steps: 23,
      nSamples: 1,
      strength: 1,
      isOpus: true,
      opusExhausted: false,
      vibeEncodes: 0,
      preciseReferences: 2,
    }),
    10,
  );

  // Exhausted Opus V5 allowance bills base image + 5 per reference per sample.
  const baseOnly = estimateNovelAiCost({
    width: 832,
    height: 1216,
    steps: 23,
    nSamples: 1,
    strength: 1,
    isOpus: true,
    opusExhausted: true,
    vibeEncodes: 0,
    preciseReferences: 0,
  });
  assert.ok(baseOnly > 0);
  assert.equal(
    estimateNovelAiCost({
      width: 832,
      height: 1216,
      steps: 23,
      nSamples: 2,
      strength: 1,
      isOpus: true,
      opusExhausted: true,
      vibeEncodes: 0,
      preciseReferences: 2,
    }),
    baseOnly * 2 + 2 * 2 * 5,
  );
});

test("effectiveNovelAiModelForAction narrows V5 Curated to V4.5 capabilities during infill", () => {
  const v5Curated = findNovelAiModel("nai-diffusion-5-curated");
  assert.ok(v5Curated);
  assert.equal(effectiveNovelAiModelForAction(v5Curated, false), v5Curated);

  const infillModel = effectiveNovelAiModelForAction(v5Curated, true);
  assert.ok(infillModel);
  assert.equal(infillModel.alpha, false);
  assert.equal(infillModel.autoText, false);
  assert.equal(infillModel.maxCharacters, 6);
  assert.equal(infillModel.lightQuality, false);
  assert.equal(infillModel.preciseReferenceInpainting, false);

  const v5Full = findNovelAiModel("nai-diffusion-5-full");
  assert.equal(effectiveNovelAiModelForAction(v5Full, true), v5Full);
});
