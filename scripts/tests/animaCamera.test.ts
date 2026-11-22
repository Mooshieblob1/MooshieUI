import assert from "node:assert/strict";
import test from "node:test";
import fixtures from "./fixtures/animaCamera.json";
import {
  animaCameraData, animaCameraPrompt, appendAnimaCameraPrompt, normalizeAnimaCamera,
  type CameraGroup,
} from "../../src/lib/utils/animaCamera.ts";
import { cameraAnglePhrase, multiAngleLoraToApply, MULTI_ANGLE_LORA } from "../../src/lib/utils/cameraAngle.ts";

// Exported from camera_core.py at Aitha camera v1.0.1, commit 2f6c067.
// These expected prompts come from Python, not the vendored JS implementation.
for (const fixture of fixtures) {
  test(`Python camera parity: ${fixture.name}`, () => {
    const values = fixture.values as Record<string, string | number>;
    const [horizontal_angle, vertical_angle, zoom, roll] = fixture.position;
    const settings = normalizeAnimaCamera({ horizontal_angle, vertical_angle, zoom, roll });
    settings.extremePreset = String(values.extreme_type ?? "None");
    settings.extremeWeight = Number(values.extreme_weight ?? 1);
    for (const group of animaCameraData.extra_groups) {
      const key = group.key as CameraGroup;
      settings.extras[key] = {
        preset: String(values[`${key}_preset`] ?? "None"),
        custom: String(values[`${key}_custom`] ?? ""),
        weight: Number(values[`${key}_weight`] ?? 1),
      };
    }
    assert.equal(animaCameraPrompt(settings), fixture.expected);
  });
}

test("untrusted settings clamp numbers and discard unknown presets", () => {
  const settings = normalizeAnimaCamera({
    horizontal_angle: 360, vertical_angle: -90, zoom: Infinity, roll: -1,
    extremePreset: "removed preset", extremeWeight: 100,
    extras: { lens: { preset: "unknown", custom: 9, weight: -10 } },
  });
  assert.deepEqual(
    [settings.horizontal_angle, settings.vertical_angle, settings.zoom, settings.roll],
    [0, -30, 4, 0],
  );
  assert.equal(settings.extremePreset, "None");
  assert.equal(settings.extremeWeight, 50);
  assert.deepEqual(settings.extras.lens, { preset: "None", custom: "", weight: 0.1 });
  assert.doesNotThrow(() => animaCameraPrompt(normalizeAnimaCamera(null)));
});

test("defaults do not share mutable extras between tabs/profiles", () => {
  const first = normalizeAnimaCamera();
  first.extras.lens.preset = "Custom";
  assert.equal(normalizeAnimaCamera().extras.lens.preset, "None");
});

test("append preserves custom weighted groups and is idempotent", () => {
  const settings = normalizeAnimaCamera();
  settings.extras.lens = { preset: "Custom", custom: "(red hair, blue eyes:1.2)", weight: 1.5 };
  const fragment = animaCameraPrompt(settings);
  const original = "1girl, (smile:1.3)";
  const sent = appendAnimaCameraPrompt(original, fragment);
  assert.equal(sent, `${original}, ${fragment}`);
  assert.equal(appendAnimaCameraPrompt(sent, fragment), sent);
  assert.equal(appendAnimaCameraPrompt(original, fragment), sent);
  assert.equal(appendAnimaCameraPrompt("", fragment), fragment);
  assert.equal(appendAnimaCameraPrompt(original, ""), original);
  assert.ok(sent.includes("((red hair, blue eyes:1.2):1.50)"));
  assert.equal(original, "1girl, (smile:1.3)");
});

test("Anima stays independent of Qwen shot vocabulary and its LoRA trigger", () => {
  assert.ok(!animaCameraPrompt(normalizeAnimaCamera()).includes("<sks>"));
  assert.equal(cameraAnglePhrase(0, 1, 1, true), "<sks> front view eye-level shot medium shot");
  assert.equal(multiAngleLoraToApply(false, [MULTI_ANGLE_LORA.filename], []), null);
  assert.equal(multiAngleLoraToApply(true, [MULTI_ANGLE_LORA.filename], []), MULTI_ANGLE_LORA.filename);
});
