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

  class MockHost {
    upsert = upsert;
    listVirtualFiles = vi.fn(() => [{ kind: "main" }]);
    getVirtualFile = vi.fn(() => ({
      code: opts?.virtualFileCode ?? "export default function Component() {}",
      sourceMap: "",
      diagnostics: { diagnostics: [], hasErrors: false },
    }));
    getAnalysis = vi.fn(() => null);
    getIde = vi.fn(() => ({
      code: opts?.ideCode ?? "// svelte tsx\nexport function $props() {}\n",
      sourceMap: '{"version":3,"mappings":""}',
    }));
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
});
