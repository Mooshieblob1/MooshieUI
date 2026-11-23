import assert from "node:assert/strict";
import test from "node:test";
import {
  compareSemver,
  isValidSemver,
  parseChangelogReleases,
  releasesSince,
  type ChangelogRelease,
} from "../../src/lib/utils/changelog.ts";

const SAMPLE_CHANGELOG = `# Changelog

## What's New in v2.4.6

### Prompt Assistant
- **Per-tab Enhance chat**: Scoped per generation tab.

---

## What's New in v2.4.5

### Outpainting
- **Dedicated Outpaint mode**: Added padding controls.

---

## What's New in v2.4.4-rc.1

### Fixes
- Fixed startup timing.

---

## What's New in v2.4.3

### Character Extract
- Better grounding.
`;

test("parseChangelogReleases splits releases, strips trailing separators, and respects limit", () => {
  const all = parseChangelogReleases(SAMPLE_CHANGELOG);
  assert.equal(all.length, 4);
  assert.deepEqual(
    all.map((r) => r.version),
    ["2.4.6", "2.4.5", "2.4.4-rc.1", "2.4.3"],
  );
  assert.equal(all[0].heading, "What's New in v2.4.6");
  assert.equal(
    all[0].markdown,
    "### Prompt Assistant\n- **Per-tab Enhance chat**: Scoped per generation tab.",
  );
  assert.ok(!all[0].markdown.includes("---"));

  const limited = parseChangelogReleases(SAMPLE_CHANGELOG, 2);
  assert.equal(limited.length, 2);
  assert.deepEqual(
    limited.map((r) => r.version),
    ["2.4.6", "2.4.5"],
  );
});

test("parseChangelogReleases handles CRLF line endings and non-release headings", () => {
  const crlf = "# Changelog\r\n\r\n## Unreleased\r\n\r\n## What's New in v1.2.3\r\n\r\n- Item A\r\n\r\n---\r\n";
  const parsed = parseChangelogReleases(crlf);
  assert.equal(parsed.length, 1);
  assert.equal(parsed[0].version, "1.2.3");
  assert.equal(parsed[0].markdown, "- Item A");
});

test("compareSemver compares major, minor, patch, and prerelease versions", () => {
  assert.equal(compareSemver("v2.4.6", "2.4.6"), 0);
  assert.ok(compareSemver("2.4.10", "2.4.6") > 0);
  assert.ok(compareSemver("2.5.0", "2.4.99") > 0);
  assert.ok(compareSemver("3.0.0", "2.99.99") > 0);
  assert.ok(compareSemver("2.4.6-rc.1", "2.4.6") < 0);
  assert.ok(compareSemver("2.4.6-rc.2", "2.4.6-rc.1") > 0);
  assert.ok(compareSemver("2.4.6-alpha", "2.4.6-beta") < 0);
  assert.ok(compareSemver("2.4.6-1", "2.4.6-alpha") < 0);
  assert.equal(isValidSemver("2.4.6"), true);
  assert.equal(isValidSemver("v2.4.6-rc.1"), true);
  assert.equal(isValidSemver("dev"), false);
  assert.equal(isValidSemver(""), false);
});

test("releasesSince handles first run, upgrades, multi-version jumps, and downgrades", () => {
  const releases: ChangelogRelease[] = [
    { version: "2.4.6", heading: "What's New in v2.4.6", markdown: "- 2.4.6" },
    { version: "2.4.5", heading: "What's New in v2.4.5", markdown: "- 2.4.5" },
    { version: "2.4.4", heading: "What's New in v2.4.4", markdown: "- 2.4.4" },
    { version: "2.4.3", heading: "What's New in v2.4.3", markdown: "- 2.4.3" },
  ];

  // First run (!lastSeen): exact match for currentVersion
  assert.deepEqual(
    releasesSince(releases, null, "2.4.5").map((r) => r.version),
    ["2.4.5"],
  );

  // First run when currentVersion is newer than changelog: fallback to newest entry
  assert.deepEqual(
    releasesSince(releases, null, "2.4.7").map((r) => r.version),
    ["2.4.6"],
  );

  // Same version or downgrade: empty
  assert.deepEqual(releasesSince(releases, "2.4.6", "2.4.6"), []);
  assert.deepEqual(releasesSince(releases, "2.4.6", "2.4.5"), []);

  // Multi-version jump: all intermediate releases newest-first
  assert.deepEqual(
    releasesSince(releases, "2.4.3", "2.4.6").map((r) => r.version),
    ["2.4.6", "2.4.5", "2.4.4"],
  );

  // Invalid / dev currentVersion or empty releases: empty
  assert.deepEqual(releasesSince(releases, null, "dev"), []);
  assert.deepEqual(releasesSince([], null, "2.4.6"), []);
});
