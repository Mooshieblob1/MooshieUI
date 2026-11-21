import type { ModelFamily } from "./modelFamily.js";

export interface AutoQualityTagSets {
  animaPositive: string;
  animaNegative: string;
  illustriousPositive: string;
  illustriousNegative: string;
  ponyPositive: string;
  ponyNegative: string;
  nanosaurPositive: string;
  nanosaurNegative: string;
}

/**
 * Lightweight filename fallback for synchronous prompt import when a new
 * checkpoint hasn't finished async `readModelSpec`.
 */
export function inferQualityFamilyFromModelName(name: string): ModelFamily {
  const lower = name.trim().toLowerCase();
  if (!lower) return "unknown";

  const tokens = lower.split(/[^a-z0-9]+/).filter(Boolean);

  if (lower.includes("noobai") || lower.includes("illustrious")) {
    return "illustrious";
  }
  if (
    tokens.some(
      (t) =>
        t === "ilxl" ||
        t === "illu" ||
        t === "noob" ||
        t === "noobxl" ||
        (t.length >= 4 && t.endsWith("illu")),
    )
  ) {
    return "illustrious";
  }

  if (
    tokens.some(
      (t) =>
        t === "anima" ||
        (t.startsWith("anima") && !t.startsWith("animag") && !t.startsWith("animat")),
    )
  ) {
    return "anima";
  }

  if (lower.includes("pony")) return "pony";
  if (lower.includes("nanosaur")) return "nanosaur";

  return "unknown";
}

function splitTagSet(raw: string): Set<string> {
  return new Set(
    raw
      .split(",")
      .map((t) => t.trim().toLowerCase())
      .filter(Boolean),
  );
}

/**
 * Return only the quality tags that `toParams()` will re-inject for `family`
 * and `side` when `autoQualityEnabled` is true. Returns an empty set when
 * auto quality tags are off or when the family does not inject any.
 */
export function autoQualityTagsForImport(
  family: ModelFamily,
  side: "positive" | "negative",
  autoQualityEnabled: boolean,
  sets: AutoQualityTagSets,
  usesAnimaQuality = family === "anima",
): Set<string> {
  if (!autoQualityEnabled) return new Set();

  if (usesAnimaQuality || family === "anima") {
    return splitTagSet(side === "positive" ? sets.animaPositive : sets.animaNegative);
  }
  if (family === "illustrious") {
    return splitTagSet(
      side === "positive" ? sets.illustriousPositive : sets.illustriousNegative,
    );
  }
  if (family === "pony") {
    return splitTagSet(side === "positive" ? sets.ponyPositive : sets.ponyNegative);
  }
  if (family === "nanosaur") {
    return splitTagSet(side === "positive" ? sets.nanosaurPositive : sets.nanosaurNegative);
  }
  return new Set();
}
