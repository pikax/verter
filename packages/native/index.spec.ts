/**
 * @ai-generated - Tests for @verter/native exports.
 * Verifies that VerterHost and processStyle work correctly with both string and Buffer inputs.
 */
import { describe, it, expect } from "vitest";
import { VerterHost, processStyle } from "./index.js";

const SFC_INPUT =
  '<script setup>\nconst msg = "hello"\n</script>\n<template><div>{{ msg }}</div></template>';

describe("VerterHost", () => {
  it("should compile a simple SFC via upsert + getVirtualFile (string source)", () => {
    const host = new VerterHost();
    const result = host.upsert({
      inputId: "Test.vue",
      source: SFC_INPUT,
    });

    expect(result.canonicalId).toBeTruthy();
    expect(result.changed).toBe(true);

    const mainFile = host.getVirtualFile({
      canonicalId: result.canonicalId,
      nodeKind: { kind: "main" },
    });

    expect(mainFile.code).toBeTruthy();
    expect(mainFile.code).toContain("_sfc_main");
  });

  it("should accept Buffer as source in upsert", () => {
    const host = new VerterHost();
    const result = host.upsert({
      inputId: "BufferTest.vue",
      source: Buffer.from(SFC_INPUT, "utf-8"),
    });

    expect(result.canonicalId).toBeTruthy();
    expect(result.changed).toBe(true);

    const mainFile = host.getVirtualFile({
      canonicalId: result.canonicalId,
      nodeKind: { kind: "main" },
    });
    expect(mainFile.code).toContain("_sfc_main");
  });

  it("should strip TypeScript when forceJs is set in compile profile", () => {
    const host = new VerterHost();
    host.upsert({
      inputId: "TypedComponent.vue",
      source: '<script setup lang="ts">\nconst x: number = 1;\n</script>\n<template><div>{{ x }}</div></template>',
    });

    const mainFile = host.getVirtualFile({
      canonicalId: "TypedComponent.vue",
      nodeKind: { kind: "main" },
      compileProfile: { forceJs: true },
    });

    expect(mainFile.code).toContain("const x");
    expect(mainFile.code).not.toContain(": number");
  });
});

describe("VerterHost type declarations in sync with native binary", () => {
  // This test ensures that the TypeScript type declarations in index.ts
  // stay in sync with the actual methods exposed by the Rust NAPI binary.
  // It catches regressions like getPublicApi being removed from the TS types
  // but still existing in the native binary.

  // Methods that are intentionally not exposed in the public TS types.
  // They exist in the native binary but are internal / feature-gated.
  const INTERNAL_METHODS = new Set(["computeCrossFileOptimizations", "getMetrics"]);

  it("every native prototype method should have a TS type declaration", () => {
    const nativeMethods = Object.getOwnPropertyNames(VerterHost.prototype)
      .filter((name) => name !== "constructor" && typeof (VerterHost.prototype as any)[name] === "function")
      .filter((name) => !INTERNAL_METHODS.has(name))
      .sort();

    // These are the methods declared in `export declare class VerterHost` in index.ts.
    // If a new method is added to the Rust NAPI impl, it must be added here AND
    // to the `export declare class VerterHost` block in index.ts.
    const declaredMethods = [
      "applyBlockOverrides",
      "applyStyleOverrides",
      "getAnalysis",
      "getCodeActions",
      "getDocumentSymbols",
      "getIde",
      "getLintRuleMetadata",
      "getPublicApi",
      "getVirtualFile",
      "lint",
      "listVirtualFiles",
      "matchCssSelectors",
      "remove",
      "resolve",
      "setImportDependencies",
      "upsert",
    ].sort();

    // Check for methods in native binary but missing from TS declarations
    const missingFromTs = nativeMethods.filter((m) => !declaredMethods.includes(m));
    expect(missingFromTs, `Native methods missing from TS type declarations (update index.ts): ${missingFromTs.join(", ")}`).toEqual([]);

    // Check for methods in TS declarations but missing from native binary
    const missingFromNative = declaredMethods.filter((m) => !nativeMethods.includes(m));
    expect(missingFromNative, `TS declarations reference non-existent native methods: ${missingFromNative.join(", ")}`).toEqual([]);
  });

  it("top-level exports should include processStyle and compileBatch", () => {
    const native = require("./index.js");
    expect(typeof native.processStyle).toBe("function");
    expect(typeof native.compileBatch).toBe("function");
    expect(typeof native.VerterHost).toBe("function");
  });
});

describe("processStyle", () => {
  it("should scope CSS selectors (string input)", () => {
    const result = processStyle(".foo { color: red }", {
      scopeId: "abc123",
      scoped: true,
    });

    expect(result.code).toContain("abc123");
  });

  it("should scope CSS selectors (Buffer input)", () => {
    const result = processStyle(Buffer.from(".foo { color: red }"), {
      scopeId: "abc123",
      scoped: true,
    });

    expect(result.code).toContain("abc123");
  });
});
