/**
 * User-defined Enhance Recipes: reusable visual concept directives that guide
 * the prompt assistant (NAI V5 and General Enhance) on demand.
 *
 * Unlike static prompt snippets, a recipe tells the LLM *how* to realize a
 * recurring visual concept, what tags/phrases to include or avoid (and put in
 * UC), and when to adapt or drop it (e.g. covered midriff or face crops).
 *
 * Leaf util, same as `naiRecipes.ts`: it must not import any store.
 */

export type RecipeScope = "character" | "scene" | "any";
export type RecipeApplyMode = "ai" | "tags";

export interface EnhanceRecipe {
  id: string;
  name: string;
  slug: string;
  intent: string;
  include: string;
  avoid: string;
  neverUc: string;
  context: string;
  scope: RecipeScope;
  applyMode: RecipeApplyMode;
  triggers: string[];
  createdAt: number;
  updatedAt: number;
}

export type EnhanceRecipeDraft = Omit<EnhanceRecipe, "id" | "createdAt" | "updatedAt">;

export const RECIPE_AUTHOR_MAX_TOKENS = 1024;
export const RECIPE_SILENT_ADAPT_MAX_TOKENS = 1800;
const MAX_RENDERED_CHARS = 6000;
const EXPORT_KIND = "mooshieui.enhance-recipes";
const EXPORT_VERSION = 1;

function escapeRegex(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function genRecipeId(): string {
  return `rcp_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

/** Normalize a recipe name or slug into a URL/hashtag-safe `#slug`. */
export function slugifyRecipe(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 40)
    .replace(/-+$/g, "");
}

function normalizeScope(value: unknown): RecipeScope {
  return value === "character" || value === "scene" || value === "any" ? value : "any";
}

function normalizeApplyMode(value: unknown): RecipeApplyMode {
  return value === "tags" ? "tags" : "ai";
}

function normalizeTriggers(raw: unknown): string[] {
  if (!Array.isArray(raw)) return [];
  const seen = new Set<string>();
  const out: string[] = [];
  for (const item of raw) {
    if (typeof item !== "string") continue;
    const cleaned = item.trim().toLowerCase();
    if (!cleaned || seen.has(cleaned)) continue;
    seen.add(cleaned);
    out.push(cleaned);
  }
  return out;
}

/** Validate and normalize one persisted or imported recipe object. */
export function sanitizeEnhanceRecipe(raw: unknown, allowMissingId = false): EnhanceRecipe | null {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const r = raw as Record<string, unknown>;
  const id = typeof r.id === "string" && r.id.trim() ? r.id.trim() : allowMissingId ? genRecipeId() : "";
  if (!id) return null;
  const name = typeof r.name === "string" ? r.name.trim() : "";
  if (!name) return null;
  const intent = typeof r.intent === "string" ? r.intent.trim() : "";
  const slug = slugifyRecipe(typeof r.slug === "string" && r.slug.trim() ? r.slug : name) || "recipe";
  const now = Date.now();
  const createdAt = typeof r.createdAt === "number" && Number.isFinite(r.createdAt) ? r.createdAt : now;
  const updatedAt = typeof r.updatedAt === "number" && Number.isFinite(r.updatedAt) ? r.updatedAt : createdAt;
  return {
    id,
    name,
    slug,
    intent,
    include: typeof r.include === "string" ? r.include.trim() : "",
    avoid: typeof r.avoid === "string" ? r.avoid.trim() : "",
    neverUc: typeof r.neverUc === "string" ? r.neverUc.trim() : "",
    context: typeof r.context === "string" ? r.context.trim() : "",
    scope: normalizeScope(r.scope),
    applyMode: normalizeApplyMode(r.applyMode),
    triggers: normalizeTriggers(r.triggers),
    createdAt,
    updatedAt,
  };
}

export function sanitizeEnhanceRecipes(raw: unknown, allowMissingId = false): EnhanceRecipe[] {
  if (!Array.isArray(raw)) return [];
  const seen = new Set<string>();
  const out: EnhanceRecipe[] = [];
  for (const item of raw) {
    const recipe = sanitizeEnhanceRecipe(item, allowMissingId);
    if (!recipe || seen.has(recipe.id)) continue;
    seen.add(recipe.id);
    out.push(recipe);
  }
  return out;
}

function matchesSlug(text: string, slug: string): boolean {
  if (!slug) return false;
  const re = new RegExp(`(?:^|[^a-z0-9_])#${escapeRegex(slug)}(?=$|[^a-z0-9_-])`, "i");
  return re.test(text);
}

function matchesTriggerPhrase(text: string, phrase: string): boolean {
  const cleaned = phrase.trim();
  if (!cleaned) return false;
  const startsWord = /^[a-z0-9_]/i.test(cleaned);
  const endsWord = /[a-z0-9_]$/i.test(cleaned);
  const prefix = startsWord ? "(?:^|[^a-z0-9_])" : "";
  const suffix = endsWord ? "(?=$|[^a-z0-9_])" : "";
  const re = new RegExp(`${prefix}${escapeRegex(cleaned)}${suffix}`, "i");
  return re.test(text);
}

/**
 * Pick recipes that are pinned active OR matched by `#slug` or custom trigger
 * phrases across `texts` (current prompt + active session user turns).
 */
export function matchTriggeredRecipes(
  recipes: EnhanceRecipe[],
  texts: string[],
  pinnedIds: ReadonlySet<string>,
): EnhanceRecipe[] {
  const seen = new Set<string>();
  const matched: EnhanceRecipe[] = [];
  for (const recipe of recipes) {
    if (seen.has(recipe.id)) continue;
    const active =
      pinnedIds.has(recipe.id) ||
      texts.some(
        (t) =>
          matchesSlug(t, recipe.slug) ||
          recipe.triggers.some((trigger) => matchesTriggerPhrase(t, trigger)),
      );
    if (active) {
      seen.add(recipe.id);
      matched.push(recipe);
    }
  }
  return matched;
}

/**
 * Pick recipes active at Generate time: pinned chips OR explicit `#slug` tokens
 * in `texts`. Plain trigger phrases are reserved for Enhance Chat so ordinary
 * manual prompts never trigger unexpected silent LLM calls.
 */
export function matchGenerateTimeRecipes(
  recipes: EnhanceRecipe[],
  texts: string[],
  pinnedIds: ReadonlySet<string>,
): EnhanceRecipe[] {
  const seen = new Set<string>();
  const matched: EnhanceRecipe[] = [];
  for (const recipe of recipes) {
    if (seen.has(recipe.id)) continue;
    const active = pinnedIds.has(recipe.id) || texts.some((t) => matchesSlug(t, recipe.slug));
    if (active) {
      seen.add(recipe.id);
      matched.push(recipe);
    }
  }
  return matched;
}

export interface RecipePromptFields {
  flow: string;
  positive: string;
  negative: string;
  characters?: readonly { prompt: string; negative_prompt: string }[];
}

/**
 * Deterministic key over prompt fields and active recipes for Generate-time
 * caching and skipping double-processing right after Enhance Chat Apply.
 */
export function recipePromptFingerprint(
  fields: RecipePromptFields,
  recipes: readonly EnhanceRecipe[],
): string {
  const recipeKey = recipes
    .map((r) => `${r.id}@${r.updatedAt}:${r.applyMode}`)
    .sort()
    .join("|");
  const charsKey = (fields.characters ?? [])
    .map((c) => `${c.prompt.trim()}\u0000${c.negative_prompt.trim()}`)
    .join("\u0001");
  return [
    fields.flow,
    fields.positive.trim(),
    fields.negative.trim(),
    charsKey,
    recipeKey,
  ].join("\u0002");
}

function normalizeTagCompare(tag: string): string {
  return tag
    .trim()
    .replace(/^[{\[]+|[}\]]+$/g, "")
    .replace(/^-?\d+(?:\.\d+)?::|::$/g, "")
    .trim()
    .toLowerCase()
    .replace(/\s+/g, " ");
}

function appendCommaTags(line: string, tags: string[]): string {
  const clean = line.trimEnd();
  if (!clean) return tags.join(", ");
  const stripped = clean.replace(/,+$/, "").trimEnd();
  return stripped ? `${stripped}, ${tags.join(", ")}` : tags.join(", ");
}

function mergeUniqueTags(existing: string, addition: string, isNaiPositive = false): string {
  const wanted = addition
    .split(",")
    .map((t) => t.trim())
    .filter(Boolean);
  if (wanted.length === 0) return existing;

  const present = new Set(
    existing
      .split(/[\n,]+/)
      .map(normalizeTagCompare)
      .filter(Boolean),
  );
  const toAdd: string[] = [];
  for (const raw of wanted) {
    const norm = normalizeTagCompare(raw);
    if (!norm || present.has(norm)) continue;
    present.add(norm);
    toAdd.push(raw);
  }
  if (toAdd.length === 0) return existing;

  if (isNaiPositive) {
    const normalized = existing.replace(/\r\n/g, "\n");
    const splitIdx = normalized.search(/\n[ \t]*\n/);
    if (splitIdx !== -1) {
      const firstBlock = normalized.slice(0, splitIdx);
      const rest = normalized.slice(splitIdx);
      // If the prompt starts directly with `Text:` without a tag line, keep `Text:` at the end.
      if (!/^\s*Text:/i.test(firstBlock)) {
        return `${appendCommaTags(firstBlock, toAdd)}${rest}`;
      }
    }
    if (/^\s*Text:/i.test(normalized)) {
      return `${toAdd.join(", ")}\n\n${normalized}`;
    }
  }
  return appendCommaTags(existing, toAdd);
}

/**
 * Instant, deterministic tag + UC fallback/application: strips `#slug` tokens,
 * merges unique `include` / `avoid` tags without disturbing NAI prose/`Text:`,
 * and strips `neverUc` tags from every UC field.
 */
export function applyRecipesDeterministic<
  TChar extends { prompt: string; negative_prompt: string },
>(
  fields: {
    positive: string;
    negative: string;
    characters?: TChar[];
  },
  recipes: EnhanceRecipe[],
  isNai: boolean,
): {
  positive: string;
  negative: string;
  characters?: TChar[];
} {
  let positive = stripLeakedRecipeSlugs(fields.positive, recipes);
  let negative = stripLeakedRecipeSlugs(fields.negative, recipes);
  let characters = fields.characters?.map((c) => ({
    ...c,
    prompt: stripLeakedRecipeSlugs(c.prompt, recipes),
    negative_prompt: stripLeakedRecipeSlugs(c.negative_prompt, recipes),
  }));

  for (const recipe of recipes) {
    if (recipe.include) {
      const activeChars = characters?.filter((c) => c.prompt.trim().length > 0) ?? [];
      if (isNai && characters && activeChars.length > 0 && recipe.scope === "character") {
        characters = characters.map((c) =>
          c.prompt.trim().length > 0
            ? { ...c, prompt: mergeUniqueTags(c.prompt, recipe.include, false) }
            : c,
        );
      } else {
        positive = mergeUniqueTags(positive, recipe.include, isNai);
      }
    }
    if (recipe.avoid) {
      negative = mergeUniqueTags(negative, recipe.avoid, false);
    }
  }

  negative = stripNeverUcTags(negative, recipes);
  if (characters) {
    characters = characters.map((c) => ({
      ...c,
      negative_prompt: stripNeverUcTags(c.negative_prompt, recipes),
    }));
  }
  return { positive, negative, characters };
}

function scopeGuidance(scope: RecipeScope, flow: "nai" | "general"): string {
  if (flow === "general") {
    return "Scope: Integrate this concept naturally into the prompt without copying scene-irrelevant boilerplate.";
  }
  if (scope === "character") {
    return "Scope: Apply to the target character's CHAR box and prose (or the only character when there is one).";
  }
  if (scope === "scene") {
    return "Scope: Apply to BASE tags and scene prose.";
  }
  return "Scope: Integrate into whichever BASE or CHAR fields fit the scene.";
}

function recipeGuidelinesHeader(flow: "nai" | "general"): string {
  const formatRules =
    flow === "nai"
      ? `- V5 tags vs prose separation: Put short genuine Danbooru tags from 'Cues to include' in the target CHAR block (or BASE tag line for scene scope). NEVER paste prose sentences into a CHAR block. Express 'Target' naturally in BASE's prose paragraph via 3D volume, surrounding contours, and tonal shading.
- Multi-character targeting: If the user places a recipe #slug or phrase beside a specific character, apply a 'character' recipe only to that character's CHAR block and CHAR n UC (not global UC when another character should not be affected).
- UC conflict removal and hygiene: Never keep terms listed in 'Never negate in UC' or broad parent tags that cancel out an active recipe in UC or CHAR n UC (even if present in an existing prompt or saved card). Keep 'Avoid / UC' to a tight list of unwanted extremes, and never put anti-subtlety negatives ('faint', 'thin line') into UC.`
      : `- Negative conflict removal: Do not emit negative terms listed in 'Never negate in UC' or broad terms that suppress an active recipe's target traits.`;
  return `USER CONCEPT RECIPE RULES (treat active recipes as first-class user instructions; they override conflicting saved-character body/avoid lines and existing-prompt defaults without violating source fidelity):
- Compose the whole prompt with the recipe in mind: pick lighting, shading, framing, and prose details that make the recipe's visual traits read clearly (for example, natural angled light instead of flat even lighting or harsh high-contrast shadows when 3D skin contours matter), unless the user requests specific lighting or framing.
${formatRules}
- Concrete visual prose + real tags: Describe natural 3D form and shading clearly in natural language and use genuine Danbooru/model tags. Never invent multi-word pseudo-tags, never heavily weight rare anatomical terms (for example '1.3::...::', which causes literal line/seam artifacts), and do not weaken an emphasized trait with hedging words ('faint', 'subtle', 'gently', 'soft', 'smooth') unless the recipe asks for subtlety.
- Form over inked lines: Realize organic, anatomical, and surface-definition concepts through 3D volume, surrounding contours, and tonal light-and-shadow modeling rather than literal line or cut words ('crease', 'groove', 'split', 'seam', 'cut', 'drawn line') that cause illustration models to paint hard dark stripes onto skin or surfaces.
- Conditional activation: If 'Adaptation' says to skip the recipe (for example covered clothing or a face crop), omit BOTH its positive cues and its 'Avoid / UC' tags.
- Trigger tokens: '#slug' tokens in the user's message are recipe triggers, never literal prompt tags or rendered text.`;
}

/**
 * Render active user recipes into a concise directive block for the system
 * prompt, or `""` when none are active.
 */
export function renderUserRecipesDirective(recipes: EnhanceRecipe[], flow: "nai" | "general"): string {
  if (recipes.length === 0) return "";
  const header = recipeGuidelinesHeader(flow);
  const blocks: string[] = [header];
  let total = header.length;

  for (const recipe of recipes) {
    const lines = [
      `USER CONCEPT RECIPE: ${recipe.name.toUpperCase()} (#${recipe.slug})`,
      scopeGuidance(recipe.scope, flow),
    ];
    if (recipe.intent) lines.push(`Target: ${recipe.intent}`);
    if (recipe.include) lines.push(`Cues to include: ${recipe.include}`);
    if (recipe.avoid) {
      const suffix =
        flow === "nai"
          ? " (keep out of positive prompt; add relevant negative terms to UC only while this recipe applies)"
          : " (keep out of positive prompt)";
      lines.push(`Avoid / UC: ${recipe.avoid}${suffix}`);
    }
    if (recipe.neverUc) {
      lines.push(`Never negate in UC: ${recipe.neverUc}`);
    }
    if (recipe.context) lines.push(`Adaptation: ${recipe.context}`);
    const block = lines.join("\n");
    const sep = 2;
    if (total + sep + block.length > MAX_RENDERED_CHARS) {
      const remaining = MAX_RENDERED_CHARS - total - sep;
      if (remaining > 80) {
        blocks.push(`${block.slice(0, remaining - 3).trimEnd()}...`);
      }
      break;
    }
    blocks.push(block);
    total += sep + block.length;
  }

  return blocks.join("\n\n");
}

/**
 * Detect whether a chat message is explicitly asking to create/save a Concept
 * Recipe (e.g. "make this into a recipe", "save this as a recipe", "create a
 * recipe for ..."), while avoiding false positives on ordinary prompt prose
 * that happens to mention a cooking recipe.
 */
export function detectRecipeCreationIntent(text: string): boolean {
  const trimmed = text.trim();
  if (!trimmed) return false;
  return (
    /\b(?:make|turn|convert)\s+(?:this|that|it|the\s+(?:last|previous|above|current)(?:\s+(?:prompt|answer|concept|look|result))?)\s+(?:in)?to\s+(?:a\s+)?(?:concept\s+)?recipe\b/i.test(trimmed) ||
    /\b(?:save|store|keep)\s+(?:this|that|it|the\s+(?:last|previous|above|current)(?:\s+(?:prompt|answer|concept|look|result))?)\s+as\s+(?:a\s+)?(?:concept\s+)?recipe\b/i.test(trimmed) ||
    /\b(?:make|create|build|write|draft|add|generate)\s+(?:(?:me\s+)?(?:a\s+(?:new\s+)?|new\s+))?(?:concept\s+)?recipe(?:\s+(?:for|about|from|out\s+of|that|to|called|named)\b|\s*:|\s*$)/i.test(trimmed)
  );
}

/**
 * Detect whether a chat message is explicitly asking to refine/update an
 * existing Concept Recipe (e.g. "update the linea alba recipe to ...",
 * "fix my recipe so it drops abs").
 */
export function detectRecipeRefineIntent(text: string): boolean {
  const trimmed = text.trim();
  if (!trimmed) return false;
  return (
    /\b(?:update|refine|fix|tweak|adjust|edit|improve|patch)\s+(?:the\s+|my\s+)?(?:(?:[\w#-]+\s+){0,4})?(?:concept\s+)?recipe\b/i.test(trimmed) ||
    /\brecipe\s+#[a-z0-9-]+\s*[:,-]/i.test(trimmed)
  );
}

/**
 * Resolve which saved recipe a chat refinement command refers to:
 * 1. Explicit `#slug` in `text`
 * 2. Exact recipe `name` in `text`
 * 3. Recipe `trigger` phrase in `text`
 * 4. Single active pinned recipe
 * 5. Single saved recipe overall
 */
export function matchTargetRecipeForRefine(
  text: string,
  recipes: EnhanceRecipe[],
  pinnedIds: ReadonlySet<string>,
): EnhanceRecipe | null {
  if (recipes.length === 0) return null;
  const bySlug = recipes.find((r) => matchesSlug(text, r.slug));
  if (bySlug) return bySlug;

  const byName = recipes.find((r) => matchesTriggerPhrase(text, r.name));
  if (byName) return byName;

  const byTrigger = recipes.find((r) =>
    r.triggers.some((trigger) => matchesTriggerPhrase(text, trigger)),
  );
  if (byTrigger) return byTrigger;

  const active = recipes.filter((r) => pinnedIds.has(r.id));
  if (active.length === 1) return active[0];

  if (recipes.length === 1) return recipes[0];
  return null;
}

function unwrapUcSegment(segment: string): {
  bare: string;
  openWeight: string | null;
  closeWeight: boolean;
  openBraces: string;
  closeBraces: string;
} {
  let s = segment.trim();
  let openWeight: string | null = null;
  let closeWeight = false;
  let openBraces = "";
  let closeBraces = "";

  // Peel outer { / [ wrappers and matching } / ]
  while (/^[{[]/.test(s)) {
    openBraces += s[0];
    s = s.slice(1).trimStart();
  }
  while (/[}\]]$/.test(s)) {
    closeBraces = s[s.length - 1] + closeBraces;
    s = s.slice(0, -1).trimEnd();
  }

  const wOpen = s.match(/^(-?\d+(?:\.\d+)?::)\s*/);
  if (wOpen) {
    openWeight = wOpen[1];
    s = s.slice(wOpen[0].length);
  }
  if (s.endsWith("::")) {
    closeWeight = true;
    s = s.slice(0, -2).trimEnd();
  }
  // Handle braces nested inside weight spans like `1.2::{abs}::`
  while (/^[{[]/.test(s)) {
    openBraces += s[0];
    s = s.slice(1).trimStart();
  }
  while (/[}\]]$/.test(s)) {
    closeBraces = s[s.length - 1] + closeBraces;
    s = s.slice(0, -1).trimEnd();
  }

  return {
    bare: s.trim().toLowerCase().replace(/\s+/g, " "),
    openWeight,
    closeWeight,
    openBraces,
    closeBraces,
  };
}

function netBraceDelta(openStr: string, closeStr: string): {
  unclosedOpen: string;
  unmatchedClose: string;
} {
  // Cancel balanced trailing open against leading close inside the single segment
  const opens = openStr.split("");
  const closes = closeStr.split("");
  while (opens.length > 0 && closes.length > 0) {
    opens.pop();
    closes.shift();
  }
  return { unclosedOpen: opens.join(""), unmatchedClose: closes.join("") };
}

/**
 * Deterministically remove tags listed in `recipe.neverUc` from a UC string
 * while preserving NAI `{...}`, `[...]`, and `weight::...::` spans cleanly.
 */
export function stripNeverUcTags(uc: string, recipes: EnhanceRecipe[]): string {
  if (!uc.trim() || recipes.length === 0) return uc;
  const forbidden = new Set<string>();
  for (const r of recipes) {
    for (const part of r.neverUc.split(",")) {
      const norm = part.trim().toLowerCase().replace(/\s+/g, " ");
      if (norm) forbidden.add(norm);
    }
  }
  if (forbidden.size === 0) return uc;

  const parts = uc.split(",");
  const kept: string[] = [];
  let inWeightSpan = false;
  let pendingWeightOpen: string | null = null;
  let pendingBraceOpen = "";

  for (const raw of parts) {
    const trimmed = raw.trim();
    if (!trimmed) continue;
    const info = unwrapUcSegment(trimmed);
    const braceNet = netBraceDelta(info.openBraces, info.closeBraces);
    const opensSpan = !!info.openWeight && !info.closeWeight;
    const closesSpan = !info.openWeight && info.closeWeight;

    if (info.bare && forbidden.has(info.bare)) {
      if (opensSpan && !inWeightSpan) {
        pendingWeightOpen = info.openWeight;
        inWeightSpan = true;
      } else if (closesSpan && inWeightSpan) {
        if (pendingWeightOpen) {
          pendingWeightOpen = null;
        } else if (kept.length > 0) {
          kept[kept.length - 1] = `${kept[kept.length - 1]}::`;
        }
        inWeightSpan = false;
      }
      if (braceNet.unclosedOpen) {
        pendingBraceOpen += braceNet.unclosedOpen;
      } else if (braceNet.unmatchedClose) {
        let rem = braceNet.unmatchedClose;
        while (rem.length > 0 && pendingBraceOpen.length > 0) {
          pendingBraceOpen = pendingBraceOpen.slice(0, -1);
          rem = rem.slice(1);
        }
        if (rem && kept.length > 0) {
          kept[kept.length - 1] = `${kept[kept.length - 1]}${rem}`;
        }
      }
      continue;
    }

    let next = trimmed;
    if (pendingWeightOpen) {
      next = `${pendingWeightOpen}${next}`;
      pendingWeightOpen = null;
    }
    if (pendingBraceOpen) {
      next = `${pendingBraceOpen}${next}`;
      pendingBraceOpen = "";
    }
    if (opensSpan) inWeightSpan = true;
    if (closesSpan) inWeightSpan = false;
    kept.push(next);
  }

  return kept.join(", ");
}

/**
 * Strip leaked `#slug` trigger tokens from generated prompt fields without
 * disturbing intentional trailing commas on NAI CHAR lines.
 */
export function stripLeakedRecipeSlugs(text: string, recipes: EnhanceRecipe[]): string {
  if (!text || recipes.length === 0) return text;
  const slugs = recipes.map((r) => r.slug.trim()).filter(Boolean);
  if (slugs.length === 0) return text;

  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const outLines: string[] = [];
  for (const line of lines) {
    const hadTrailingComma = /,[ \t]*$/.test(line);
    let next = line;
    let changed = false;
    for (const slug of slugs) {
      const re = new RegExp(`(^|[^a-z0-9_])#${escapeRegex(slug)}(?=$|[^a-z0-9_-])`, "gi");
      const replaced = next.replace(re, "$1");
      if (replaced !== next) {
        next = replaced;
        changed = true;
      }
    }
    if (changed) {
      next = next
        .replace(/,[ \t]*,+/g, ",")
        .replace(/[ \t]+,/g, ",")
        .replace(/^[ \t]*,[ \t]*/g, "")
        .replace(/[ \t]{2,}/g, " ")
        .trimEnd();
      if (!hadTrailingComma) next = next.replace(/,[ \t]*$/, "");
      if (!next.trim() || /^[ \t,]+$/.test(next)) continue;
    }
    outLines.push(next);
  }
  return outLines.join("\n").trim();
}

/** System prompt for authoring or distilling a Concept Recipe from chat or a description. */
export function recipeDistillSystemPrompt(): string {
  return `You author a reusable image-prompt Concept Recipe JSON object from an Enhance conversation and/or a user's plain-English concept description.

A Concept Recipe is NOT a static prompt snippet. It is a set of instructions telling a prompt-writing LLM how to realize one specific visual, anatomical, lighting, framing, or stylistic concept across future scenes, outfits, poses, and crops.

Rules:
- Extract or author the core recurring concept, NOT one-off scene details (ignore specific outfits, hair colors, backgrounds, or named characters unless they are essential to the concept itself).
- Keep every field concise, concrete, and actionable.
- If reference images are attached, inspect the visual qualities the user points to and describe them concretely without ever mentioning "the image" or "the reference".
- Separate "avoid" and "neverUc" cleanly: put ONLY a tight comma-separated list of 3-8 unwanted extreme tags in "avoid" (no prose sentences or meta-instructions). Put broad parent tags that MUST NOT appear in UC while active (such as "abs, muscular" for toned abs) in "neverUc".
- Never include anti-subtlety negatives (such as "faint", "thin line") in "avoid", as they force harsh drawn-line artifacts.
- Avoid hedging words ("faint", "subtle", "gently", "soft", "smooth") unless the user specifically asked for the trait to be barely visible.
- Put ONLY short genuine Danbooru/model tags in "include" (never prose sentences, never invented compound pseudo-tags like "toned feminine abdomen", and never heavy numeric weights on rare anatomical terms). Put natural-language contour/shading guidance in "intent".
- Form over inked lines: When the user describes an organic, anatomical, or surface-definition concept using words like "lines", "outline", or "definition", translate that into natural 3D contours, surrounding anatomical form, and tonal shading. NEVER write literal cut or line words ("crease", "groove", "split", "seam", "cut", "drawn line") into "intent" or "include" for skin or anatomy, as illustration models render them as harsh inked stripes or scars.
- When the concept relies on surface contours or shading (anatomy, relief, fabric texture), include a natural angled lighting cue in "context" (avoiding both flat washout and harsh high-contrast ink shadows) alongside crop/occlusion fallbacks.
- Reply with ONE valid JSON object only. No markdown fences, no commentary.

JSON schema:
{
  "name": "Short title (2-4 words)",
  "slug": "lowercase-hyphen-slug",
  "scope": "character" | "scene" | "any",
  "intent": "Plain-English description of the visual goal, 3D form, and shading",
  "include": "Short genuine Danbooru/model tags ONLY (no sentences, no pseudo-tags, no literal line/cut words)",
  "avoid": "Tight comma-separated list of 3-8 unwanted extreme UC tags ONLY (no meta-instructions)",
  "neverUc": "Comma-separated broad parent tags that MUST NEVER appear in UC while active (empty if none)",
  "context": "When to adapt, tone down, or drop the concept (e.g. covered clothing, face crop, angled light)",
  "triggers": ["optional natural trigger phrases"]
}`;
}

export function recipeDistillUserPrompt(
  exchanges: { user: string; reply: string; prompt: string }[],
): string {
  const recent = exchanges.slice(-6);
  const formatted = recent
    .map((ex, i) => {
      const parts = [`Turn ${i + 1} User: ${ex.user.trim()}`];
      if (ex.reply.trim()) parts.push(`Assistant Reply: ${ex.reply.trim()}`);
      if (ex.prompt.trim()) parts.push(`Resulting Prompt:\n${ex.prompt.trim()}`);
      return parts.join("\n");
    })
    .join("\n\n---\n\n");
  return `Distill the recurring concept the user was aiming for into one Concept Recipe JSON object:\n\n${formatted}`;
}

export function recipeCreateUserPrompt(
  instruction: string,
  exchanges: { user: string; reply: string; prompt: string }[] = [],
): string {
  const cleanInstruction = instruction.trim();
  const recent = exchanges.slice(-6);
  if (recent.length === 0) {
    return `Create one Concept Recipe JSON object from this description:\n\n${cleanInstruction}`;
  }
  if (!cleanInstruction) {
    return recipeDistillUserPrompt(recent);
  }
  const formatted = recent
    .map((ex, i) => {
      const parts = [`Turn ${i + 1} User: ${ex.user.trim()}`];
      if (ex.reply.trim()) parts.push(`Assistant Reply: ${ex.reply.trim()}`);
      if (ex.prompt.trim()) parts.push(`Resulting Prompt:\n${ex.prompt.trim()}`);
      return parts.join("\n");
    })
    .join("\n\n---\n\n");
  return `Create one Concept Recipe JSON object from the recent conversation and the user's request.\n\nRecent conversation:\n${formatted}\n\nUser request:\n${cleanInstruction}`;
}

/** Compact plain-text summary shown in Enhance chat when a recipe is created. */
export function formatRecipeChatSummary(recipe: EnhanceRecipe): string {
  const lines = [`Saved and activated recipe "${recipe.name}" (#${recipe.slug}).`];
  if (recipe.intent) lines.push(`Target: ${recipe.intent}`);
  if (recipe.include) lines.push(`Include: ${recipe.include}`);
  if (recipe.avoid) lines.push(`Avoid / UC: ${recipe.avoid}`);
  if (recipe.neverUc) lines.push(`Never negate in UC: ${recipe.neverUc}`);
  if (recipe.context) lines.push(`Adaptation: ${recipe.context}`);
  return lines.join("\n");
}

/** System prompt for patching an existing recipe based on user feedback about drift/failure. */
export function recipeRefineSystemPrompt(): string {
  return `You refine an existing image-prompt Concept Recipe based on the user's feedback about what drifted or failed in generated prompts.

Rules:
- Preserve what already works in the recipe.
- Tighten "intent", "include", "avoid", "neverUc", or "context" to prevent the reported issue from recurring.
- Put ONLY tight extreme UC tags in "avoid" (no meta-instructions); move broad parent tags that must NOT be in UC (such as "abs" or "muscular") into "neverUc", and remove anti-subtlety terms ("faint", "thin line") from "avoid".
- Remove hedging words ("faint", "subtle", "gently", "soft", "smooth") unless subtlety is desired, keep "include" to short genuine Danbooru/model tags ONLY (no prose sentences, no invented compound pseudo-tags, no heavy numeric weights on rare anatomical terms), replace any literal line/cut words ("crease", "groove", "split", "seam", "cut") in "intent" or "include" with 3D form and surrounding contour descriptions in "intent", and add natural angled lighting/shading cues in "context" if surface definition disappeared under flat light or turned into harsh lines.
- Reply with ONE valid JSON object using the same keys ("name", "slug", "scope", "intent", "include", "avoid", "neverUc", "context", "triggers"). No markdown fences, no commentary.`;
}

export function recipeRefineUserPrompt(recipe: EnhanceRecipe, feedback: string): string {
  const current = {
    name: recipe.name,
    slug: recipe.slug,
    scope: recipe.scope,
    intent: recipe.intent,
    include: recipe.include,
    avoid: recipe.avoid,
    neverUc: recipe.neverUc,
    context: recipe.context,
    triggers: recipe.triggers,
  };
  return `Current recipe:\n${JSON.stringify(current, null, 2)}\n\nUser feedback on what went wrong:\n${feedback.trim()}`;
}

/** Parse LLM JSON output into a partial recipe draft, returning `null` if unusable. */
export function parseRecipeDraft(raw: string): Partial<EnhanceRecipeDraft> | null {
  const cleaned = raw
    .trim()
    .replace(/^```(?:json)?\s*/i, "")
    .replace(/\s*```$/i, "")
    .trim();
  const start = cleaned.indexOf("{");
  const end = cleaned.lastIndexOf("}");
  if (start === -1 || end <= start) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(cleaned.slice(start, end + 1));
  } catch {
    return null;
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
  const obj = parsed as Record<string, unknown>;
  const name = typeof obj.name === "string" ? obj.name.trim() : "";
  const intent = typeof obj.intent === "string" ? obj.intent.trim() : "";
  if (!name && !intent) return null;
  const slugSource = typeof obj.slug === "string" && obj.slug.trim() ? obj.slug : name;
  return {
    name: name || "Untitled recipe",
    slug: slugifyRecipe(slugSource) || "recipe",
    scope: normalizeScope(obj.scope),
    intent,
    include: typeof obj.include === "string" ? obj.include.trim() : "",
    avoid: typeof obj.avoid === "string" ? obj.avoid.trim() : "",
    neverUc: typeof obj.neverUc === "string" ? obj.neverUc.trim() : "",
    context: typeof obj.context === "string" ? obj.context.trim() : "",
    applyMode: normalizeApplyMode(obj.applyMode),
    triggers: normalizeTriggers(obj.triggers),
  };
}

/** System prompt for silently adapting an existing NovelAI V5 prompt to active recipes. */
export function naiSilentRecipeAdaptSystemPrompt(
  recipes: EnhanceRecipe[],
  variant: "curated" | "full",
): string {
  const budget = variant === "curated" ? 703 : 1471;
  return `You silently apply active user Concept Recipes to an existing NovelAI Diffusion V5 prompt before generation.

${renderUserRecipesDirective(recipes, "nai")}

RULES
- Keep the user's subject, character names, series tags, artist tags, quality tags, weights, outfit, pose, camera angle, setting, and any Text: block intact.
- Keep the exact same number of CHAR blocks as the input (add none, drop none).
- Change ONLY what is needed to realize the active recipes cleanly (for example, adding a recipe's tags/3D contour prose, tuning flat lighting when contours require angled light, adding recipe Avoid/UC terms, and removing Never-negate UC conflicts).
- If an active recipe's Adaptation rules say to skip this prompt (e.g. covered midriff, face crop, unrelated subject, or landscape), return the prompt fields unchanged.
- Stay well within the NovelAI V5 ${variant === "curated" ? "Curated" : "Full"} budget (~${budget} tokens).
- Never use em dashes or en dashes.
- Reply ONLY in NovelAI V5 labelled fields (BASE:, UC:, and CHAR n: / CHAR n UC: when character boxes are present). No <reply>, no markdown fences, no commentary.`;
}

export function naiSilentRecipeAdaptUserPrompt(fields: {
  base: string;
  uc: string;
  characters: string[];
  characterUcs: string[];
}): string {
  const parts = [`BASE:\n${fields.base}`, `UC:\n${fields.uc}`];
  fields.characters.forEach((c, i) => {
    parts.push(`CHAR ${i + 1}:\n${c}`);
    const cuc = fields.characterUcs[i]?.trim();
    if (cuc) parts.push(`CHAR ${i + 1} UC:\n${cuc}`);
  });
  return `Apply the active Concept Recipes to this NovelAI V5 prompt (or return unchanged if Adaptation skips):\n\n${parts.join("\n\n")}`;
}

/** System prompt for silently adapting a General (non-V5) prompt to active recipes. */
export function generalSilentRecipeAdaptSystemPrompt(
  recipes: EnhanceRecipe[],
  family: string,
): string {
  return `You silently apply active user Concept Recipes to an existing ${family || "image"} prompt before generation.

${renderUserRecipesDirective(recipes, "general")}

RULES
- Keep the user's subject, artists, quality tags, LoRA syntax, style, setting, and existing syntax intact.
- Weave in the active recipes' positive cues and remove conflicting negatives (keeping Never-negate UC terms out of the negative prompt).
- If an active recipe's Adaptation rules say to skip this prompt, return positive and negative unchanged.
- Reply with ONE valid JSON object only: {"positive":"...","negative":"..."}. No markdown fences, no commentary.`;
}

export function generalSilentRecipeAdaptUserPrompt(positive: string, negative: string): string {
  return `Positive prompt:\n${positive}\n\nNegative prompt:\n${negative}`;
}

/** Parse JSON output from `generalSilentRecipeAdaptSystemPrompt`. */
export function parseGeneralAdaptedPrompt(
  raw: string,
): { positive: string; negative: string } | null {
  const cleaned = raw
    .trim()
    .replace(/^```(?:json)?\s*/i, "")
    .replace(/\s*```$/i, "")
    .trim();
  const start = cleaned.indexOf("{");
  const end = cleaned.lastIndexOf("}");
  if (start === -1 || end <= start) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(cleaned.slice(start, end + 1));
  } catch {
    return null;
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
  const obj = parsed as Record<string, unknown>;
  const positive = typeof obj.positive === "string" ? obj.positive.trim() : "";
  if (!positive) return null;
  const negative = typeof obj.negative === "string" ? obj.negative.trim() : "";
  return { positive, negative };
}

/** Serialize recipes to a portable JSON export string. */
export function serializeRecipesExport(recipes: EnhanceRecipe[]): string {
  return JSON.stringify(
    {
      kind: EXPORT_KIND,
      version: EXPORT_VERSION,
      recipes,
    },
    null,
    2,
  );
}

/** Parse an imported JSON file (either wrapped bundle or raw recipe array). */
export function parseRecipesImport(json: string): EnhanceRecipe[] | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    return null;
  }
  let list: unknown;
  if (Array.isArray(parsed)) {
    list = parsed;
  } else if (parsed && typeof parsed === "object") {
    const obj = parsed as Record<string, unknown>;
    if (obj.kind && obj.kind !== EXPORT_KIND) return null;
    list = obj.recipes;
  }
  const sanitized = sanitizeEnhanceRecipes(list, true);
  return sanitized.length > 0 ? sanitized : null;
}
