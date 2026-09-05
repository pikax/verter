/**
 * Discriminating tests for descriptor-driven, framework-agnostic compilation.
 * Uses the `__setHostForTest` mock host so they run in the default gate without
 * loading WASM. These FAIL against a Vue-only `compileFile` (no svelte branch).
 */
import { describe, it, expect, vi } from "vitest";
import { compileFile, __setHostForTest } from "./compiler";
import { File, type PublicApiDeclarationShapeReason, type PublicApiProjectionError } from "./types";

const DECLARATION_SHAPE_REASONS = [
  "semantic-inference-depth-budget-exceeded",
  "semantic-inference-work-budget-exceeded",
  "semantic-inference-unsupported-macro-kind",
  "semantic-inference-unsupported-construct",
  "semantic-inference-missing-type-argument",
  "semantic-inference-missing-declaration",
  "semantic-inference-ambiguous-reference",
  "semantic-inference-missing-dependency",
  "owner-value-dependency-unavailable",
  "class-decorator",
  "complex-class-heritage",
  "decorated-class-member",
  "computed-class-member",
  "private-class-member",
  "rest-class-parameter",
  "destructured-class-parameter",
  "decorated-class-parameter",
  "constructor-overload",
  "unsupported-class-shape",
  "unsupported-enum-shape",
  "inconsistent-class-inference",
] as const satisfies readonly PublicApiDeclarationShapeReason[];

type SameUnion<Left, Right> = [Exclude<Left, Right>, Exclude<Right, Left>] extends [never, never]
  ? true
  : false;

const DECLARATION_SHAPE_REASON_UNION_IS_EXHAUSTIVE: SameUnion<
  PublicApiDeclarationShapeReason,
  (typeof DECLARATION_SHAPE_REASONS)[number]
> = true;

interface UpsertCall {
  inputId: string;
  fileKind: string;
}

function createMockHost(opts?: {
  virtualFileCode?: string;
  ideCode?: string;
  publicApiResult?: {
    value: { code: string; sourceMap: string | null } | null;
    error: PublicApiProjectionError | null;
  };
  publicApiResults?: Partial<
    Record<
      "public" | "declaration",
      {
        value: { code: string; sourceMap: string | null } | null;
        error: PublicApiProjectionError | null;
      }
    >
  >;
}) {
  const upsertCalls: UpsertCall[] = [];
  const upsert = vi.fn((request: { inputId: string; fileKind: string }) => {
    upsertCalls.push({ inputId: request.inputId, fileKind: request.fileKind });
    return {
      diagnostics: { diagnostics: [], hasErrors: false },
      moduleReferences: [],
      parseDurationMs: 0,
    };
  });

  const virtualFileCode = opts?.virtualFileCode ?? "export default function Component() {}";
  const ideCode = opts?.ideCode ?? "// svelte tsx\nexport function $props() {}\n";

  class MockHost {
    upsert = upsert;
    listVirtualFiles = vi.fn(() => [{ kind: "main" }]);
    compileRequest = vi.fn((canonicalId: string) => ({
      canonicalId,
      diagnostics: { diagnostics: [], hasErrors: false },
      products: [
        {
          kind: "runtimeClient",
          nodes: [{ node: { kind: "main" }, code: virtualFileCode, meta: {} }],
        },
        {
          kind: "ideCompanion",
          code: ideCode,
          sourceMap: '{"version":3,"mappings":""}',
        },
      ],
    }));
    getAnalysis = vi.fn(() => null);
    // Mode-aware: the DECLARATION surface is a distinct output from the
    // public API surface (mirrors the WASM host's getPublicApi(id, mode)).
    getPublicApi = vi.fn((_id: string, mode?: string) => {
      const normalizedMode = mode === "declaration" ? "declaration" : "public";
      const modeResult = opts?.publicApiResults?.[normalizedMode];
      if (modeResult) return modeResult;
      if (opts?.publicApiResult) {
        return normalizedMode === "public" ? opts.publicApiResult : { value: null, error: null };
      }
      return {
        value:
          normalizedMode === "declaration"
            ? { code: "declare const App: unknown;\nexport default App;", sourceMap: "" }
            : { code: "export {};", sourceMap: "" },
        error: null,
      };
    });
    lint = vi.fn(() => []);
  }

  return { MockHost, upsert, upsertCalls };
}

describe("compileFile — descriptor-driven framework dispatch", () => {
  it("keeps public-API reason and invalid-subject unions lossless", () => {
    expect(DECLARATION_SHAPE_REASON_UNION_IS_EXHAUSTIVE).toBe(true);
    expect(new Set(DECLARATION_SHAPE_REASONS).size).toBe(DECLARATION_SHAPE_REASONS.length);
    expect(DECLARATION_SHAPE_REASONS).toHaveLength(21);
    expect(DECLARATION_SHAPE_REASONS).not.toContain("semantic-inference-unavailable");

    // @ts-expect-error — Rust never emits the retired, lossy umbrella code.
    const retired: PublicApiDeclarationShapeReason = "semantic-inference-unavailable";
    const attrsNonObjectRoot: PublicApiProjectionError = {
      code: "tsc-generation",
      detailCode: "unavailable-outcome",
      subject: {
        kind: "scriptSetupAttrs",
        // @ts-expect-error — non-object-root requires a macro subject.
        sourceRange: { start: 1, end: 2 },
      },
      declarationShapeReason: null,
      memberOrdinal: null,
      outcomeKind: "invalid",
      outcomeReason: "non-object-root",
      outcomeDiagnostic: null,
    };
    const macroMalformedSyntax: PublicApiProjectionError = {
      code: "tsc-generation",
      detailCode: "unavailable-outcome",
      subject: {
        kind: "macro",
        // @ts-expect-error — malformed authored syntax requires an attrs subject.
        syntaxIndex: 0,
      },
      declarationShapeReason: null,
      memberOrdinal: null,
      outcomeKind: "invalid",
      outcomeReason: "malformed-or-recovered-type-syntax",
      outcomeDiagnostic: null,
    };
    expect([retired, attrsNonObjectRoot.outcomeReason, macroMalformedSyntax.outcomeReason]).toEqual(
      ["semantic-inference-unavailable", "non-object-root", "malformed-or-recovered-type-syntax"],
    );
  });

  it("preserves structured public-API failure and distinguishes ordinary absence", async () => {
    const projectionError = {
      code: "tsc-generation" as const,
      detailCode: "unsupported-declaration-shape" as const,
      subject: { kind: "macro" as const, syntaxIndex: 4 },
      declarationShapeReason: "unsupported-enum-shape" as const,
      memberOrdinal: null,
      outcomeKind: null,
      outcomeReason: null,
      outcomeDiagnostic: null,
    };
    const failing = createMockHost({ publicApiResult: { value: null, error: projectionError } });
    const teardownFailing = __setHostForTest(new failing.MockHost() as any);
    try {
      const file = new File("Unsafe.svelte", "<h1>unsafe</h1>");
      await compileFile(file);
      expect(file.compiled.tscCode).toBe("");
      expect(file.compiled.declCode).toBe("");
      expect(file.compiled.compilerDiagnostics).toContainEqual({
        severity: "error",
        code: "tsc-generation/unsupported-declaration-shape",
        message:
          "public API projection failed: tsc-generation/unsupported-declaration-shape (subject=macro(4), declarationShapeReason=unsupported-enum-shape, memberOrdinal=null, outcomeKind=null, outcomeReason=null, outcomeDiagnostic=null)",
        projectionError,
      });
      expect(file.compiled.errors).toContain(
        "[error] public API projection failed: tsc-generation/unsupported-declaration-shape (subject=macro(4), declarationShapeReason=unsupported-enum-shape, memberOrdinal=null, outcomeKind=null, outcomeReason=null, outcomeDiagnostic=null)",
      );
    } finally {
      teardownFailing();
    }

    const absent = createMockHost({ publicApiResult: { value: null, error: null } });
    const teardownAbsent = __setHostForTest(new absent.MockHost() as any);
    try {
      const file = new File("Absent.svelte", "<h1>absent</h1>");
      await compileFile(file);
      expect(file.compiled.compilerDiagnostics).toEqual([]);
      expect(file.compiled.errors).toEqual([]);
    } finally {
      teardownAbsent();
    }
  });

  it("keeps declaration output when only public mode fails", async () => {
    const projectionError: PublicApiProjectionError = {
      code: "tsc-generation",
      detailCode: "unsupported-declaration-shape",
      subject: { kind: "macro", syntaxIndex: 1 },
      declarationShapeReason: "unsupported-class-shape",
      memberOrdinal: null,
      outcomeKind: null,
      outcomeReason: null,
      outcomeDiagnostic: null,
    };
    const fixture = createMockHost({
      publicApiResults: {
        public: { value: null, error: projectionError },
        declaration: {
          value: { code: "declare const Kept: string;", sourceMap: "decl-map" },
          error: null,
        },
      },
    });
    const teardown = __setHostForTest(new fixture.MockHost() as any);
    try {
      const file = new File("PublicFails.svelte", "<h1>failure</h1>");
      await compileFile(file);

      expect(file.compiled.tscCode).toBe("");
      expect(file.compiled.publicApiOutcome).toEqual({
        kind: "projectionFailure",
        error: projectionError,
      });
      expect(file.compiled.declCode).toBe("declare const Kept: string;");
      expect(file.compiled.declSourceMap).toBe("decl-map");
      expect(file.compiled.declarationOutcome).toEqual({
        kind: "value",
        value: { code: "declare const Kept: string;", sourceMap: "decl-map" },
      });
      expect(file.compiled.compilerDiagnostics).toHaveLength(1);
    } finally {
      teardown();
    }
  });

  it("keeps public output when only declaration mode fails", async () => {
    const projectionError: PublicApiProjectionError = {
      code: "tsc-generation",
      detailCode: "unsupported-declaration-shape",
      subject: { kind: "macro", syntaxIndex: 2 },
      declarationShapeReason: "unsupported-enum-shape",
      memberOrdinal: null,
      outcomeKind: null,
      outcomeReason: null,
      outcomeDiagnostic: null,
    };
    const fixture = createMockHost({
      publicApiResults: {
        public: { value: { code: "export interface Kept {}", sourceMap: null }, error: null },
        declaration: { value: null, error: projectionError },
      },
    });
    const teardown = __setHostForTest(new fixture.MockHost() as any);
    try {
      const file = new File("DeclarationFails.svelte", "<h1>failure</h1>");
      await compileFile(file);

      expect(file.compiled.tscCode).toBe("export interface Kept {}");
      expect(file.compiled.publicApiOutcome).toEqual({
        kind: "value",
        value: { code: "export interface Kept {}", sourceMap: null },
      });
      expect(file.compiled.declCode).toBe("");
      expect(file.compiled.declSourceMap).toBe("");
      expect(file.compiled.declarationOutcome).toEqual({
        kind: "projectionFailure",
        error: projectionError,
      });
      expect(file.compiled.compilerDiagnostics).toHaveLength(1);
    } finally {
      teardown();
    }
  });

  it("preserves all unavailable-outcome arms in diagnostics", async () => {
    const cases: PublicApiProjectionError[] = [
      {
        code: "tsc-generation",
        detailCode: "unavailable-outcome",
        subject: { kind: "macro", syntaxIndex: 0 },
        declarationShapeReason: null,
        memberOrdinal: null,
        outcomeKind: "partial",
        outcomeReason: "incomplete-traversal",
        outcomeDiagnostic: "partial detail",
      },
      {
        code: "tsc-generation",
        detailCode: "unavailable-outcome",
        subject: { kind: "macro", syntaxIndex: 1 },
        declarationShapeReason: null,
        memberOrdinal: null,
        outcomeKind: "unresolved",
        outcomeReason: "ambiguous-reference",
        outcomeDiagnostic: "unresolved detail",
      },
      {
        code: "tsc-generation",
        detailCode: "unavailable-outcome",
        subject: { kind: "macro", syntaxIndex: 2 },
        declarationShapeReason: null,
        memberOrdinal: null,
        outcomeKind: "unsupported",
        outcomeReason: "semantic-construct",
        outcomeDiagnostic: "unsupported detail",
      },
      {
        code: "tsc-generation",
        detailCode: "unavailable-outcome",
        subject: { kind: "macro", syntaxIndex: 3 },
        declarationShapeReason: null,
        memberOrdinal: null,
        outcomeKind: "invalid",
        outcomeReason: "non-object-root",
        outcomeDiagnostic: "invalid detail",
      },
      {
        code: "tsc-generation",
        detailCode: "unavailable-outcome",
        subject: {
          kind: "scriptSetupAttrs",
          sourceRange: { start: 31, end: 37 },
        },
        declarationShapeReason: null,
        memberOrdinal: null,
        outcomeKind: "invalid",
        outcomeReason: "malformed-or-recovered-type-syntax",
        outcomeDiagnostic: null,
      },
    ];

    for (const projectionError of cases) {
      const fixture = createMockHost({
        publicApiResult: { value: null, error: projectionError },
      });
      const teardown = __setHostForTest(new fixture.MockHost() as any);
      try {
        const file = new File(`${projectionError.outcomeKind}.svelte`, "<h1>failure</h1>");
        await compileFile(file);

        expect(file.compiled.compilerDiagnostics).toHaveLength(1);
        expect(file.compiled.compilerDiagnostics[0]?.projectionError).toEqual(projectionError);
        expect(file.compiled.compilerDiagnostics[0]?.message).toContain(
          `outcomeKind=${projectionError.outcomeKind}, outcomeReason=${projectionError.outcomeReason}, outcomeDiagnostic=${projectionError.outcomeDiagnostic}`,
        );
      } finally {
        teardown();
      }
    }
  });

  it("compiles a .svelte file with fileKind:'svelte' and populates output panels", async () => {
    const mock = createMockHost({
      virtualFileCode: "export default class App {}",
      ideCode: "// generated svelte tsx",
    });
    const host = new mock.MockHost() as any;
    const teardown = __setHostForTest(host);
    try {
      const file = new File(
        "App.svelte",
        "<script>let count = $state(0)</script>\n<h1>{count}</h1>",
      );
      await compileFile(file);

      // Discriminator: the upsert MUST be issued with fileKind 'svelte'.
      expect(
        mock.upsertCalls.some((c) => c.inputId === "App.svelte" && c.fileKind === "svelte"),
      ).toBe(true);
      // No Vue fileKind leaked for the svelte carrier.
      expect(mock.upsertCalls.some((c) => c.inputId === "App.svelte" && c.fileKind === "vue")).toBe(
        false,
      );

      // Output panels populated.
      expect(file.compiled.js).toBe("export default class App {}");
      expect(file.compiled.types).toBe("// generated svelte tsx");
      expect(file.compiled.tscCode).toBe("export {};");
      // The DECLARATION carrier surface is produced alongside the API surface
      // (getPublicApi(id, "declaration")) — and is NOT the API code relabeled.
      expect(file.compiled.declCode).toBe("declare const App: unknown;\nexport default App;");
      expect(file.compiled.declCode).not.toBe(file.compiled.tscCode);
    } finally {
      teardown();
    }
  });

  it("does NOT apply the Vue render-merge assembly to svelte (no __sfc__ wrapping)", async () => {
    const mock = createMockHost({ virtualFileCode: "function render() {}" });
    const host = new mock.MockHost() as any;
    const teardown = __setHostForTest(host);
    try {
      const file = new File("App.svelte", "<h1>hi</h1>");
      await compileFile(file);
      // mergeRenderIntoComponent (Vue-only) would inject `const __sfc__`. Svelte must not.
      expect(file.compiled.js).toBe("function render() {}");
      expect(file.compiled.js).not.toContain("__sfc__");
      expect(file.compiled.js).not.toContain("export default __sfc__");
    } finally {
      teardown();
    }
  });

  it("compiles a .svelte.ts adapter module as svelte (longest-suffix), not non_sfc", async () => {
    const mock = createMockHost();
    const host = new mock.MockHost() as any;
    const teardown = __setHostForTest(host);
    try {
      const file = new File("store.svelte.ts", "export const count = $state(0)");
      await compileFile(file);
      expect(
        mock.upsertCalls.some((c) => c.inputId === "store.svelte.ts" && c.fileKind === "svelte"),
      ).toBe(true);
      expect(
        mock.upsertCalls.some((c) => c.inputId === "store.svelte.ts" && c.fileKind === "non_sfc"),
      ).toBe(false);
    } finally {
      teardown();
    }
  });

  it("compiles a .vue file with fileKind:'vue' (Vue path unchanged)", async () => {
    const mock = createMockHost({ virtualFileCode: "export default {}" });
    const host = new mock.MockHost() as any;
    const teardown = __setHostForTest(host);
    try {
      const file = new File("App.vue", "<template><div/></template>");
      await compileFile(file);
      expect(mock.upsertCalls.some((c) => c.inputId === "App.vue" && c.fileKind === "vue")).toBe(
        true,
      );
      // Vue render assembly still runs mergeRenderIntoComponent (main path → __sfc__).
      expect(file.compiled.js).toContain("__sfc__");
      // The Vue compile path produces the declaration surface too.
      expect(file.compiled.declCode).toBe("declare const App: unknown;\nexport default App;");
    } finally {
      teardown();
    }
  });

  it("does not copy Vue source into the typed compile request r-source-leak", async () => {
    const mock = createMockHost({ virtualFileCode: "export default {}" });
    const host = new mock.MockHost() as any;
    const teardown = __setHostForTest(host);
    try {
      const source = "<template><div/></template>";
      const file = new File("App.vue", source);
      await compileFile(file);

      expect(host.upsert).toHaveBeenCalledTimes(1);
      expect(host.upsert.mock.calls[0][0]).toEqual({
        inputId: "App.vue",
        source,
        fileKind: "vue",
        aliases: [],
      });
      expect(host.compileRequest).toHaveBeenCalledTimes(1);

      const [canonicalId, request] = host.compileRequest.mock.calls[0];
      expect(canonicalId).toBe("App.vue");
      expect(
        JSON.stringify(request).includes(JSON.stringify(source)),
        "typed request must not copy source",
      ).toBe(false);
      expect(request).toEqual({
        vue: {
          identity: { isProduction: false, forceJs: true },
          products: [
            { runtimeClient: { runtimeSourceMap: true } },
            {
              ideCompanion: {
                wantSourceMap: true,
                embedAmbientTypes: false,
                conditionalRootNarrowing: false,
                strictSlots: false,
                ideChunkBoundaries: false,
              },
            },
          ],
          options: {
            backend: "inferred",
            ssr: false,
            isCustomElement: [],
            babelParserPlugins: [],
          },
        },
      });
    } finally {
      teardown();
    }
  });
});

function createTypedCompileHost(options?: {
  clientNodes?: Array<{ node: { kind: string; index?: number }; code: string; sourceMap?: string }>;
  ide?: { code: string; sourceMap?: string };
  serverNodes?: Array<{ node: { kind: string; index?: number }; code: string; sourceMap?: string }>;
}) {
  const clientNodes = options?.clientNodes ?? [
    { node: { kind: "main" }, code: "export default {}", meta: {} },
  ];
  const compileRequest = vi.fn((canonicalId: string, request: Record<string, unknown>) => {
    const vue = request.vue as { products?: Array<Record<string, unknown>> } | undefined;
    const wantsServer = Boolean(vue?.products?.some((product) => "runtimeServer" in product));
    if (wantsServer) {
      return {
        canonicalId,
        diagnostics: { diagnostics: [], hasErrors: false },
        products: [
          {
            kind: "runtimeServer",
            nodes: options?.serverNodes ?? [
              { node: { kind: "script" }, code: "function ssrRender() {}", meta: {} },
            ],
          },
        ],
      };
    }
    return {
      canonicalId,
      diagnostics: { diagnostics: [], hasErrors: false },
      products: [
        { kind: "runtimeClient", nodes: clientNodes },
        {
          kind: "ideCompanion",
          code: options?.ide?.code ?? "tsx",
          sourceMap: options?.ide?.sourceMap ?? "tsx-map",
        },
      ],
    };
  });

  class MockHost {
    upsert = vi.fn((_request: { inputId: string }) => ({
      diagnostics: { diagnostics: [], hasErrors: false },
      moduleReferences: [],
      parseDurationMs: 0,
    }));
    compileRequest = compileRequest;
    getAnalysis = vi.fn(() => null);
    getPublicApi = vi.fn(() => ({ value: null, error: null }));
    lint = vi.fn(() => []);
    getDocumentStructure = vi.fn(() => null);
  }

  return { MockHost, compileRequest };
}

function seedStaleCompiled(file: File): void {
  file.compiled.js = "stale-js";
  file.compiled.css = "stale-css";
  file.compiled.templateCode = "stale-template";
  file.compiled.verterSourceMap = "stale-map";
  file.compiled.ssrCode = "stale-ssr";
  file.compiled.types = "stale-types";
  file.compiled.typesSourceMap = "stale-types-map";
  file.compiled.tscCode = "stale-dts";
  file.compiled.publicApiOutcome = { kind: "value", value: { code: "stale-api", sourceMap: null } };
  file.compiled.declCode = "stale-decl";
  file.compiled.declarationOutcome = {
    kind: "value",
    value: { code: "stale-decl", sourceMap: "stale-decl-map" },
  };
  file.compiled.declSourceMap = "stale-decl-map";
  file.compiled.analysis = {
    imports: [],
    bindings: [],
    macros: [],
    macroTypeDeps: [],
    scriptFlags: 1,
    styles: [],
    template: null,
  };
  file.compiled.lintDiagnostics = [
    {
      rule: "stale",
      category: "error",
      severity: "error",
      message: "stale-lint",
      spanStart: 0,
      spanEnd: 1,
    },
  ];
}

function expectCompiledSurfacesCleared(file: File): void {
  expect(file.compiled.js).toBe("");
  expect(file.compiled.css).toBe("");
  expect(file.compiled.templateCode).toBe("");
  expect(file.compiled.verterSourceMap).toBe("");
  expect(file.compiled.ssrCode).toBe("");
  expect(file.compiled.types).toBe("");
  expect(file.compiled.typesSourceMap).toBe("");
  expect(file.compiled.tscCode).toBe("");
  expect(file.compiled.publicApiOutcome).toEqual({ kind: "absent" });
  expect(file.compiled.declCode).toBe("");
  expect(file.compiled.declarationOutcome).toEqual({ kind: "absent" });
  expect(file.compiled.declSourceMap).toBe("");
  expect(file.compiled.analysis).toBeNull();
  expect(file.compiled.lintDiagnostics).toEqual([]);
}

describe("compileFile typed host request", () => {
  it("upserts Vue source only and compiles client + IDE in one request r-vue-typed-bytes", async () => {
    const host = createTypedCompileHost({
      clientNodes: [
        { node: { kind: "script" }, code: "export default {}", sourceMap: "script-map" },
        { node: { kind: "template" }, code: "function render() {}", sourceMap: "tpl-map" },
        { node: { kind: "style", index: 0 }, code: ".box { color: red }" },
      ],
      ide: { code: "tsx-output", sourceMap: "tsx-map" },
    });
    const instance = new host.MockHost() as any;
    const teardown = __setHostForTest(instance);
    try {
      const source = '<template><div class="box">hi</div></template>';
      const file = new File("App.vue", source);
      await compileFile(file, { isProduction: false, ssr: false, strictSlots: true });

      expect(instance.upsert).toHaveBeenCalledWith({
        inputId: "App.vue",
        source,
        fileKind: "vue",
        aliases: [],
      });
      expect(instance.compileRequest).toHaveBeenCalledTimes(1);
      expect(instance.getAnalysis).toHaveBeenCalledWith("App.vue");

      const [, request] = host.compileRequest.mock.calls[0];
      expect(
        JSON.stringify(request).includes(JSON.stringify(source)),
        "typed request must not copy source",
      ).toBe(false);
      expect(request).toEqual({
        vue: {
          identity: { isProduction: false, forceJs: true },
          products: [
            { runtimeClient: { runtimeSourceMap: true } },
            {
              ideCompanion: {
                wantSourceMap: true,
                embedAmbientTypes: false,
                conditionalRootNarrowing: false,
                strictSlots: true,
                ideChunkBoundaries: false,
              },
            },
          ],
          options: {
            backend: "inferred",
            ssr: false,
            isCustomElement: [],
            babelParserPlugins: [],
          },
        },
      });
      expect(file.compiled.js).toContain("__sfc__");
      expect(file.compiled.templateCode).toBe("function render() {}");
      expect(file.compiled.css, "vue runtime css").toBe(".box { color: red }");
      expect(file.compiled.types).toBe("tsx-output");
      expect(file.compiled.typesSourceMap).toBe("tsx-map");
    } finally {
      teardown();
    }
  });

  it("compiles SSR as a second server-only request", async () => {
    const host = createTypedCompileHost({
      clientNodes: [{ node: { kind: "script" }, code: "export default {}", sourceMap: "" }],
      serverNodes: [{ node: { kind: "script" }, code: "function ssrRender() {}", sourceMap: "" }],
    });
    const instance = new host.MockHost() as any;
    const teardown = __setHostForTest(instance);
    try {
      const file = new File("App.vue", "<template><div/></template>");
      await compileFile(file, { isProduction: false, ssr: true, strictSlots: false });

      expect(host.compileRequest).toHaveBeenCalledTimes(2);
      const clientRequest = host.compileRequest.mock.calls[0][1] as {
        vue: { products: Array<Record<string, unknown>>; options: { ssr: boolean } };
      };
      const serverRequest = host.compileRequest.mock.calls[1][1] as {
        vue: { products: Array<Record<string, unknown>>; options: { ssr: boolean } };
      };
      expect(clientRequest.vue.options.ssr).toBe(false);
      expect(clientRequest.vue.products.some((product) => "runtimeClient" in product)).toBe(true);
      expect(clientRequest.vue.products.some((product) => "runtimeServer" in product)).toBe(false);
      expect(serverRequest.vue.options.ssr).toBe(true);
      expect(serverRequest.vue.products).toEqual([{ runtimeServer: { runtimeSourceMap: true } }]);
      expect(file.compiled.ssrCode).toContain("ssrRender");
      expect(file.compiled.ssrCode).toContain("__sfc__");
    } finally {
      teardown();
    }
  });

  it("assembles Svelte main, styles, and IDE from the typed response nodes r-svelte-assembly", async () => {
    const host = createTypedCompileHost({
      clientNodes: [
        { node: { kind: "main" }, code: "export default class App {}", sourceMap: "main-map" },
        { node: { kind: "style", index: 0 }, code: ".action { color: red }" },
      ],
      ide: { code: "// svelte tsx", sourceMap: "ide-map" },
    });
    const instance = new host.MockHost() as any;
    const teardown = __setHostForTest(instance);
    try {
      const source = "<h1>hi</h1>";
      const file = new File("App.svelte", source);
      await compileFile(file);

      const [, request] = host.compileRequest.mock.calls[0];
      expect(
        JSON.stringify(request).includes(JSON.stringify(source)),
        "typed request must not copy source",
      ).toBe(false);
      expect(request).toEqual({
        svelte: {
          identity: { isProduction: false, forceJs: true },
          products: [
            { runtimeClient: { runtimeSourceMap: true } },
            {
              ideCompanion: {
                wantSourceMap: true,
                embedAmbientTypes: false,
                conditionalRootNarrowing: false,
                strictSlots: false,
                ideChunkBoundaries: false,
              },
            },
          ],
          options: {},
        },
      });
      expect(file.compiled.js).toBe("export default class App {}");
      expect(file.compiled.js).not.toContain("__sfc__");
      expect(file.compiled.css).toBe(".action { color: red }");
      expect(file.compiled.verterSourceMap).toBe("main-map");
      expect(file.compiled.types).toBe("// svelte tsx");
    } finally {
      teardown();
    }
  });

  it("does not send Vue-only strictSlots on a Svelte request r-strict-slots-svelte", async () => {
    const host = createTypedCompileHost({
      clientNodes: [{ node: { kind: "main" }, code: "export default class App {}" }],
    });
    const instance = new host.MockHost() as any;
    const teardown = __setHostForTest(instance);
    try {
      const file = new File("App.svelte", "<h1>hi</h1>");
      const timing = await compileFile(file, {
        isProduction: false,
        ssr: false,
        strictSlots: true,
      });

      const [, request] = host.compileRequest.mock.calls[0] as [
        string,
        {
          svelte: {
            products: Array<{ ideCompanion?: { strictSlots: boolean } }>;
          };
        },
      ];
      const ide = request.svelte.products.find((product) => product.ideCompanion);
      expect(ide?.ideCompanion?.strictSlots, "svelte ideCompanion.strictSlots").toBe(false);
      expect(file.compiled.errors).toEqual([]);
      expect(file.compiled.js).toBe("export default class App {}");
      expect(timing.tsxMs).not.toBeNull();
    } finally {
      teardown();
    }
  });

  it("keeps style nodes that omit index r-style-unindexed", async () => {
    const host = createTypedCompileHost({
      clientNodes: [
        { node: { kind: "main" }, code: "export default class App {}" },
        { node: { kind: "style" }, code: ".plain { color: blue }" },
      ],
    });
    const instance = new host.MockHost() as any;
    const teardown = __setHostForTest(instance);
    try {
      const file = new File("App.svelte", "<h1>hi</h1>");
      await compileFile(file);
      expect(file.compiled.css, "unindexed style node").toBe(".plain { color: blue }");
    } finally {
      teardown();
    }
  });

  it("records a compileRequest refusal on the file and does not reject r-compile-refusal", async () => {
    const host = createTypedCompileHost();
    host.compileRequest.mockImplementation(() => {
      throw "vue-only axis strictSlots on svelte request";
    });
    const instance = new host.MockHost() as any;
    instance.compileRequest = host.compileRequest;
    const teardown = __setHostForTest(instance);
    try {
      const file = new File("App.svelte", "<h1>hi</h1>");
      seedStaleCompiled(file);
      const timing = await compileFile(file);
      expectCompiledSurfacesCleared(file);
      expect(
        file.compiled.compilerDiagnostics.some(
          (diagnostic) => diagnostic.code === "compile-request-refused",
        ),
        "compile-request-refused",
      ).toBe(true);
      expect(
        file.compiled.errors.some((entry) => entry.includes("strictSlots")),
        "compile-request-refused",
      ).toBe(true);
      expect(timing.tsxMs).toBeNull();
    } finally {
      teardown();
    }
  });

  it("stamps compile-unexpected-error on playground-side throws r-unexpected-compile-error", async () => {
    const host = createTypedCompileHost();
    const instance = new host.MockHost() as any;
    instance.upsert = vi.fn(() => {
      throw new Error("mergeRenderIntoComponent boom");
    });
    const teardown = __setHostForTest(instance);
    try {
      const file = new File("App.svelte", "<h1>hi</h1>");
      seedStaleCompiled(file);
      const timing = await compileFile(file);
      expectCompiledSurfacesCleared(file);
      expect(
        file.compiled.compilerDiagnostics.some(
          (diagnostic) =>
            diagnostic.code === "compile-unexpected-error" &&
            diagnostic.message.includes("mergeRenderIntoComponent boom"),
        ),
        "compile-unexpected-error",
      ).toBe(true);
      expect(
        file.compiled.compilerDiagnostics.some(
          (diagnostic) => diagnostic.code === "compile-request-refused",
        ),
      ).toBe(false);
      expect(timing.tsxMs).toBeNull();
    } finally {
      teardown();
    }
  });

  it("reports a diagnostic when a requested runtime product is absent r-missing-product", async () => {
    const host = createTypedCompileHost({ clientNodes: [] });
    const instance = new host.MockHost() as any;
    const teardown = __setHostForTest(instance);
    try {
      const file = new File("App.svelte", "<h1>hi</h1>");
      await compileFile(file);
      expect(file.compiled.js).toBe("");
      expect(
        file.compiled.compilerDiagnostics.some(
          (diagnostic) => diagnostic.code === "missing-runtime-product",
        ),
        "missing-runtime-product diagnostic",
      ).toBe(true);
      expect(file.compiled.errors.some((entry) => entry.includes("runtimeClient"))).toBe(true);
    } finally {
      teardown();
    }
  });

  it("upserts a .ts file as a Vue script SFC and compiles through one typed request r-ts-arm", async () => {
    const host = createTypedCompileHost({
      clientNodes: [{ node: { kind: "script" }, code: "export const n = 1;", sourceMap: "" }],
    });
    const instance = new host.MockHost() as any;
    const teardown = __setHostForTest(instance);
    try {
      const source = "export const n = 1;";
      const file = new File("counter.ts", source);
      await compileFile(file, { isProduction: false, ssr: false, strictSlots: false });

      expect(instance.compileRequest).toHaveBeenCalledTimes(1);

      const [canonicalId, request] = host.compileRequest.mock.calls[0];
      expect(canonicalId, "typed ts arm").toBe("counter.vue");
      expect(
        JSON.stringify(request).includes(JSON.stringify(source)),
        "typed request must not copy source",
      ).toBe(false);
      expect(instance.upsert).toHaveBeenCalledWith({
        inputId: "counter.vue",
        source: `<script setup lang="ts">\n${source}\n</script>`,
        fileKind: "vue",
        aliases: [],
      });
      expect(request).toEqual({
        vue: {
          identity: { isProduction: false, forceJs: true },
          products: [
            { runtimeClient: { runtimeSourceMap: true } },
            {
              ideCompanion: {
                wantSourceMap: true,
                embedAmbientTypes: false,
                conditionalRootNarrowing: false,
                strictSlots: false,
                ideChunkBoundaries: false,
              },
            },
          ],
          options: {
            backend: "inferred",
            ssr: false,
            isCustomElement: [],
            babelParserPlugins: [],
          },
        },
      });
      expect(file.compiled.js).toBe("export const n = 1;");
    } finally {
      teardown();
    }
  });
});
