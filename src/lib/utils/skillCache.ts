/**
 * How a model-authored skill is stored in localStorage, shared by the V5 and
 * H3 notes. Leaf util: no store, IPC or Svelte import.
 */

/** How long "this model wrote nothing usable" stands before it is asked again. */
export const EMPTY_SKILL_TTL_MS = 7 * 24 * 60 * 60 * 1000;

const EMPTY_PREFIX = "empty@";

/**
 * Decode a stored value. Notes never expire. An empty verdict does: one
 * truncated or malformed answer should not cost a model its notes for good,
 * and a model that really cannot write them is only asked again once a week.
 * A bare `""` is a verdict from before verdicts were dated, so it is stale.
 */
export function readCachedSkill(stored: string | null, now: number): string | null {
  if (stored === null || stored === "") return null;
  if (!stored.startsWith(EMPTY_PREFIX)) return stored;
  const at = Number(stored.slice(EMPTY_PREFIX.length));
  return Number.isFinite(at) && now - at < EMPTY_SKILL_TTL_MS && now >= at ? "" : null;
}

/** What to store for notes, or for an empty verdict reached at `now`. */
export function cachedSkillValue(skill: string, now: number): string {
  return skill || `${EMPTY_PREFIX}${now}`;
}
