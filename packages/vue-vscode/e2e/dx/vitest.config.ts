import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

import { defineConfig } from "vitest/config";

// Hermetic unit gate for the extension-host DX driver. It exercises the PURE
// cores of the DX helpers (launch wiring, settings verification, the
// matching-generation startup gate, the incremental typing helper, the real
// accept-path sequencing, and the log-canary verdict) with in-memory fakes — no
// real VS Code, no harness build, no network. The env-gated real launch lives in
// `test:e2e:dx` and is deliberately NOT part of this gate.
export default defineConfig({
  test: {
    root: dirname(fileURLToPath(import.meta.url)),
    // Only the hermetic `*.unit.test.ts` cores run here. `dx-harness.test.ts` is the
    // real-VS-Code mocha suite (it imports the `vscode` runtime) and is excluded.
    include: ["**/*.unit.test.ts"],
    exclude: ["**/node_modules/**", "**/dist/**", "**/out-test/**", "**/dx-harness.test.ts"],
    globals: false,
    testTimeout: 20_000,
  },
});
