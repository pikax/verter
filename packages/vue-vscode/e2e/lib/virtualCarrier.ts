/** Generated carrier names that must never escape through authored-source navigation. */
export const VIRTUAL_CARRIER_PATTERN =
  /(?:\.(?:vue|svelte)\.(?:tsx|jsx|verter\.ts|d\.ts|__verter_test\.ts)|\.d\.(?:vue|svelte)\.ts)$/i;

export function isVirtualCarrierPath(file: string): boolean {
  return VIRTUAL_CARRIER_PATTERN.test(file);
}
