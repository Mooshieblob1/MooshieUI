export interface ChangelogRelease {
  version: string;
  heading: string;
  markdown: string;
}

interface ParsedSemver {
  major: number;
  minor: number;
  patch: number;
  pre: string[];
}

const RELEASE_HEADING_RE = /^##\s+((?:What's New in\s+)?v([0-9]+\.[0-9]+\.[0-9]+[^\r\n]*?))\s*$/i;
const SEMVER_RE = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/;
const SEPARATOR_LINE_RE = /^-{3,}$/;

function parseSemver(input: string): ParsedSemver | null {
  const trimmed = input.trim();
  if (!trimmed) return null;
  const match = SEMVER_RE.exec(trimmed);
  if (!match) return null;
  const major = Number(match[1]);
  const minor = Number(match[2]);
  const patch = Number(match[3]);
  if (!Number.isSafeInteger(major) || !Number.isSafeInteger(minor) || !Number.isSafeInteger(patch)) {
    return null;
  }
  const pre = match[4] ? match[4].split(".").filter(Boolean) : [];
  return { major, minor, patch, pre };
}

/** Return whether `version` is a valid `v?major.minor.patch[-pre]` semver string. */
export function isValidSemver(version: string): boolean {
  return parseSemver(version) !== null;
}

function comparePreIdentifiers(a: string[], b: string[]): number {
  if (a.length === 0 && b.length === 0) return 0;
  if (a.length === 0) return 1;
  if (b.length === 0) return -1;

  const len = Math.max(a.length, b.length);
  for (let i = 0; i < len; i++) {
    if (i >= a.length) return -1;
    if (i >= b.length) return 1;
    const idA = a[i];
    const idB = b[i];
    if (idA === idB) continue;

    const numA = /^\d+$/.test(idA) ? Number(idA) : null;
    const numB = /^\d+$/.test(idB) ? Number(idB) : null;
    if (numA !== null && numB !== null) {
      return numA < numB ? -1 : 1;
    }
    if (numA !== null) return -1;
    if (numB !== null) return 1;
    return idA < idB ? -1 : 1;
  }
  return 0;
}

/**
 * Compare two semver strings (`v?major.minor.patch[-pre]`).
 * Returns `< 0` when `a < b`, `0` when equal, and `> 0` when `a > b`.
 */
export function compareSemver(a: string, b: string): number {
  const parsedA = parseSemver(a);
  const parsedB = parseSemver(b);
  if (!parsedA && !parsedB) return 0;
  if (!parsedA) return -1;
  if (!parsedB) return 1;

  if (parsedA.major !== parsedB.major) return parsedA.major - parsedB.major;
  if (parsedA.minor !== parsedB.minor) return parsedA.minor - parsedB.minor;
  if (parsedA.patch !== parsedB.patch) return parsedA.patch - parsedB.patch;
  return comparePreIdentifiers(parsedA.pre, parsedB.pre);
}

function cleanSectionBody(lines: string[]): string {
  let end = lines.length;
  while (end > 0) {
    const trimmed = lines[end - 1].trim();
    if (trimmed === "" || SEPARATOR_LINE_RE.test(trimmed)) {
      end--;
    } else {
      break;
    }
  }
  let start = 0;
  while (start < end && lines[start].trim() === "") {
    start++;
  }
  return lines.slice(start, end).join("\n").trim();
}

/**
 * Parse release sections from `CHANGELOG.md` (newest first, up to `limit`).
 */
export function parseChangelogReleases(source: string, limit = 15): ChangelogRelease[] {
  if (!source || limit <= 0) return [];
  const lines = source.split(/\r?\n/);
  const releases: ChangelogRelease[] = [];

  let currentHeading: string | null = null;
  let currentVersion: string | null = null;
  let currentLines: string[] = [];

  const flush = () => {
    if (currentHeading === null || currentVersion === null) return;
    releases.push({
      version: currentVersion,
      heading: currentHeading,
      markdown: cleanSectionBody(currentLines),
    });
  };

  for (const line of lines) {
    const match = RELEASE_HEADING_RE.exec(line.trimEnd());
    if (match) {
      flush();
      if (releases.length >= limit) {
        currentHeading = null;
        currentVersion = null;
        break;
      }
      currentHeading = match[1].trim();
      const rawVersion = match[2].trim();
      // Keep the version token (e.g. "2.4.6" or "2.4.6-rc.1") cleanly extracted.
      const verToken = /^([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?)/.exec(rawVersion)?.[1] ?? rawVersion;
      currentVersion = verToken;
      currentLines = [];
      continue;
    }

    if (currentHeading !== null) {
      currentLines.push(line);
    }
  }

  if (releases.length < limit) {
    flush();
  }

  return releases.slice(0, limit);
}

/**
 * Select releases to surface on startup given `lastSeen` and `currentVersion`.
 */
export function releasesSince(
  releases: ChangelogRelease[],
  lastSeen: string | null,
  currentVersion: string,
): ChangelogRelease[] {
  if (releases.length === 0 || !isValidSemver(currentVersion)) {
    return [];
  }

  if (!lastSeen || !isValidSemver(lastSeen)) {
    const exact = releases.find((r) => compareSemver(r.version, currentVersion) === 0);
    return exact ? [exact] : [releases[0]];
  }

  if (compareSemver(currentVersion, lastSeen) <= 0) {
    return [];
  }

  return releases
    .filter(
      (r) =>
        compareSemver(r.version, lastSeen) > 0 &&
        compareSemver(r.version, currentVersion) <= 0,
    )
    .sort((a, b) => compareSemver(b.version, a.version));
}
