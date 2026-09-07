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

import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import * as wasm from "../wasm/verter_wasm.js";
import * as pkg from "./index.js";

const WASM_BINARY_PATH = resolve(import.meta.dirname, "../wasm/verter_wasm_bg.wasm");
const WASM_DECLARATIONS_PATH = resolve(import.meta.dirname, "../wasm/verter_wasm.d.ts");

/** Every binding `initialize()` reads off the imported module. */
const REQUIRED_ARTIFACT_EXPORTS = ["default", "initSync", "VerterHost"] as const;

/**
 * Every host method the wrapper forwards to. The wrapper reaches these off
 * the constructed instance, so an absent one throws at call time on a fully
 * initialized module — the exact defect `compile`/`compileSync` were.
 */
const REQUIRED_HOST_METHODS = [
  "resolve",
  "upsert",
  "compileRequest",
  "listVirtualFiles",
  "remove",
  "getAnalysis",
  "setImportDependencies",
  "collectResolvableModuleReferenceSpecifiers",
  "resolveKnownModuleReferenceDependencies",
  "lint",
  "getCodeActions",
  "getLintRuleMetadata",
  "getDocumentSymbols",
  "matchCssSelectors",
] as const;

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

  it("the generated host object carries every method the wrapper forwards", () => {
    // The generated JS object, not the Rust source: a Rust method with no
    // binding annotation compiles, is reachable from Rust tests, and is
    // absent here. Reading the prototype needs no WASM instance, so this
    // asks the same question a browser caller does before it ever calls.
    const prototype = (wasm.VerterHost as unknown as { prototype: Record<string, unknown> })
      .prototype;
    const missing = REQUIRED_HOST_METHODS.filter((name) => typeof prototype[name] !== "function");
    expect(
      missing,
      "the wrapper forwards to these; an absent one throws at call time, not here",
    ).toEqual([]);
  });

  it("the generated declarations declare the typed compile entry", () => {
    // The wrapper's own declaration says a `compileRequest` exists on the
    // binding. That claim is only true if the GENERATED declarations say so
    // too — a hand-written binding interface can name a method the artifact
    // never had, which is how the removed standalone compile survived.
    const declarations = readFileSync(WASM_DECLARATIONS_PATH, "utf8");
    expect(
      /^\s*compileRequest\(/m.test(declarations),
      `no \`compileRequest\` method declared in ${WASM_DECLARATIONS_PATH}`,
    ).toBe(true);
  });
});
