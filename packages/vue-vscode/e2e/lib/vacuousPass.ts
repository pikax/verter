const VACUOUS_PASS_PATTERN =
  /(?:^|\s)N\/A(?:\s|$)|pass\s*\(\s*N\/A|not applicable[^\r\n]*\bpass(?:ing|ed)?\b/i;

/** Refuse runtime attempts to turn an inapplicable editor assertion into a pass. */
export function assertNotVacuousPassLog(message: string): void {
  if (VACUOUS_PASS_PATTERN.test(message)) {
    throw new Error(`VS Code E2E vacuous pass refused: ${message}`);
  }
}
