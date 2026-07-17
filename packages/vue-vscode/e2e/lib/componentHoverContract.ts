const GENERATED_CARRIER_PATTERN = /__Verter\w*/;
const ERASED_CONSTRUCTOR_PATTERN =
  /\bnew\s*\([^)]*\.\.\.args\s*:\s*(?:any|unknown)\[\][^)]*\)\s*=>/;
const ERASED_PUBLIC_PROPS_PATTERN = /\$props\s*:\s*(?:any|unknown)\b/;

/**
 * Reject erased public component carriers while allowing `any` or `unknown`
 * inside framework-native implementation detail and documentation prose.
 */
export function assertSafeComponentHoverCarrier(hover: string): void {
  if (GENERATED_CARRIER_PATTERN.test(hover)) {
    throw new Error(`component hover leaked a generated carrier: ${hover}`);
  }
  if (ERASED_CONSTRUCTOR_PATTERN.test(hover) || ERASED_PUBLIC_PROPS_PATTERN.test(hover)) {
    throw new Error(`component hover exposed an unsafe erased public carrier: ${hover}`);
  }
}
