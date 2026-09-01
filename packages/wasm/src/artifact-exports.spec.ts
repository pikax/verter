/**
 * The wrapper's bindings, checked against the REAL built artifact.
 *
 * `index.spec.ts` mocks `../wasm/verter_wasm.js`, so it proves only that
 * the wrapper routes correctly through whatever the mock supplies. For a
 * long time that mock supplied a `compile` export the artifact never had:
 * the package's documented headline API threw "WASM module not
 * initialized" on a fully initialized module, and sixteen green tests
 * asserted routing against a module that did not exist.
 *
 * This file closes that gap from the other side. It imports the real
 * binary and asserts, name by name, that every export the wrapper reaches
 * for is actually there — and that the removed standalone-compile surface
 * has not quietly come back through the wrapper without the artifact.
 */

import { existsSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import * as wasm from "../wasm/verter_wasm.js";
import * as pkg from "./index.js";

const WASM_BINARY_PATH = resolve(import.meta.dirname, "../wasm/verter_wasm_bg.wasm");

/** Every binding `initialize()` reads off the imported module. */
const REQUIRED_ARTIFACT_EXPORTS = ["default", "initSync", "VerterHost"] as const;

describe("@verter/wasm artifact export surface", () => {
  it("the built artifact carries every export the wrapper binds", () => {
    expect(
      existsSync(WASM_BINARY_PATH),
      `WASM binary missing at ${WASM_BINARY_PATH}. Run \`pnpm --filter @verter/wasm build:wasm\` first.`,
    ).toBe(true);

    const missing = REQUIRED_ARTIFACT_EXPORTS.filter(
      (name) => (wasm as Record<string, unknown>)[name] === undefined,
    );
    expect(
      missing,
      "the wrapper binds these names at initialize(); an absent one fails at runtime, not here",
    ).toEqual([]);
  });

  it("the package exposes the host surface and no standalone compile", () => {
    // `createHost`/`Host` ARE the package's compile entry. `compile` and
    // `compileSync` were removed because no artifact export backed them;
    // re-adding either without a matching artifact export reintroduces
    // the original defect, so both directions are asserted.
    expect(typeof pkg.initialize).toBe("function");
    expect(typeof pkg.isInitialized).toBe("function");
    expect(typeof pkg.createHost).toBe("function");
    expect(typeof pkg.Host).toBe("function");

    for (const name of ["compile", "compileSync"]) {
      const exported = (pkg as Record<string, unknown>)[name] !== undefined;
      const backed = (wasm as Record<string, unknown>)[name] !== undefined;
      expect(
        exported && !backed,
        `@verter/wasm exports \`${name}\` but the artifact does not — the wrapper would throw at call time`,
      ).toBe(false);
    }
  });
});
