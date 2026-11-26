/**
 * On NovelAI models with character prompts (V4 and later), a portrait puts
 * the character in a character box, the way Use does, and keeps the scene
 * (count, framing, style) in the main prompt. Counts such as 1girl belong in
 * the main prompt only (see `naiPrompt.ts`), so the box starts with girl, boy
 * or other instead.
 *
 * Leaf util with no store imports, so it runs under plain Node for checking.
 */
import { splitTopLevel } from "./portraitFilter.js";
import { isExpressionTag, withExpression, type PortraitPromptStyle } from "./portraitExpression.js";

/** Supported view counts for generated character turnaround sheets. */
export type SheetTurnaroundViews = 3 | 4;

/** Positioned character slot compatible with `NovelAiCharacter`. */
export interface SheetTurnaroundSlot {
  prompt: string;
  negative_prompt: string;
  center: { x: number; y: number };
  enabled: true;
}

/** A count tag such as 1girl, 2boys or 1other. */
const COUNT_TAG = /^(\d+)\s*(girl|boy|other)s?$/;

/** Lowercase, with underscores read as spaces. */
function plain(tag: string): string {
  return tag.trim().toLowerCase().replace(/_/g, " ").replace(/\s+/g, " ");
}

/**
 * Split a card's tag prompt into the count tags the main prompt takes and the
 * look the character box takes. `solo` goes too, since the portrait adds its
 * own. The box starts with girl, boy or other when the card had a count tag
 * and does not say so already.
 */
export function splitCountTags(prompt: string): { counts: string; look: string } {
  const counts: string[] = [];
  const look: string[] = [];
  let noun: string | null = null;
  for (const tag of splitTopLevel(prompt)) {
    const text = plain(tag);
    const count = COUNT_TAG.exec(text);
    if (count) {
      counts.push(tag);
      noun ??= count[2];
    } else if (text !== "solo") {
      look.push(tag);
    }
  }
  if (noun && !look.some((t) => plain(t) === noun)) look.unshift(noun);
  return { counts: counts.join(", "), look: look.join(", ") };
}

function unwrapTag(tag: string): string {
  let inner = tag.trim();
  const weightBlock = /^-?[\d.]+::([\s\S]+?)::$/;
  const m = weightBlock.exec(inner);
  if (m) inner = m[1].trim();
  while (/^[([{]/.test(inner) && /[)\]}]$/.test(inner)) {
    inner = inner.slice(1, -1).trim();
  }
  inner = inner.replace(/:\s*-?[\d.]+$/, "").trim();
  return plain(inner.replace(/-/g, " "));
}

function isProtectedUserToken(tag: string): boolean {
  const trimmed = tag.trim();
  return /^@/.test(trimmed) || /^artist:/i.test(trimmed);
}

const BACK_VIEW_ANGLE_TAGS = new Set([
  "looking at viewer",
  "facing viewer",
  "straight on",
  "front view",
  "from front",
  "eye contact",
  "looking ahead",
  "looking up",
  "looking down",
  "looking to the side",
  "looking away",
  "from side",
  "side view",
  "profile",
  "three quarter view",
  "3/4 view",
  "facing left",
  "facing right",
]);

const FRONT_CONFLICT_ANGLE_TAGS = new Set([
  "from side",
  "side view",
  "profile",
  "from behind",
  "back view",
  "facing away",
  "facing left",
  "facing right",
]);

const SIDE_CONFLICT_ANGLE_TAGS = new Set([
  "front view",
  "from front",
  "straight on",
  "facing viewer",
  "looking at viewer",
  "eye contact",
  "from behind",
  "back view",
  "facing away",
  "facing left",
  "facing right",
]);

const FACIAL_FEATURE_PATTERN =
  /\b(?:eyes?|eyebrows?|eyelashes?|eyeshadow|eyeliner|pupils?|irises?|sclera|mouth|lips|lipstick|teeth|fangs?|tongue|nose|cheeks?|freckles?|blush|facepaint|facial mark|makeup|glasses|sunglasses|monocle|eyepatch|eyewear|mole under (?:eye|mouth)|under eye mole|face mole)\b/;

function tidyPrompt(text: string): string {
  return text
    .replace(/,\s*,+/g, ", ")
    .replace(/\s{2,}/g, " ")
    .replace(/^[,\s.]+|[,\s]+$/g, "")
    .trim();
}

/**
 * Remove facial expressions, eye/mouth features, eyewear, and viewer-gaze
 * angle cues so a Back-view turnaround slot does not force a front/side face.
 */
export function stripBackViewFacialTags(
  prompt: string,
  style: PortraitPromptStyle = "tags",
): string {
  if (style === "natural") {
    const cleaned = prompt
      .replace(/\b(?:seen )?straight from the front\b/gi, "")
      .replace(/\b(?:directly )?facing the viewer\b/gi, "")
      .replace(/\blooking (?:directly )?at the viewer\b/gi, "");
    return tidyPrompt(cleaned).replace(/\.\s*\./g, ".");
  }

  const kept: string[] = [];
  for (const tag of splitTopLevel(prompt)) {
    if (isProtectedUserToken(tag)) {
      kept.push(tag);
      continue;
    }
    const clean = unwrapTag(tag);
    if (!clean) continue;
    if (isExpressionTag(tag) || BACK_VIEW_ANGLE_TAGS.has(clean) || FACIAL_FEATURE_PATTERN.test(clean)) {
      continue;
    }
    kept.push(tag);
  }
  return kept.join(", ");
}

/** Remove angle tags that fight the target turnaround `view`. */
export function stripConflictingAngleTags(
  prompt: string,
  view: "front" | "side" | "back",
  style: PortraitPromptStyle = "tags",
): string {
  if (view === "back") return stripBackViewFacialTags(prompt, style);
  if (style === "natural") return prompt.trim();

  const blocked = view === "front" ? FRONT_CONFLICT_ANGLE_TAGS : SIDE_CONFLICT_ANGLE_TAGS;
  return splitTopLevel(prompt)
    .filter((tag) => isProtectedUserToken(tag) || !blocked.has(unwrapTag(tag)))
    .join(", ");
}

function combineNegative(slotUc: string, charNeg: string, includeNegative: boolean): string {
  if (!includeNegative) return "";
  return [slotUc.trim(), charNeg.trim()].filter(Boolean).join(", ");
}

function appendTags(base: string, suffix: string): string {
  const b = base.trim().replace(/[,\s]+$/, "");
  return b ? `${b}, ${suffix}` : suffix;
}

function appendNaturalSentence(base: string, sentence: string): string {
  const trimmed = base.trim();
  if (!trimmed) return sentence;
  const withDot = /[.!?]$/.test(trimmed) ? trimmed : `${trimmed}.`;
  return `${withDot} ${sentence}`;
}

/**
 * Build positioned horizontal Character Prompt slots for a 3-view or 4-view
 * NovelAI character turnaround sheet.
 */
export function buildTurnaroundCharacterSlots(opts: {
  characterPrompt: string;
  characterNegative: string;
  expression: string;
  style: PortraitPromptStyle;
  views: SheetTurnaroundViews;
  includeNegative: boolean;
}): SheetTurnaroundSlot[] {
  const { characterPrompt, characterNegative, expression, style, views, includeNegative } = opts;
  const frontLook = stripConflictingAngleTags(characterPrompt, "front", style);
  const sideLook = stripConflictingAngleTags(characterPrompt, "side", style);
  const backLook = stripConflictingAngleTags(characterPrompt, "back", style);
  const expr = expression.trim();

  if (style === "natural") {
    const frontCore = expr ? withExpression(frontLook, expr, "natural") : frontLook;
    const sideCore = expr ? withExpression(sideLook, expr, "natural") : sideLook;
    const backNeg = combineNegative(
      "front view, facing the viewer, face, eyes, mouth, smile, side view, profile, from side",
      characterNegative,
      includeNegative,
    );

    if (views === 4) {
      return [
        {
          prompt: appendNaturalSentence(frontCore, "Front view, standing straight-on facing the viewer, full body."),
          negative_prompt: combineNegative("side view, profile, from side, back view, from behind", characterNegative, includeNegative),
          center: { x: 0.14, y: 0.5 },
          enabled: true,
        },
        {
          prompt: appendNaturalSentence(sideCore, "Side profile view facing left, standing, full body."),
          negative_prompt: combineNegative("front view, facing the viewer, back view, from behind, facing right", characterNegative, includeNegative),
          center: { x: 0.38, y: 0.5 },
          enabled: true,
        },
        {
          prompt: appendNaturalSentence(backLook, "Back view seen straight from behind, facing away from the viewer, standing, full body."),
          negative_prompt: backNeg,
          center: { x: 0.62, y: 0.5 },
          enabled: true,
        },
        {
          prompt: appendNaturalSentence(sideCore, "Side profile view facing right, standing, full body."),
          negative_prompt: combineNegative("front view, facing the viewer, back view, from behind, facing left", characterNegative, includeNegative),
          center: { x: 0.86, y: 0.5 },
          enabled: true,
        },
      ];
    }

    return [
      {
        prompt: appendNaturalSentence(frontCore, "Front view, standing straight-on facing the viewer, full body."),
        negative_prompt: combineNegative("side view, profile, from side, back view, from behind", characterNegative, includeNegative),
        center: { x: 0.18, y: 0.5 },
        enabled: true,
      },
      {
        prompt: appendNaturalSentence(sideCore, "Side profile view facing left, standing, full body."),
        negative_prompt: combineNegative("front view, facing the viewer, back view, from behind", characterNegative, includeNegative),
        center: { x: 0.5, y: 0.5 },
        enabled: true,
      },
      {
        prompt: appendNaturalSentence(backLook, "Back view seen straight from behind, facing away from the viewer, standing, full body."),
        negative_prompt: backNeg,
        center: { x: 0.82, y: 0.5 },
        enabled: true,
      },
    ];
  }

  const frontWithExpr = expr ? withExpression(frontLook, expr, "tags") : frontLook;
  const sideWithExpr = expr ? withExpression(sideLook, expr, "tags") : sideLook;
  const backUc =
    "front view, straight-on, facing viewer, looking at viewer, face, eyes, mouth, smile, side view, profile, from side";

  if (views === 4) {
    return [
      {
        prompt: appendTags(frontWithExpr, "front view, straight-on, facing viewer, looking at viewer, standing, full body"),
        negative_prompt: combineNegative("from side, side view, profile, from behind, back view, facing away", characterNegative, includeNegative),
        center: { x: 0.14, y: 0.5 },
        enabled: true,
      },
      {
        prompt: appendTags(sideWithExpr, "side view, profile, from side, facing left, standing, full body"),
        negative_prompt: combineNegative("front view, straight-on, facing viewer, looking at viewer, from behind, back view, facing away, facing right", characterNegative, includeNegative),
        center: { x: 0.38, y: 0.5 },
        enabled: true,
      },
      {
        prompt: appendTags(backLook, "back view, from behind, facing away, standing, full body"),
        negative_prompt: combineNegative(backUc, characterNegative, includeNegative),
        center: { x: 0.62, y: 0.5 },
        enabled: true,
      },
      {
        prompt: appendTags(sideWithExpr, "side view, profile, from side, facing right, standing, full body"),
        negative_prompt: combineNegative("front view, straight-on, facing viewer, looking at viewer, from behind, back view, facing away, facing left", characterNegative, includeNegative),
        center: { x: 0.86, y: 0.5 },
        enabled: true,
      },
    ];
  }

  return [
    {
      prompt: appendTags(frontWithExpr, "front view, straight-on, facing viewer, looking at viewer, standing, full body"),
      negative_prompt: combineNegative("from side, side view, profile, from behind, back view, facing away", characterNegative, includeNegative),
      center: { x: 0.18, y: 0.5 },
      enabled: true,
    },
    {
      prompt: appendTags(sideWithExpr, "side view, profile, from side, facing left, standing, full body"),
      negative_prompt: combineNegative("front view, straight-on, facing viewer, looking at viewer, from behind, back view, facing away", characterNegative, includeNegative),
      center: { x: 0.5, y: 0.5 },
      enabled: true,
    },
    {
      prompt: appendTags(backLook, "back view, from behind, facing away, standing, full body"),
      negative_prompt: combineNegative(backUc, characterNegative, includeNegative),
      center: { x: 0.82, y: 0.5 },
      enabled: true,
    },
  ];
}
