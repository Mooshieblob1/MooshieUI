/**
 * Random prompt alternation syntax expander.
 *
 * Supported syntax (Dynamic-Prompts compatible subset):
 *   {a|b|c}               - picks one option uniformly at random
 *   {2$$a|b|c}            - picks 2 distinct options joined with ", "
 *   {1-3$$a|b|c}          - picks a random count in [1, 3] distinct options
 *   {2$$ and $$a|b|c}     - picks 2 options joined with " and "
 *   {3::a|2::b|c}         - weighted pick (a is 3x, b is 2x, c is 1x likely)
 *   {a|{b|c}}             - nesting supported; inner groups expand first
 *   \{, \|, \}            - literal brace / pipe escapes
 *
 * Braces without a top-level "|" are left untouched so NovelAI emphasis
 * syntax ({tag}) and prompt scheduling ([a:b:0.5]) pass through unchanged.
 *
 * RNG: splitmix32 seeded by (seed + imageIndex).  The same seed always
 * produces the same expansion, so gallery metadata can be reproduced.
 */

// ---------------------------------------------------------------------------
// PRNG
// ---------------------------------------------------------------------------

/** Create a deterministic splitmix32 generator seeded by `seed`. */
function makeSplitmix32(seed: number): () => number {
  let s = seed >>> 0;
  return (): number => {
    s = (s + 0x9e3779b9) >>> 0;
    let z = s;
    z = (Math.imul(z ^ (z >>> 16), 0x85ebca6b)) >>> 0;
    z = (Math.imul(z ^ (z >>> 13), 0xc2b2ae35)) >>> 0;
    return (z ^ (z >>> 16)) >>> 0;
  };
}

/** Return a float in [0, 1). */
function randFloat(rng: () => number): number {
  return rng() / 0x100000000;
}

/** Return an integer in [min, max] inclusive. */
function randInt(rng: () => number, min: number, max: number): number {
  if (min >= max) return min;
  return min + (rng() % (max - min + 1));
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/** Check if `s` has at least one "|" at brace-depth 0 (top-level). */
function hasTopLevelPipe(s: string): boolean {
  let depth = 0;
  for (let i = 0; i < s.length; i++) {
    if (s[i] === "\\" && i + 1 < s.length) { i++; continue; }
    if (s[i] === "{") { depth++; continue; }
    if (s[i] === "}") { depth = Math.max(0, depth - 1); continue; }
    if (s[i] === "|" && depth === 0) return true;
  }
  return false;
}

/**
 * Split `s` by top-level "|" (not inside nested braces, respecting escapes).
 * Always returns at least one element.
 */
function splitTopLevelPipes(s: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < s.length; i++) {
    if (s[i] === "\\" && i + 1 < s.length) { i++; continue; }
    if (s[i] === "{") { depth++; continue; }
    if (s[i] === "}") { depth = Math.max(0, depth - 1); continue; }
    if (s[i] === "|" && depth === 0) {
      parts.push(s.slice(start, i));
      start = i + 1;
    }
  }
  parts.push(s.slice(start));
  return parts;
}

/**
 * Find the byte offset of `$$` in `s` at depth 0, stopping (returning -1) if
 * a top-level `|` is found first.  Used to locate the optional separator spec.
 */
function findTopLevelDollarDollar(s: string): number {
  let depth = 0;
  for (let i = 0; i < s.length - 1; i++) {
    if (s[i] === "\\" && i + 1 < s.length) { i++; continue; }
    if (s[i] === "{") { depth++; continue; }
    if (s[i] === "}") { depth = Math.max(0, depth - 1); continue; }
    if (depth === 0) {
      if (s[i] === "|") return -1;
      if (s[i] === "$" && s[i + 1] === "$") return i;
    }
  }
  return -1;
}

// ---------------------------------------------------------------------------
// Count-spec / option parsing
// ---------------------------------------------------------------------------

interface CountSpec {
  min: number;
  max: number;
  sep: string;
  optionsStr: string;
}

/**
 * If `inner` starts with a count spec (`N$$` or `N-M$$`, optionally followed
 * by `sep$$`), parse and return it.  Otherwise return null.
 */
function parseCountSpec(inner: string): CountSpec | null {
  const m = /^(\d+)(?:-(\d+))?\$\$(.*)$/s.exec(inner);
  if (!m) return null;

  const min = parseInt(m[1], 10);
  const max = m[2] !== undefined ? parseInt(m[2], 10) : min;
  const afterCount = m[3];

  // Check for an optional separator between the first $$ and a second $$
  // Stop scanning if we hit a top-level | (no separator spec)
  let sep = ", ";
  let optionsStr = afterCount;
  const sepPos = findTopLevelDollarDollar(afterCount);
  if (sepPos !== -1) {
    sep = afterCount.slice(0, sepPos);
    optionsStr = afterCount.slice(sepPos + 2);
  }

  return { min, max: Math.max(min, max), sep, optionsStr };
}

interface Option {
  text: string;
  weight: number;
}

/** Parse `N::text` weight prefixes from a list of raw option strings. */
function parseOptions(strs: string[]): Option[] {
  return strs.map((s) => {
    const m = /^(\d+(?:\.\d+)?)::([\s\S]*)$/.exec(s);
    if (m) return { text: m[2], weight: parseFloat(m[1]) };
    return { text: s, weight: 1 };
  });
}

/**
 * Weighted pick of one index from `opts`.
 * Uses `rng` for the random draw.
 */
function weightedPickIndex(opts: Option[], rng: () => number): number {
  const total = opts.reduce((sum, o) => sum + Math.max(0, o.weight), 0);
  if (total <= 0) return 0;
  let r = randFloat(rng) * total;
  for (let i = 0; i < opts.length; i++) {
    r -= Math.max(0, opts[i].weight);
    if (r <= 0) return i;
  }
  return opts.length - 1;
}

/**
 * Pick `count` distinct option indices by weighted sampling without replacement.
 * Returns an array of `count` indices (or fewer if the option pool is smaller).
 */
function pickDistinctIndices(opts: Option[], count: number, rng: () => number): number[] {
  // Copy so we can splice without mutating the original
  const pool = opts.map((o, i) => ({ ...o, origIdx: i }));
  const picked: number[] = [];
  const n = Math.min(count, pool.length);
  for (let i = 0; i < n; i++) {
    const localIdx = weightedPickIndex(pool, rng);
    picked.push(pool[localIdx].origIdx);
    pool.splice(localIdx, 1);
  }
  return picked;
}

// ---------------------------------------------------------------------------
// Core expander
// ---------------------------------------------------------------------------

/**
 * Recursively expand random alternation blocks in `text`.
 * `rng` is the shared PRNG — all nested rolls draw from the same stream so
 * nesting depth does not bias outer rolls.
 */
function expandInner(text: string, rng: () => number): string {
  let result = "";
  let i = 0;
  while (i < text.length) {
    // Escape sequences: \{ \| \} become literal characters
    if (text[i] === "\\" && i + 1 < text.length) {
      result += text[i + 1];
      i += 2;
      continue;
    }

    if (text[i] === "{") {
      // Find the matching closing brace (tracking nesting depth)
      let depth = 1;
      let j = i + 1;
      while (j < text.length && depth > 0) {
        if (text[j] === "\\" && j + 1 < text.length) { j += 2; continue; }
        if (text[j] === "{") depth++;
        else if (text[j] === "}") depth--;
        j++;
      }
      // j is now one past the closing "}"
      const inner = text.slice(i + 1, j - 1);

      if (hasTopLevelPipe(inner)) {
        // This is a random alternation block — expand it
        result += processAlternation(inner, rng);
      } else {
        // No top-level "|": leave braces intact (NovelAI emphasis, etc.)
        // Still recurse in case nested content has its own random blocks
        result += "{" + expandInner(inner, rng) + "}";
      }
      i = j;
      continue;
    }

    result += text[i];
    i++;
  }
  return result;
}

/**
 * Process the content of one `{...}` block that contains at least one
 * top-level `|`.
 */
function processAlternation(inner: string, rng: () => number): string {
  // Try to parse a count / separator spec at the start
  const countSpec = parseCountSpec(inner);
  let count = 1;
  let sep = ", ";
  let optionsStr = inner;

  if (countSpec) {
    count =
      countSpec.min < countSpec.max
        ? randInt(rng, countSpec.min, countSpec.max)
        : countSpec.min;
    sep = countSpec.sep;
    optionsStr = countSpec.optionsStr;
  }

  // Split the remaining text by top-level "|"
  const rawOptions = splitTopLevelPipes(optionsStr);
  const options = parseOptions(rawOptions);
  if (options.length === 0) return "";

  // Clamp count to the number of available options
  count = Math.max(1, Math.min(count, options.length));

  // Pick distinct options by weighted sampling
  const indices = pickDistinctIndices(options, count, rng);

  // Recursively expand each picked option, then join
  const picks = indices.map((idx) => expandInner(options[idx].text, rng));
  return picks.join(sep);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Expand all `{a|b|c}` alternation blocks in `text` using a deterministic
 * splitmix32 PRNG seeded with `seed`.
 *
 * `seed` should be `(userSeed + imageIndex)` when the user seed is known, or
 * a randomly chosen positive integer when the generation seed is -1.
 *
 * Braces without a top-level "|" pass through untouched.
 */
export function expandRandomPrompt(text: string, seed: number): string {
  if (!text.includes("{") || !text.includes("|")) return text;
  const rng = makeSplitmix32(seed);
  return expandInner(text, rng);
}

/**
 * Fast check: does `text` contain any `{...}` block with a top-level `|`?
 * Returns false for NovelAI `{tag}` emphasis (no `|`) and for plain prompts.
 */
export function hasRandomSyntax(text: string): boolean {
  if (!text.includes("{") || !text.includes("|")) return false;
  // Check each { block for a top-level |
  let i = 0;
  while (i < text.length) {
    if (text[i] === "\\" && i + 1 < text.length) { i += 2; continue; }
    if (text[i] === "{") {
      let depth = 1;
      let j = i + 1;
      while (j < text.length && depth > 0) {
        if (text[j] === "\\" && j + 1 < text.length) { j += 2; continue; }
        if (text[j] === "{") depth++;
        else if (text[j] === "}") depth--;
        else if (text[j] === "|" && depth === 1) return true;
        j++;
      }
      i = j;
      continue;
    }
    i++;
  }
  return false;
}

/**
 * Generate a one-example roll for UI preview purposes.
 * Uses `seed` (or `Date.now() | 0` when omitted) so the preview is stable
 * until the user explicitly rerolls it.
 */
export function previewRandomPrompt(text: string, seed?: number): string {
  const s = seed !== undefined ? seed : (Date.now() | 0);
  return expandRandomPrompt(text, s);
}
