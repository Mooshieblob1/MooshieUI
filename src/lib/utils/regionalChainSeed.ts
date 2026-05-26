/** Stay within Number.MAX_SAFE_INTEGER so IPC/JSON does not round seeds. */
const REGIONAL_CHAIN_SEED_MOD = 2_147_483_000;

/**
 * Derive a distinct, IPC-safe seed for each regional inpaint chain step.
 * Large ComfyUI seeds (e.g. 8173306563118891294) lose precision in JSON and
 * every chain step would otherwise get the same rounded value.
 */
export function regionalChainStepSeed(baseSeed: number, regionIndex: number): number {
  if (baseSeed < 0) return -1;
  const base = Math.abs(Math.floor(baseSeed)) % REGIONAL_CHAIN_SEED_MOD;
  return (base + (regionIndex + 1) * 9973) % REGIONAL_CHAIN_SEED_MOD;
}
