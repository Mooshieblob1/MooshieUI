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
 * plump thighs and hips, huge breasts, soft tucked stomach, slim face.
 *
 * The tag choices are deliberate and each one was earned against a failure:
 * shortstack pulled chibi and stubby hands, flat stomach read as a board,
 * curvy or hourglass swung the body fit, chubby swung it fat. The UC is the
 * one place the spec's "never negate chibi on a full size character" rule
 * gives way, because short plus plump is exactly what pulls that way.
 */
const DUMMY_THICC = `BODY RECIPE: DUMMY THICC
The user asked for this body by name. Apply it to the character they mean, or to the only character when there is one. Body only: do not take hair, eyes, outfit, style or setting from it.

Target: a short adult, about 148 cm, with a normal head to body ratio. Huge chest, thick thighs, wide hips, a soft midriff that stays tucked, soft arms, a slim face. Not a shortstack, not a belly, not a chubby face.

- The body section of that character's CHAR box holds exactly these tags, every time: short, plump, soft body, thick thighs, wide hips, soft stomach, huge breasts, slim face
- These tags go in the CHAR box, never in the BASE tag line.
- In the natural language body, add one present tense sentence only if the shot can show it: She is short with a normal head size, thick thighs and hips, a huge chest, a stomach soft but tucked, and a slim face. Drop the thigh clause on a face crop. Drop the stomach clause only if the waist is fully covered.
- Never add shortstack, curvy, narrow waist, hourglass, chubby or pudgy. Never swap soft stomach for flat stomach.
- Do not name Elegg anywhere in the prompt. The name brings her hair and outfit with it.
- UC for this body, added while these tags are in the prompt: big belly, pot belly, obese, chubby face, fat face, chibi, super deformed, stubby hands, short fingers. This overrides the rule against negating chibi and stubby hands, because short plus plump is what pulls toward them.
- Do not add long fingers or giant hands, and do not weight the hands.
- If the character is a named one, this body replaces their canon body. Do not restate their canon proportions.`;

/**
 * Whether any of the user's messages asks for the dummy thicc body.
 *
 * `texts` is the current instruction followed by the earlier user turns of the
 * session that are being sent, so a follow-up such as "now at the beach" keeps
 * the body the first turn asked for.
 */
function wantsDummyThicc(texts: string[]): boolean {
  // TODO(human)
  return false;
}

/**
 * The recipe blocks this request asked for, joined, or `""` when it asked for
 * none.
 */
export function naiRecipeDirective(texts: string[]): string {
  return wantsDummyThicc(texts) ? DUMMY_THICC : "";
}
