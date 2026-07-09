// Weight-button editing across the prompt weight syntaxes MooshieUI understands.
//
// The prompt textarea operates on RAW user text; the send-time translator
// (translatePromptWeightSyntax in generation.svelte.ts) converts every syntax
// to A1111 (tag:weight) form only when params are sent. So the +0.05/-0.05
// buttons must edit whatever syntax the user actually typed, IN PLACE, keeping
// that family. This module holds the pure parsing/formatting for that.
//
// Families the buttons can numerically adjust:
//   A1111:    (tag:0.95)   -> (tag:1.0)   (collapses to bare tag at 1.0)
//   NAI:      0.95::tag::  -> 1.00::tag:: (collapses to bare tag at 1.0)
//   InvokeAI: (tag)0.95    -> (tag)1.0    (collapses to bare tag at 1.0)
//   plain:    tag          -> (tag:1.05)  (wrap; existing behavior)
//
// Recognized but NOT numerically adjustable (buttons greyed out):
//   InvokeAI emphasis: (tag)-- / (tag)++
//   NAI brace wrap:    {tag} / [tag]

const A1111_RE = /^\((.+):(\d+\.?\d*)\)$/;
const NAI_NUMERIC_RE = /^(\d+\.?\d*)::([^:]+)::$/;
const INVOKEAI_TRAILING_RE = /^\((.+)\)(\d+\.?\d*)$/;
const INVOKEAI_EMPHASIS_RE = /^\(.+\)(?:\++|-+)$/;
const BRACE_WRAP_RE = /^(?:\{.+\}|\[.+\])$/;

function clampWeight(weight: number): number {
  const rounded = Math.round(weight * 100) / 100;
  return Math.max(0, Math.min(2, rounded));
}

const isNeutral = (weight: number): boolean => Math.abs(weight - 1.0) < 0.001;

/**
 * A recognized weighted form the +/- buttons cannot numerically adjust
 * (InvokeAI emphasis marks, or NAI brace/bracket wrap). The buttons grey out
 * for these; everything else (numeric families and plain text) is adjustable.
 */
export function isNonNumericWeightSelection(selected: string): boolean {
  return INVOKEAI_EMPHASIS_RE.test(selected) || BRACE_WRAP_RE.test(selected);
}

/**
 * Adjust the weight of a selected prompt fragment by `delta`, preserving its
 * syntax family. Returns the replacement text, or null when the selection is a
 * recognized-but-non-numeric weighted form (caller should no-op). Plain text is
 * wrapped in A1111 syntax, matching the existing button behavior.
 */
export function adjustWeightText(selected: string, delta: number): string | null {
  if (isNonNumericWeightSelection(selected)) return null;

  const a1111 = selected.match(A1111_RE);
  if (a1111) {
    const weight = clampWeight(parseFloat(a1111[2]) + delta);
    return isNeutral(weight) ? a1111[1] : `(${a1111[1]}:${weight.toFixed(2)})`;
  }

  const nai = selected.match(NAI_NUMERIC_RE);
  if (nai) {
    const weight = clampWeight(parseFloat(nai[1]) + delta);
    return isNeutral(weight) ? nai[2] : `${weight.toFixed(2)}::${nai[2]}::`;
  }

  const invokeai = selected.match(INVOKEAI_TRAILING_RE);
  if (invokeai) {
    const weight = clampWeight(parseFloat(invokeai[2]) + delta);
    return isNeutral(weight) ? invokeai[1] : `(${invokeai[1]})${weight.toFixed(2)}`;
  }

  // Plain text: wrap in A1111 syntax.
  const weight = clampWeight(1.0 + delta);
  return `(${selected}:${weight.toFixed(2)})`;
}
