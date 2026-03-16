/**
 * @ai-generated - Tests for @verter/native exports.
 * Verifies that VerterHost and processStyle work correctly with both string and Buffer inputs.
 */
import { basename, sep } from "node:path";
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

  it("should expose moduleReferences in upsert results", () => {
    const host = new VerterHost();
    const result = host.upsert({
      inputId: "Deps.vue",
      source: `<script setup lang="ts">
const view = import('./Foo.vue')
</script>
<template><div>{{ view }}</div></template>`,
    });

    expect(result.moduleReferences).toHaveLength(1);
    expect(result.moduleReferences[0].syntax).toBe("dynamicImport");
    expect(result.moduleReferences[0].analyzability).toBe("exact");
    expect(result.moduleReferences[0].literalSpecifier).toBe("./Foo.vue");
  });

  it("should strip TypeScript when forceJs is set in compile profile", () => {
    const host = new VerterHost();
    host.upsert({
      inputId: "TypedComponent.vue",
      source:
        '<script setup lang="ts">\nconst x: number = 1;\n</script>\n<template><div>{{ x }}</div></template>',
    });

    const mainFile = host.getVirtualFile({
      canonicalId: "TypedComponent.vue",
      nodeKind: { kind: "main" },
      compileProfile: { forceJs: true },
    });

    expect(mainFile.code).toContain("const x");
    expect(mainFile.code).not.toContain(": number");
  });

  it("collects exact and finite module reference candidates in encounter order", () => {
    const host = new VerterHost() as any;
    const specifiers = host.collectResolvableModuleReferenceSpecifiers([
      {
        syntax: "staticImport",
        semantics: "import",
        isTypeOnly: false,
        rawText: "'./exact'",
        literalSpecifier: "./exact",
        finiteSpecifiers: [],
        analyzability: "exact",
        spanStart: 0,
        spanEnd: 8,
        exprSpanStart: 0,
        exprSpanEnd: 8,
      },
      {
        syntax: "dynamicImport",
        semantics: "import",
        isTypeOnly: false,
        rawText: "`./${name}`",
        finiteSpecifiers: ["./components/Foo.vue", "./utils", "./exact"],
        analyzability: "finiteSet",
        spanStart: 10,
        spanEnd: 24,
        exprSpanStart: 10,
        exprSpanEnd: 24,
      },
      {
        syntax: "dynamicImport",
        semantics: "import",
        isTypeOnly: false,
        rawText: "`./${name}.vue`",
        finiteSpecifiers: [],
        staticPrefix: "./",
        analyzability: "unknownDynamic",
        spanStart: 26,
        spanEnd: 42,
        exprSpanStart: 26,
        exprSpanEnd: 42,
      },
    ]);

    expect(specifiers).toEqual(["./exact", "./components/Foo.vue", "./utils"]);
  });

  it("resolves known module reference dependencies with caller-supplied extension order", () => {
    const host = new VerterHost() as any;
    const moduleReferences = [
      {
        syntax: "staticImport",
        semantics: "import",
        isTypeOnly: false,
        rawText: "'./widget'",
        literalSpecifier: "./widget",
        finiteSpecifiers: [],
        analyzability: "exact",
        spanStart: 0,
        spanEnd: 9,
        exprSpanStart: 0,
        exprSpanEnd: 9,
      },
    ];
    const knownIds = ["src/widget.ts", "src/widget.vue"];

    expect(
      host.resolveKnownModuleReferenceDependencies("src/App.vue", moduleReferences, knownIds, [
        ".vue",
        ".ts",
      ]),
    ).toEqual(["src/widget.vue"]);
    expect(
      host.resolveKnownModuleReferenceDependencies("src/App.vue", moduleReferences, knownIds, [
        ".ts",
        ".vue",
      ]),
    ).toEqual(["src/widget.ts"]);
  });

  it("should not produce DuplicateAttribute for style + :style and same-name shorthand", () => {
    const source = `<script setup lang="ts">
// Verter — UTF-8 multibyte: «»
import { ref } from 'vue'
const stickyTop = ref(true)
const height = ref('100px')
</script>
<template>
  <div
    style="overflow: auto"
    :style="{ height }"
    :sticky-top
  >
    content
  </div>
</template>`;

    for (const input of [source, Buffer.from(source, "utf-8")]) {
      const host = new VerterHost();
      const result = host.upsert({
        inputId: "DupAttrRegression.vue",
        source: input,
      });

      const parseDup = (result.diagnostics?.diagnostics ?? []).filter(
        (d: any) => d.code === "DuplicateAttribute",
      );
      expect(parseDup, "upsert should not produce DuplicateAttribute").toEqual([]);

      const mainFile = host.getVirtualFile({
        canonicalId: result.canonicalId,
        nodeKind: { kind: "main" },
        compileProfile: { target: "ide" },
      });

      expect(mainFile.code).toBeTruthy();
      const compileDup = (mainFile.diagnostics?.diagnostics ?? []).filter(
        (d: any) => d.code === "DuplicateAttribute",
      );
      expect(compileDup, "compile should not produce DuplicateAttribute").toEqual([]);
      expect(mainFile.diagnostics?.hasErrors).toBe(false);
    }
  });

  it("returns a testing-mode public API that exposes script setup bindings", () => {
    const host = new VerterHost();
    host.upsert({
      inputId: "DebugBindings.vue",
      source: `<script setup lang="ts">
import { ref } from 'vue'
const count = ref(1)
const hidden = ref('secret')
defineExpose({ count })
</script>
<template><div>{{ count }}</div></template>`,
    });

    const publicApi = host.getPublicApi("DebugBindings.vue");
    const testingApi = host.getPublicApi("DebugBindings.vue", "testing");

    expect(publicApi?.code).toBeTruthy();
    expect(testingApi?.code).toContain("count: typeof count");
    expect(testingApi?.code).toContain("hidden: typeof hidden");
    expect(testingApi?.code).not.toContain("ref: typeof ref");
    expect(publicApi?.code).not.toContain("hidden: typeof hidden");
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
      .filter(
        (name) =>
          name !== "constructor" && typeof (VerterHost.prototype as any)[name] === "function",
      )
      .filter((name) => !INTERNAL_METHODS.has(name))
      .sort();

    // These are the methods declared in `export declare class VerterHost` in index.ts.
    // If a new method is added to the Rust NAPI impl, it must be added here AND
    // to the `export declare class VerterHost` block in index.ts.
    const declaredMethods = [
      "applyBlockOverrides",
      "close",
      "collectResolvableModuleReferenceSpecifiers",
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
      "resolveExports",
      "resolveKnownModuleReferenceDependencies",
      "setImportDependencies",
      "upsert",
    ].sort();

    // Check for methods in native binary but missing from TS declarations
    const missingFromTs = nativeMethods.filter((m) => !declaredMethods.includes(m));
    expect(
      missingFromTs,
      `Native methods missing from TS type declarations (update index.ts): ${missingFromTs.join(", ")}`,
    ).toEqual([]);

    // Check for methods in TS declarations but missing from native binary
    const missingFromNative = declaredMethods.filter((m) => !nativeMethods.includes(m));
    expect(
      missingFromNative,
      `TS declarations reference non-existent native methods: ${missingFromNative.join(", ")}`,
    ).toEqual([]);
  });

  it("top-level exports should include processStyle and compileBatch", () => {
    const native = require("./index.js");
    expect(typeof native.processStyle).toBe("function");
    expect(typeof native.compileBatch).toBe("function");
    expect(typeof native.VerterHost).toBe("function");
  });

  it("prefers the canonical verter-native binary when loading from dist", () => {
    const indexPath = require.resolve("./index.js");
    const nativeNodeModules = Object.keys(require.cache).filter(
      (entry) =>
        entry.includes(`${sep}packages${sep}native${sep}dist${sep}`) && entry.endsWith(".node"),
    );

    delete require.cache[indexPath];
    for (const entry of nativeNodeModules) {
      delete require.cache[entry];
    }

    require("./index.js");

    const loadedNodeModules = Object.keys(require.cache).filter(
      (entry) =>
        entry.includes(`${sep}packages${sep}native${sep}dist${sep}`) && entry.endsWith(".node"),
    );

    expect(loadedNodeModules).toHaveLength(1);
    expect(basename(loadedNodeModules[0])).toMatch(/^verter-native\./);
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
