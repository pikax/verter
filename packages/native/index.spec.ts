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
});

// Tier 6 §8.2 / T9.5 — close-host repeated-spawn lifecycle test.
//
// The test runs in `describe.sequential` so it does not race with
// other native-host-creating tests in this file (and other files in
// the package) when vitest schedules them in parallel. The
// repeated-close cycle is sensitive to CPU pressure: on Windows,
// each `host.close()` joins and respawns the scheduler driver
// thread, and OS-thread spawn/teardown latency compounds quickly
// when other tests are simultaneously instantiating hosts.
//
// Root-cause investigation (per the brief). The 4-second budget is
// the discriminator. Profiling on Windows showed `host.close()` in
// `verter_session::host_lifecycle::close` calls
// `scheduler.reset()` → `restart_driver()`, which:
//
//   1. Joins the existing scheduler driver thread (~5-50ms).
//   2. Spawns a NEW driver thread (~50-200ms on Windows).
//
// For a host being thrown away the restart is wasted work, but the
// fix lives at the verter_napi/verter_session API boundary: a new
// `dispose()` method that performs cleanup without restarting, OR
// an `is_terminal_close: bool` parameter to `close()`. Both expand
// scope beyond Tier 6 dev-experience. A `TODO(follow-up)` comment
// in `crates/verter_session/src/host_lifecycle.rs::close` documents
// the architectural fix; this test sequencing is the immediate
// hardening that prevents flakes when the package's other tests
// pressure the Windows OS-thread scheduler in parallel.
const isWindowsHost = process.platform === "win32";

describe.sequential("VerterHost close lifecycle (Tier 6 §8.2 / T9.5)", () => {
  // The brief's named discriminating test:
  // `windows_close_native_hosts_promptly_serial`. Skipped on
  // non-Windows hosts (NOT vacuously passing — the brief gates the
  // discriminator on `cfg(target_os = "windows")` equivalent).
  it.runIf(isWindowsHost)("windows_close_native_hosts_promptly_serial", { timeout: 10000 }, () => {
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

  // Cross-platform companion: even on macOS / Linux the close
  // cycle should complete promptly. This characterizes the
  // baseline behavior so the fix never silently regresses on
  // non-Windows runners. Same script, looser budget (12s) because
  // CI on slow Linux runners (qemu-emulated arm64) needs headroom.
  it("close lets repeated native hosts exit promptly", { timeout: 20000 }, () => {
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
      timeout: 12000,
    });
    const elapsedMs = Date.now() - started;

    expect(output).toBe("closed");
    expect(elapsedMs).toBeLessThan(12000);
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
    // The three audit bindings must all be
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
    // empty-footprint bundle.
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
    // With audit_enabled +
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
    // u64 fields must be decimal strings.
    expect(typeof bundle.record.request_id).toBe("string");
    expect(bundle.record.request_id).toMatch(/^[0-9]+$/);
    // i64 fields are likewise decimal strings.
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
    // The audit JSON round-trip through
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
    // Binding-shape rule: the audit NAPI
    // methods are SYNCHRONOUS. "Promise resolves
    // synchronously" means the binding hands the audit record
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
      "analyzeWithAudit",
      "applyBlockOverrides",
      "auditWorkspaceOp",
      "close",
      "collectResolvableModuleReferenceSpecifiers",
      "compileMany",
      "compileWithAudit",
      "configureProjects",
      "evaluateTypeExpressionWithAudit",
      "evaluateTypes",
      "getAnalysis",
      "getAuditRecords",
      "getBundlerBatchSummary",
      "getCodeActions",
      "getDocumentSymbols",
      "getIde",
      "getLastAuditRecord",
      "getLintRuleMetadata",
      "getPublicApi",
      "getVirtualFile",
      "lint",
      "listSymbols",
      "listVirtualFiles",
      "matchCssSelectors",
      "remove",
      "resolve",
      "resolveExports",
      "resolveImport",
      "resolveKnownModuleReferenceDependencies",
      "resolveSymbolWithAudit",
      "resolveTypeWithAudit",
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

  it("top-level exports should include processStyle and VerterHost", () => {
    const native = require("./index.js");
    expect(typeof native.processStyle).toBe("function");
    expect(typeof native.VerterHost).toBe("function");
    // `compileBatch` does not exist; batch SFC compile is
    // the `host.compileMany` instance method. Negative assertion to
    // catch any future re-export attempt.
    expect((native as { compileBatch?: unknown }).compileBatch).toBeUndefined();
  });

  it("prefers the canonical verter-native binary when loading from dist", () => {
    const indexPath = require.resolve("./index.js");
    // The root wrapper delegates to the NAPI-generated loader at
    // `./dist/index.js`, which is what actually `require`s the `.node`.
    // Evict BOTH the wrapper and the loader from the module cache so the
    // re-require below re-runs the loader's binary resolution (otherwise
    // the cached loader short-circuits and no `.node` is re-loaded).
    const loaderPath = require.resolve("./dist/index.js");
    const nativeNodeModules = Object.keys(require.cache).filter(
      (entry) =>
        entry.includes(`${sep}packages${sep}native${sep}dist${sep}`) && entry.endsWith(".node"),
    );

    delete require.cache[indexPath];
    delete require.cache[loaderPath];
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

// =============================================================================
// VerterHost.compileMany E2E
// =============================================================================

describe("VerterHost.compileMany", () => {
  it("compiles a single SFC end-to-end", () => {
    const host = new VerterHost();
    const r = host.compileMany(
      [{ canonicalId: "/A.vue", source: "<template><div>x</div></template>" }],
      {},
    );
    expect(r).toHaveLength(1);
    expect(r[0].errors).toEqual([]);
    expect(r[0].code.length).toBeGreaterThan(0);
    expect(r[0].canonicalId).toBe("/A.vue");
    expect(r[0].cacheHit).toBe(false);
  });

  it("isolates per-file errors", () => {
    const host = new VerterHost();
    const r = host.compileMany(
      [
        { canonicalId: "/A.vue", source: "<template><div>good</div></template>" },
        { canonicalId: "/B.vue", source: "<template><div>{{ unclosed </template>" },
        { canonicalId: "/C.vue", source: "<template><div>also good</div></template>" },
      ],
      {},
    );
    expect(r).toHaveLength(3);
    expect(r[0].errors).toEqual([]);
    expect(r[1].errors.length).toBeGreaterThanOrEqual(1);
    expect(r[2].errors).toEqual([]);
  });

  it("warm-hits compile_cache on second call", () => {
    const host = new VerterHost();
    const inputs = [{ canonicalId: "/A.vue", source: "<template><div>x</div></template>" }];
    const r1 = host.compileMany(inputs, {});
    const r2 = host.compileMany(inputs, {});
    expect(r1[0].cacheHit).toBe(false);
    expect(r2[0].cacheHit).toBe(true);
    expect(r1[0].code).toBe(r2[0].code);
  });

  it("reports a content-addressed warm hit on the second Content call", () => {
    // A production host (dev mode off) is required: the default dev host
    // fires HasDevLastGood and downgrades every Content request to
    // Stateless, which never warm-hits. A fact-free SFC under a Content
    // request runs as Content and publishes a content-addressed entry.
    const host = new VerterHost({ devMode: false, compileErrorPolicy: "strict" });
    const inputs = () => [
      {
        canonicalId: "/W.vue",
        source:
          '<script setup lang="ts">const n = 1</script><template><div>{{ n }}</div></template>',
        requestedMode: "content" as const,
      },
    ];
    const r1 = host.compileMany(inputs(), {});
    expect(r1[0].actualMode).toBe("content");
    expect(r1[0].cacheHit).toBe(false);

    const r2 = host.compileMany(inputs(), {});
    expect(r2[0].actualMode).toBe("content");
    // The content-addressed warm hit is invisible to a session-slot-only
    // probe; sourcing cacheHit from the compile response makes it true.
    expect(r2[0].cacheHit).toBe(true);
  });

  it("carries the true downgraded mode on a Content compile error", () => {
    // defineProps<Props>() makes Props a macro type dep (HasMacroTypeDeps),
    // and importing the missing './missing' module makes the compile fail.
    // A Content request downgrades to Stateless for HasMacroTypeDeps before
    // erroring, so the error entry must report the real mode + reason.
    const host = new VerterHost({ devMode: false, compileErrorPolicy: "strict" });
    const r = host.compileMany(
      [
        {
          canonicalId: "/E.vue",
          source:
            "<script setup lang=\"ts\">\nimport type { Props } from './missing';\ndefineProps<Props>();\n</script>\n<template><div/></template>",
          requestedMode: "content" as const,
        },
      ],
      {},
    );
    expect(r[0].errors.length).toBeGreaterThanOrEqual(1);
    expect(r[0].requestedMode).toBe("content");
    // An error arm that reset to the requested mode would report "content"
    // and no reason; reading the compile-failure payload reports the truth.
    expect(r[0].actualMode).toBe("stateless");
    expect(r[0].downgradeReason).toBe("HasMacroTypeDeps");
  });

  it("accepts priority='interactive'", () => {
    const host = new VerterHost();
    const r = host.compileMany(
      [{ canonicalId: "/A.vue", source: "<template><div>x</div></template>" }],
      { priority: "interactive" },
    );
    expect(r[0].errors).toEqual([]);
  });

  it("rejects invalid priority", () => {
    const host = new VerterHost();
    expect(() =>
      host.compileMany(
        [{ canonicalId: "/A.vue", source: "<template></template>" }],
        // @ts-expect-error — testing runtime validation of an invalid string
        { priority: "urgent" },
      ),
    ).toThrow(/invalid priority/);
  });

  // hostCpuThreads is a real, typed (`u32`) field on the NAPI host
  // config: `FfiHostConfig → HostConfig` forwarding and the
  // `Option<usize>::filter(|&n| n > 0).unwrap_or(available_parallelism)`
  // pool-sizing resolution are characterised Rust-side by
  // `verter_ffi::convert::tests::host_cpu_threads_forwards_to_host_config`
  // and `verter_session::host_compile_tests::host_cpu_threads_some_{zero,explicit}_*`
  // (which read the resolved worker count via `pool_thread_count()`).
  // The JS surface exposes no pool introspection, so this spec's only
  // honest job is to pin the *wire/type surface*: that `hostCpuThreads`
  // is a real NAPI-decoded `u32` field, not an ignored extra key.
  //
  // DISCRIMINATION (the reason each test below pairs a valid value with a
  // bad-typed one): napi-derive emits `let hostCpuThreads: Option<u32> =
  // obj.get("hostCpuThreads")?` during constructor-argument binding. An
  // absent or `undefined` key yields `None`; a present value is decoded
  // through `u32::from_napi_value` → `napi_get_value_uint32`, which
  // returns `napi_number_expected` (and throws) for any non-number JS type.
  // If `hostCpuThreads` were dropped
  // from `NapiHostConfig`, no `obj.get("hostCpuThreads")` would be
  // generated, the bad value would be an ignored extra key, the
  // constructor would NOT throw, and the `toThrow()` arm would FAIL —
  // which is exactly the wire regression these tests exist to catch.
  // (Removing the field from the `HostConfig` TS type independently
  // breaks the valid-construction lines under any spec typecheck.)
  it("accepts hostCpuThreads = 2 (valid u32) and rejects a non-numeric value", () => {
    const host = new VerterHost({ hostCpuThreads: 2 });
    const r = host.compileMany(
      [{ canonicalId: "/host-cpu-threads-2.vue", source: "<template><div>x</div></template>" }],
      {},
    );
    expect(r).toHaveLength(1);
    expect(r[0].errors).toEqual([]);
    expect(r[0].code.length).toBeGreaterThan(0);

    expect(
      // @ts-expect-error — exercising NAPI runtime rejection of a non-numeric hostCpuThreads
      () => new VerterHost({ hostCpuThreads: "nope" }),
    ).toThrow();
  });

  it("accepts hostCpuThreads = 0 (documented Some(0)→None normalisation) and rejects a non-numeric value", () => {
    // Documented contract: Some(0) is normalised to None so a
    // misconfigured caller still gets a working host pool. 0 is a valid
    // u32, so the wire accepts it and compileMany completes.
    const host = new VerterHost({ hostCpuThreads: 0 });
    const r = host.compileMany(
      [{ canonicalId: "/host-cpu-threads-0.vue", source: "<template><div>x</div></template>" }],
      {},
    );
    expect(r).toHaveLength(1);
    expect(r[0].errors).toEqual([]);
    expect(r[0].code.length).toBeGreaterThan(0);

    // Same wire discriminator as above: a non-numeric value makes
    // `napi_get_value_uint32` return `NumberExpected`. (A negative JS
    // *number* would NOT discriminate — `napi_get_value_uint32` coerces
    // it via ToUint32 rather than throwing — so we use a non-number type.)
    // Dropping the field makes this an ignored key → no throw → FAILS.
    expect(
      // @ts-expect-error — exercising NAPI runtime rejection of a non-numeric hostCpuThreads
      () => new VerterHost({ hostCpuThreads: {} }),
    ).toThrow();
  });
});
