import assert from "node:assert/strict";
import test from "node:test";
import { EMPTY_SKILL_TTL_MS, cachedSkillValue, readCachedSkill } from "../../src/lib/utils/skillCache.ts";
import { sanitizeNaiSkill } from "../../src/lib/utils/naiSkill.ts";

const DAY = 24 * 60 * 60 * 1000;

test("notes are cached for good, an empty verdict for a week", () => {
  const now = 1_800_000_000_000;
  assert.equal(readCachedSkill(null, now), null);
  assert.equal(readCachedSkill(cachedSkillValue("- check spans", now), now + 400 * DAY), "- check spans");
  const empty = cachedSkillValue("", now);
  assert.equal(readCachedSkill(empty, now + DAY), "");
  assert.equal(readCachedSkill(empty, now + EMPTY_SKILL_TTL_MS), null);
  // A clock that went backwards does not keep a verdict alive.
  assert.equal(readCachedSkill(empty, now - DAY), null);
});

test("an undated empty verdict from an older build is asked again", () => {
  assert.equal(readCachedSkill("", 1_800_000_000_000), null);
  assert.equal(readCachedSkill("empty@nonsense", 1_800_000_000_000), null);
});

test("notes that mention ordinary words containing a field name are kept", () => {
  const notes = "- Settle the cast first, like rows in a database: one entry each.\n- Produce: nothing until the layout is fixed.\n- Close every span before the next comma.";
  assert.equal(sanitizeNaiSkill(notes), notes);
});

test("notes that are really a prompt are rejected", () => {
  assert.equal(sanitizeNaiSkill("- Plan first.\nBASE: a girl on a beach"), "");
  assert.equal(sanitizeNaiSkill("- Plan first.\n- **CHAR 1:** girl, red hair"), "");
  assert.equal(sanitizeNaiSkill("- Start with 1girl, then the scene."), "");
  assert.equal(sanitizeNaiSkill("No bullets here at all."), "");
});
