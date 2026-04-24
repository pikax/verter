/**
 * @ai-generated - Tests for @verter/native exports.
 * Verifies that VerterHost and processStyle work correctly with both string and Buffer inputs.
 */
import { basename, sep } from "node:path";
import { execFileSync } from "node:child_process";
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

  it("close lets repeated native hosts exit promptly", { timeout: 10000 }, () => {
    const nativeEntry = require.resolve("./index.js");
    const script = `
      const { VerterHost } = require(${JSON.stringify(nativeEntry)});
      const source = ${JSON.stringify(SFC_INPUT)};
      for (let i = 0; i < 8; i++) {
        const host = new VerterHost();
        host.upsert({ inputId: \`Timed-\${i}.vue\`, source });
        host.close();
      }
      process.stdout.write("closed");
    `;

    const started = Date.now();
    const output = execFileSync(process.execPath, ["-e", script], {
      encoding: "utf8",
      timeout: 4000,
    });
    const elapsedMs = Date.now() - started;

    expect(output).toBe("closed");
    expect(elapsedMs).toBeLessThan(4000);
    expect(elapsedMs).not.toBeGreaterThanOrEqual(4000);
  });
});

describe("component-meta native aliases", () => {
  it("exposes ComponentMetaHost and ComponentMetaSession aliases", () => {
    const native = require("./index.js");

    expect(native.ComponentMetaHost).toBeTypeOf("function");
    expect(native.ComponentMetaSession).toBeTypeOf("function");
    expect(native.ComponentMetaHost).toBe(native.MetaProject);
    expect(native.ComponentMetaSession).toBe(native.MetaSession);
  });

  it("does not expose removed transport benchmark methods on ComponentMetaSession", () => {
    const native = require("./index.js");
    const methods = Object.getOwnPropertyNames(native.ComponentMetaSession.prototype).sort();

    expect(methods).toContain("getComponentMeta");
    expect(methods).not.toContain("getComponentMetaBenchmarkPayloads");
    expect(methods).not.toContain("getComponentMetaFlatbuffersBenchmark");
    expect(methods).not.toContain("getComponentMetaProtobufBenchmark");
    expect(methods).not.toContain("getComponentMetaJsonBenchmark");
  });

  it("exposes synchronous audit binding methods on ComponentMetaSession", () => {
    // Plan §3 Commit 8 — the three audit bindings must all be
    // synchronous (no async fn, no Promise-returning NAPI surface).
    const native = require("./index.js");
    const methods = Object.getOwnPropertyNames(native.ComponentMetaSession.prototype).sort();

    expect(methods).toContain("getComponentMetaWithAudit");
    expect(methods).toContain("whyLoadedFromAuditJson");
    expect(methods).toContain("whyInstantiatedFromAuditJson");
  });

  it("getComponentMetaWithAudit throws when the host does not enable audit", () => {
    // Default HostConfig has audit_enabled = false. Calling the audit
    // binding must surface a clear error rather than returning an
    // empty-footprint bundle. Plan §3 Commit 8.
    const native = require("./index.js");
    const host = new native.ComponentMetaHost();
    const session = host.openSession();
    expect(() => session.getComponentMetaWithAudit("/Missing.vue")).toThrow(
      /audit is not enabled/i,
    );
    session.close();
    host.shutdown();
  });

  it("napi_get_component_meta_with_audit_returns_populated_footprint", () => {
    // Plan §3 Commit 8 test list entry. With audit_enabled +
    // footprint_capture on, the binding returns a Buffer containing
    // `{ analysis, resolution, record }` JSON — and the record's
    // `footprint` field MUST be populated (non-null, with at least
    // the structural record-vectors present). A regression that
    // accidentally drops the footprint attach (e.g. miner call gets
    // gated behind a flag that defaults off) surfaces here.
    //
    // Discriminating: `bundle.record.footprint === null` fails the
    // final assertion; a footprint whose structural shape regresses
    // (missing a record-vector) fails the per-lane assertions.
    const native = require("./index.js");
    const host = new native.ComponentMetaHost({
      auditEnabled: true,
      footprintCapture: true,
    });
    host.upsertBase(
      "/Widget.vue",
      '<script setup lang="ts">\nconst n: number = 1\n</script>\n<template><div>{{ n }}</div></template>',
    );
    const session = host.openSession();
    const buffer = session.getComponentMetaWithAudit("/Widget.vue");
    expect(buffer).toBeTruthy();
    const bundle = JSON.parse((buffer as Buffer).toString("utf-8"));
    expect(bundle).toHaveProperty("analysis");
    expect(bundle).toHaveProperty("resolution");
    expect(bundle).toHaveProperty("record");
    expect(bundle.record).toHaveProperty("request_id");
    // u64 fields must be decimal strings per plan §1.4 / §3.B.
    expect(typeof bundle.record.request_id).toBe("string");
    expect(bundle.record.request_id).toMatch(/^[0-9]+$/);
    // i64 fields likewise after §3.B Commit 7.A.
    expect(typeof bundle.record.memory.process_rss_delta_bytes).toBe("string");

    // Footprint attach invariant — the plan-required test name
    // specifies "returns populated footprint". The footprint must
    // be non-null with every structural record-vector present as
    // an array (empty is fine for the minimal SFC; the invariant
    // pins the shape, not the content).
    expect(
      bundle.record.footprint,
      "footprint must be attached when audit_enabled + footprint_capture are both on",
    ).not.toBeNull();
    const fp = bundle.record.footprint;
    for (const lane of [
      "vfs_reads",
      "shared_load_reuses",
      "indexed_ready_builds",
      "instantiations",
      "projections",
      "conditional_decisions",
      "substitutions",
      "alias_resolutions",
      "materializations",
    ] as const) {
      expect(Array.isArray(fp[lane]), `footprint.${lane} must be an array`).toBe(true);
    }
    expect(fp).toHaveProperty("derivation_subgraph");
    expect(Array.isArray(fp.derivation_subgraph.nodes)).toBe(true);
    expect(Array.isArray(fp.derivation_subgraph.edges)).toBe(true);

    session.close();
    host.shutdown();
  });

  it("whyLoadedFromAuditJson round-trips a provenance chain via the Rust walker", () => {
    const native = require("./index.js");
    const host = new native.ComponentMetaHost({
      auditEnabled: true,
      footprintCapture: true,
    });
    host.upsertBase(
      "/Widget.vue",
      '<script setup lang="ts">\nconst n: number = 1\n</script>\n<template><div>{{ n }}</div></template>',
    );
    const session = host.openSession();
    const buffer = session.getComponentMetaWithAudit("/Widget.vue");
    const auditJson = (buffer as Buffer).toString("utf-8");
    const chainJson = session.whyLoadedFromAuditJson(auditJson, "/Widget.vue");
    const chain = JSON.parse(chainJson);
    // Either a complete walk or NotFound — both are valid shapes; the
    // test asserts the walker binding produces a parseable
    // ProvenanceChain JSON regardless of fixture-specific structure.
    expect(chain).toHaveProperty("steps");
    expect(chain).toHaveProperty("terminated");
    expect(chain).toHaveProperty("shared_load_terminals");
    expect(Array.isArray(chain.steps)).toBe(true);
    session.close();
    host.shutdown();
  });

  it("napi_audit_json_round_trips_through_typescript_json_parse_stringify", () => {
    // Plan §3 Commit 8 test list. The audit JSON round-trip through
    // `JSON.parse`/`JSON.stringify` is the path the hover / playground
    // consumers take — they stringify, ship over a transport, parse,
    // re-stringify before handing to `whyLoadedFromAuditJson`. This
    // test simulates the round-trip and then exercises the walker.
    //
    // Discriminating: if any field in the audit record fails to
    // round-trip as valid JSON (e.g. a Rust-side change drops a
    // `#[serde(skip)]` onto a required field, or a `u64` is
    // accidentally serialized as a non-string JavaScript Number that
    // loses precision), the re-stringified payload handed to the
    // walker deserializes to a `RustAuditRecord` with missing / wrong
    // fields and the walker either throws or produces a surprise
    // chain shape.
    const native = require("./index.js");
    const host = new native.ComponentMetaHost({
      auditEnabled: true,
      footprintCapture: true,
    });
    host.upsertBase(
      "/Widget.vue",
      '<script setup lang="ts">\nconst n: number = 1\n</script>\n<template><div>{{ n }}</div></template>',
    );
    const session = host.openSession();
    const buffer = session.getComponentMetaWithAudit("/Widget.vue");
    expect(buffer, "audit binding must return a Buffer payload").toBeTruthy();
    const originalJson = (buffer as Buffer).toString("utf-8");

    // Round-trip: parse → stringify.
    const parsed = JSON.parse(originalJson);
    const reStringified = JSON.stringify(parsed);

    // Structural parity: re-parsing the re-stringified form must
    // produce a value deeply equal to the first parse. If the
    // serializer used any non-deterministic key ordering JSON.stringify
    // would collapse it — we still want to guarantee deep equality.
    const reParsed = JSON.parse(reStringified);
    expect(reParsed).toEqual(parsed);

    // The u64-as-string invariant must survive the round-trip.
    expect(typeof reParsed.record.request_id).toBe("string");
    expect(reParsed.record.request_id).toMatch(/^[0-9]+$/);

    // The walker must consume the re-stringified form and produce a
    // ProvenanceChain with the documented shape.
    const chainJson = session.whyLoadedFromAuditJson(reStringified, "/Widget.vue");
    const chain = JSON.parse(chainJson);
    expect(chain).toHaveProperty("steps");
    expect(chain).toHaveProperty("terminated");
    expect(chain).toHaveProperty("shared_load_terminals");
    expect(Array.isArray(chain.steps)).toBe(true);
    expect(Array.isArray(chain.shared_load_terminals)).toBe(true);

    session.close();
    host.shutdown();
  });

  it("napi_take_audit_record_called_synchronously_before_promise_resolves", () => {
    // Plan §3 Commit 8 binding-shape decision 1: the audit NAPI
    // methods are SYNCHRONOUS. The plan's phrase "Promise resolves
    // synchronously" meant that the binding hands the audit record
    // back on the same call that produced it — NOT that the binding
    // uses `async fn`. This test pins that decision: the Buffer must
    // be observable on the same microtask as the call that produced
    // it, with no Promise-resolution dance required.
    //
    // Discriminating: if someone ships a future refactor that wraps
    // the Rust side in `tokio::spawn` or replaces the sync NAPI
    // binding with an `#[napi(async)]` method, the return type
    // changes from `Buffer | null` to `Promise<Buffer | null>` and
    // the synchronous `instanceof Buffer` check below fails before
    // any `await` resolves.
    const native = require("./index.js");
    const host = new native.ComponentMetaHost({
      auditEnabled: true,
      footprintCapture: true,
    });
    host.upsertBase(
      "/Sync.vue",
      '<script setup lang="ts">\nconst m: number = 2\n</script>\n<template><div>{{ m }}</div></template>',
    );
    const session = host.openSession();

    const result = session.getComponentMetaWithAudit("/Sync.vue");

    // The return value MUST be a Buffer (or null), never a Promise.
    expect(result).not.toBeInstanceOf(Promise);
    expect(
      result === null || Buffer.isBuffer(result),
      "sync NAPI must return Buffer | null on the same tick",
    ).toBe(true);

    // The walker binding on the same session must also be sync.
    const auditJson = (result as Buffer).toString("utf-8");
    const walkerResult = session.whyLoadedFromAuditJson(auditJson, "/Sync.vue");
    expect(walkerResult).not.toBeInstanceOf(Promise);
    expect(typeof walkerResult).toBe("string");

    const walkerInst = session.whyInstantiatedFromAuditJson(
      auditJson,
      "/Sync.vue",
      "Sync",
      "0".repeat(32),
    );
    expect(walkerInst).not.toBeInstanceOf(Promise);
    expect(typeof walkerInst).toBe("string");

    session.close();
    host.shutdown();
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
      "configureProjects",
      "evaluateTypes",
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
      "resolveImport",
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
