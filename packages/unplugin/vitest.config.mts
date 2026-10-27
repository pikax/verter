// Vitest reads its config from the package root only, so the unit run
// takes the repository config explicitly. It keeps the Playwright suite
// under `e2e/` (run by `pnpm test:e2e`) out of `vitest run`.
export { default } from "../../vitest.config";
