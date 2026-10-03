/**
 * Hidden body recipes for the NovelAI V5 rewrite.
 *
 * Not a setting and not in the UI. A recipe joins the system turn only when the
 * user's own words ask for it by one of its trigger phrases, so a rewrite that
 * never mentions one runs on the plain specification and pays nothing for it.
 * Gating it here rather than telling the model "apply this when the user says
 * X" matters: a rule the model can see is a rule it applies to any picture that
 * looks close enough.
 *
 * Leaf util, same as `naiPrompt.ts`: it must not import any store.
 */

/**
 * Elegg (Goddess of Victory: Nikke) proportions: short but normal head size,
 * plump thighs and hips, huge breasts, soft tucked stomach.
 *
 * Every tag is a live Danbooru tag, checked against post counts in 2026-10.
 * petite, not short: short is deprecated with no posts and its wiki points at
 * short hair first. soft body, soft stomach and slim face are not tags at all,
 * and plump already carries the softness. Of the old UC only chibi, big belly
 * and obese have posts behind them; super deformed, pot belly, fat face,
 * stubby hands and short fingers are empty. shortstack is negated because
 * petite plus big curves is exactly what that tag describes, and it pulled
 * chibi and stubby hands when it was in the prompt.
 */
const DUMMY_THICC = `BODY RECIPE: DUMMY THICC
The user asked for this body by name. Apply it to the character they mean, or to the only character when there is one. Body only: do not take hair, eyes, outfit, style or setting from it.

Target: a short adult with a normal head to body ratio. Huge chest, thick thighs, wide hips, a soft midriff that stays tucked. Not a shortstack, not a belly, not chibi.

- The body section of that character's CHAR box holds exactly these tags, every time: petite, plump, thick thighs, wide hips, huge breasts
- These tags go in the CHAR box, never in the BASE tag line.
- Do not describe the body in the natural language body. The tags carry it, and prose restating them spends budget for nothing. Never write heights, comparisons to a named character, or instructions such as "face stays slim".
- On a face crop, drop thick thighs and wide hips.
- Never add short, shortstack, curvy, narrow waist or chubby. short reads as hair length, shortstack and curvy swing the body bottom heavy or fit, and chubby is the same tag as plump.
- Do not name Elegg anywhere in the prompt. The name brings her hair and outfit with it.
- UC for this body, added while these tags are in the prompt: chibi, shortstack, big belly, obese. This overrides the rule against negating chibi, because petite plus plump is what pulls toward it.
- Do not add long fingers or giant hands, and do not weight the hands.
- If the character is a named one, this body replaces their canon body. Do not restate their canon proportions.`;

/** dummy thicc / thick / thic / dummythicc, Elegg proportions or body, or the skill name. */
const DUMMY_THICC_TRIGGER = /\bdummy[\s-]*thic+k?\b|\belegg(?:'s)?\s+(?:proportions|body)\b|\bnai-dummy-thicc\b/i;

/**
 * The recipe blocks this request asked for, joined, or `""` when it asked for
 * none.
 */
export function naiRecipeDirective(texts: string[]): string {
  return wantsDummyThicc(texts) ? DUMMY_THICC : "";
}

/**
 * Whether any of the user's messages asks for the dummy thicc body.
 *
 * `texts` is the current instruction followed by the earlier user turns of the
 * session that are being sent, so a follow-up such as "now at the beach" keeps
 * the body the first turn asked for.
 */
function wantsDummyThicc(texts: string[]): boolean {
  // "same body" and "same proportions" are deliberately not triggers: on their
  // own they are ordinary prompt words ("the same body as image 1"). After a
  // real trigger they need no matching, because that earlier turn is in
  // `texts` and keeps the recipe on for the rest of the session.
  return texts.some((text) => DUMMY_THICC_TRIGGER.test(text));
}
