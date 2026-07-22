import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

import { defineConfig } from "vitest/config";

/**
 * Hermetic unit gate for the VS Code acceptance lane's pure cores: the
 * TypeScript-answer discriminator and the probe selector.
 *
 * The discriminator's whole purpose is to refuse to credit a Verter-native
 * hover to the TypeScript engine, so its rejection behaviour is proven here
 * against verbatim output of the Rust hover formatters — no VS Code host, no
 * network, no corpus. The real extension-host run lives in `test:e2e:acceptance`
 * and is deliberately NOT part of this gate.
 */
export default defineConfig({
  test: {
    root: dirname(fileURLToPath(import.meta.url)),
    include: ["**/*.unit.test.ts"],
    exclude: ["**/node_modules/**", "**/dist/**", "**/out-test/**", "**/suite/**"],
    globals: false,
    testTimeout: 20_000,
  },
});
