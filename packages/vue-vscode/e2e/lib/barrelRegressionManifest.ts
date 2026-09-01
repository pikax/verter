/**
 * Focused barrel-export regression contract.
 *
 * The broader legacy barrel fixture also exercises unfinished IDE features.
 * These nine rows are the typed public-surface checks that must stay green on
 * every required provider route.
 */
export const BARREL_REGRESSION_SUITE_GLOB = "barrel-type-integrity.test";

export const BARREL_REGRESSION_LOADED_FILES = ["barrel-type-integrity.test.js"] as const;

export const BARREL_REGRESSION_TEST_IDS = [
  "hover on <Button> tag shows label, disabled, size props",
  "hover on <Overlay> tag shows zIndex, duration, show, lockScroll props",
  "completions inside <Button > include label, disabled, size, @click",
  "completions inside <Overlay > include zIndex, duration, show, lockScroll",
  "hover on Button import binding shows component type, not any",
  "hover on Overlay import binding shows component type, not any",
  "hover on :show value shows boolean type",
  "hover on :zIndex prop name shows number type",
  "hover on label= string value shows string type",
] as const;

export function isBarrelRegressionFixture(fixture: string): boolean {
  return fixture === "barrel-exports";
}
