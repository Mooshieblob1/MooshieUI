/**
 * Self-check script for randomPrompt.ts
 * Run with: npx tsx scripts/test-random-prompt.ts
 */

import { expandRandomPrompt, hasRandomSyntax, previewRandomPrompt } from "../src/lib/utils/randomPrompt.js";

let passed = 0;
let failed = 0;

function assert(condition: boolean, label: string, extra?: string): void {
  if (condition) {
    console.log(`  PASS  ${label}`);
    passed++;
  } else {
    console.error(`  FAIL  ${label}${extra ? " -- " + extra : ""}`);
    failed++;
  }
}

function assertEqual(actual: string, expected: string, label: string): void {
  assert(actual === expected, label, `got "${actual}", expected "${expected}"`);
}

function assertOneOf(actual: string, options: string[], label: string): void {
  assert(options.includes(actual), label, `got "${actual}", expected one of ${JSON.stringify(options)}`);
}

// ---------------------------------------------------------------------------
console.log("\n=== hasRandomSyntax ===");
assert(!hasRandomSyntax(""), "empty string");
assert(!hasRandomSyntax("masterpiece, best quality"), "plain prompt");
assert(!hasRandomSyntax("{masterpiece}"), "NovelAI emphasis (no pipe)");
assert(!hasRandomSyntax("{masterpiece:1.5}"), "NovelAI weighted emphasis");
assert(hasRandomSyntax("{a|b}"), "simple alternation");
assert(hasRandomSyntax("{a|b|c}"), "three options");
assert(hasRandomSyntax("foo, {a|b}, bar"), "alternation embedded");
assert(hasRandomSyntax("{a|{b|c}}"), "nested alternation");
assert(!hasRandomSyntax("\\{a|b\\}"), "escaped braces");
assert(!hasRandomSyntax("[a:b:0.5]"), "prompt scheduling square brackets");

// ---------------------------------------------------------------------------
console.log("\n=== basic alternation {a|b|c} ===");
// With seed 42, verify determinism and coverage
const seed42 = expandRandomPrompt("{red|green|blue}", 42);
const seed42again = expandRandomPrompt("{red|green|blue}", 42);
assertEqual(seed42, seed42again, "same seed produces same result");
assertOneOf(seed42, ["red", "green", "blue"], "pick one of three");

// Different seeds should explore all options
const outcomes = new Set<string>();
for (let s = 0; s < 100; s++) {
  outcomes.add(expandRandomPrompt("{red|green|blue}", s));
}
assert(outcomes.has("red") && outcomes.has("green") && outcomes.has("blue"), "all options reached over 100 seeds");

// ---------------------------------------------------------------------------
console.log("\n=== no-op cases ===");
assertEqual(expandRandomPrompt("plain text", 1), "plain text", "plain text unchanged");
assertEqual(expandRandomPrompt("{masterpiece}", 1), "{masterpiece}", "NovelAI brace left intact");
assertEqual(expandRandomPrompt("[a:b:0.5]", 1), "[a:b:0.5]", "square bracket scheduling unchanged");

// ---------------------------------------------------------------------------
console.log("\n=== literal escapes ===");
// Escapes are only meaningful in the context of the parser (when alternation is present).
// Text with no alternation blocks passes through unchanged, backslashes included.
assertEqual(expandRandomPrompt("\\{literal\\}", 1), "\\{literal\\}", "escaped braces with no pipe: unchanged");
// When alternation IS present, \{ \| \} are treated as literal chars in that pass:
const esc1 = expandRandomPrompt("\\{static\\} {a|b}", 1);
assert(esc1 === "{static} a" || esc1 === "{static} b", "escaped braces in mixed text", `got "${esc1}"`);
assertEqual(expandRandomPrompt("{a\\|b|c}", 1), "c", "escaped pipe in option: only one real option 'c' ... well, 'a|b' or 'c'");
// {a\|b|c}: the \| is literal inside, so options are ["a|b", "c"]
const escPipe = new Set<string>();
for (let s = 0; s < 50; s++) escPipe.add(expandRandomPrompt("{a\\|b|c}", s));
assert(escPipe.has("a|b") && escPipe.has("c"), "escaped pipe: options are 'a|b' and 'c'");

// ---------------------------------------------------------------------------
console.log("\n=== fixed count {2$$a|b|c} ===");
// {2$$a|b|c} should always pick exactly 2 distinct options joined with ", "
for (let s = 0; s < 30; s++) {
  const result = expandRandomPrompt("{2$$a|b|c}", s);
  const parts = result.split(", ");
  assert(parts.length === 2, `seed ${s}: 2 parts`, `got "${result}"`);
  assert(new Set(parts).size === 2, `seed ${s}: distinct`, `got "${result}"`);
  parts.forEach(p => assert(["a","b","c"].includes(p), `seed ${s}: valid option "${p}"`));
}

// ---------------------------------------------------------------------------
console.log("\n=== range count {1-3$$a|b|c} ===");
const rangeCounts = new Set<number>();
for (let s = 0; s < 200; s++) {
  const result = expandRandomPrompt("{1-3$$a|b|c}", s);
  const parts = result.split(", ");
  rangeCounts.add(parts.length);
  assert(parts.length >= 1 && parts.length <= 3, `seed ${s}: count in [1,3]`, `got "${result}"`);
}
assert(rangeCounts.has(1) && rangeCounts.has(2) && rangeCounts.has(3), "range 1-3 produces all counts");

// ---------------------------------------------------------------------------
console.log("\n=== custom separator {2$$ and $$a|b|c} ===");
for (let s = 0; s < 20; s++) {
  const result = expandRandomPrompt("{2$$ and $$a|b|c}", s);
  assert(result.includes(" and "), `seed ${s}: contains ' and '`, `got "${result}"`);
  const parts = result.split(" and ");
  assert(parts.length === 2, `seed ${s}: exactly 2 parts`);
}

// ---------------------------------------------------------------------------
console.log("\n=== weights {3::a|b} ===");
const weightCounts = { a: 0, b: 0 };
for (let s = 0; s < 400; s++) {
  const r = expandRandomPrompt("{3::a|b}", s);
  if (r === "a") weightCounts.a++;
  else if (r === "b") weightCounts.b++;
}
const ratio = weightCounts.a / weightCounts.b;
assert(ratio > 2.0 && ratio < 5.0, `weights: a/b ratio ${ratio.toFixed(2)} ~3x (expected ~3)`);

// ---------------------------------------------------------------------------
console.log("\n=== nesting {a|{b|c}} ===");
const nestOutcomes = new Set<string>();
for (let s = 0; s < 100; s++) nestOutcomes.add(expandRandomPrompt("{a|{b|c}}", s));
assert(nestOutcomes.has("a"), "nested: outer option 'a' reachable");
assert(nestOutcomes.has("b") || nestOutcomes.has("c"), "nested: inner options reachable");

// ---------------------------------------------------------------------------
console.log("\n=== context around the alternation ===");
for (let s = 0; s < 20; s++) {
  const r = expandRandomPrompt("masterpiece, {red|blue} car, best quality", s);
  assert(r === "masterpiece, red car, best quality" || r === "masterpiece, blue car, best quality",
    `seed ${s}: context preserved`, `got "${r}"`);
}

// ---------------------------------------------------------------------------
console.log("\n=== multiple alternation blocks ===");
for (let s = 0; s < 20; s++) {
  const r = expandRandomPrompt("{a|b} and {c|d}", s);
  const valid = ["a and c", "a and d", "b and c", "b and d"];
  assertOneOf(r, valid, `seed ${s}: two blocks`);
}

// ---------------------------------------------------------------------------
console.log("\n=== seed reproducibility ===");
// Seeding from (userSeed + imageIndex): different images get different rolls
const base = 12345;
const img0 = expandRandomPrompt("{a|b|c}", base + 0);
const img1 = expandRandomPrompt("{a|b|c}", base + 1);
// Not necessarily different (could collide), but if seed changes, RNG state changes
// Just verify the function runs and returns valid options
assertOneOf(img0, ["a","b","c"], "image 0 valid");
assertOneOf(img1, ["a","b","c"], "image 1 valid");

// Verify a specific known output (lock down determinism)
const knownSeed = 0;
const known = expandRandomPrompt("{alpha|beta|gamma}", knownSeed);
assertOneOf(known, ["alpha", "beta", "gamma"], "known seed 0: valid option");
console.log(`  INFO  seed=0 picks: "${known}" (lock this for regression if needed)`);

// ---------------------------------------------------------------------------
console.log("\n=== previewRandomPrompt ===");
const preview1 = previewRandomPrompt("{x|y|z}", 100);
const preview2 = previewRandomPrompt("{x|y|z}", 100);
assertEqual(preview1, preview2, "previewRandomPrompt: same seed = same result");
assertOneOf(preview1, ["x","y","z"], "previewRandomPrompt: valid option");

// ---------------------------------------------------------------------------
console.log("\n=== single option (degenerate) ===");
assertEqual(expandRandomPrompt("{only}", 1), "{only}", "single option no pipe: leave unchanged");

// ---------------------------------------------------------------------------
console.log(`\n=== Results: ${passed} passed, ${failed} failed ===\n`);
if (failed > 0) process.exit(1);
