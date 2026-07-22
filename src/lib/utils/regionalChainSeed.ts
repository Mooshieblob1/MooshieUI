/** Keep derived seeds small and well-spread; BigInt keeps the math exact. */
const REGIONAL_CHAIN_SEED_MOD = 2_147_483_000n;

/**
 * Derive a distinct seed for each regional inpaint chain step.
 * Seeds are decimal strings ("-1" = random): 63-bit values exceed
 * Number.MAX_SAFE_INTEGER, so the arithmetic runs in BigInt.
 */
export function regionalChainStepSeed(baseSeed: string, regionIndex: number): string {
  let parsed: bigint;
  try {
    parsed = BigInt(baseSeed.trim());
  } catch {
    return "-1";
  }
  if (parsed < 0n) return "-1";
  const base = parsed % REGIONAL_CHAIN_SEED_MOD;
  const step = (base + BigInt(regionIndex + 1) * 9973n) % REGIONAL_CHAIN_SEED_MOD;
  return step.toString();
}
