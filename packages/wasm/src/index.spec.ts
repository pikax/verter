/**
 * @ai-generated - Tests for the @verter/wasm host wrapper against a mocked binding module.
 *
 * These mocks stand in for the real artifact, so they can only prove the
 * wrapper's own routing. That the artifact actually EXPORTS what the
 * wrapper binds to is a separate claim, proven against the real binary in
 * `artifact-exports.spec.ts`.
 */
import { describe, it, expect, vi, beforeEach } from "vitest";

const mockInit = vi.fn(async () => {});
const mockHostResolve = vi.fn(() => null);
const mockHostUpsert = vi.fn(() => ({
  changed: true,
  moduleReferences: [
    {
      syntax: "dynamicImport",
      semantics: "import",
      isTypeOnly: false,
      rawText: "'./Foo.vue'",
      literalSpecifier: "./Foo.vue",
      finiteSpecifiers: [],
      analyzability: "exact",
      spanStart: 0,
      spanEnd: 10,
      exprSpanStart: 0,
      exprSpanEnd: 10,
    },
  ],
}));
const mockHostGetVirtualFile = vi.fn(() => ({ code: "virtual", diagnostics: { diagnostics: [] } }));
const mockHostListVirtualFiles = vi.fn(() => []);
const mockHostRemove = vi.fn(() => null);
const mockHostCollectResolvableModuleReferenceSpecifiers = vi.fn(() => ["./Foo.vue"]);
const mockHostResolveKnownModuleReferenceDependencies = vi.fn(() => ["src/Foo.vue"]);
// The analysis payload nests under its own key, exactly as the binding
// publishes it — a mock that flattened it would teach the wrong row shape,
// which is how a previous mocked surface outlived the artifact it stood for.
const mockHostCompileRequest = vi.fn(() => ({
  canonicalId: "Comp.vue",
  diagnostics: { diagnostics: [], hasErrors: false },
  products: [{ kind: "analysis", analysis: { bindingOccurrences: [] } }],
}));
const mockHostCtor = vi.fn<(config?: unknown) => void>();
class MockHost {
  constructor(config?: unknown) {
    mockHostCtor(config);
  }

  resolve = mockHostResolve;
  upsert = mockHostUpsert;
  getVirtualFile = mockHostGetVirtualFile;
  listVirtualFiles = mockHostListVirtualFiles;
  remove = mockHostRemove;
  collectResolvableModuleReferenceSpecifiers = mockHostCollectResolvableModuleReferenceSpecifiers;
  resolveKnownModuleReferenceDependencies = mockHostResolveKnownModuleReferenceDependencies;
  compileRequest = mockHostCompileRequest;
}

vi.mock("../wasm/verter_wasm.js", () => ({
  default: mockInit,
  VerterHost: MockHost,
}));

// Import after mock setup so the module picks up mocked dependencies
const { initialize, isInitialized, createHost } = await import("./index.js");

beforeEach(async () => {
  mockInit.mockClear();
  mockHostCtor.mockClear();
  mockHostResolve.mockClear();
  mockHostUpsert.mockClear();
  mockHostGetVirtualFile.mockClear();
  mockHostListVirtualFiles.mockClear();
  mockHostRemove.mockClear();
  mockHostCollectResolvableModuleReferenceSpecifiers.mockClear();
  mockHostResolveKnownModuleReferenceDependencies.mockClear();
  mockHostCompileRequest.mockClear();

  // Ensure module is initialized for each test
  await initialize();
});

describe("@verter/wasm host wrapper", () => {
  describe("isInitialized", () => {
    // @ai-generated - Reports initialization state correctly
    it("should return true after initialization", () => {
      expect(isInitialized()).toBe(true);
    });
  });

  describe("host wrapper", () => {
    it("should create host and forward methods", async () => {
      const host = await createHost({ devMode: true });

      host.resolve("Comp.vue");
      const upsert = host.upsert({ inputId: "Comp.vue", source: "<template/>", fileKind: "vue" });
      host.getVirtualFile({ rawId: "Comp.vue" });
      host.listVirtualFiles("Comp.vue");
      host.remove("Comp.vue");

      expect(upsert.moduleReferences[0].literalSpecifier).toBe("./Foo.vue");
      expect(mockHostCtor).toHaveBeenCalledWith({ devMode: true });
      expect(mockHostResolve).toHaveBeenCalledWith("Comp.vue");
      expect(mockHostUpsert).toHaveBeenCalled();
      expect(mockHostGetVirtualFile).toHaveBeenCalled();
      expect(mockHostListVirtualFiles).toHaveBeenCalledWith("Comp.vue");
      expect(mockHostRemove).toHaveBeenCalledWith("Comp.vue");
    });

    it("forwards the typed compile request as two separate arguments", async () => {
      const host = await createHost();
      const request = {
        vue: {
          identity: { isProduction: false, forceJs: false },
          products: [{ analysis: { wantScriptBindings: true, wantTemplateData: false } }],
          options: { backend: "inferred", ssr: false, isCustomElement: [], babelParserPlugins: [] },
        },
      } as const;

      const response = host.compileRequest("Comp.vue", request);

      // The id and the request stay separate on the way through: a wrapper
      // that folded the id into the request, or reordered the pair, would
      // hand the binding a payload the schema refuses at run time.
      expect(mockHostCompileRequest).toHaveBeenCalledWith("Comp.vue", request);
      expect(response.canonicalId).toBe("Comp.vue");
      expect(response.products).toEqual([
        { kind: "analysis", analysis: { bindingOccurrences: [] } },
      ]);
    });

    it("forwards shared module reference helper methods", async () => {
      const host = await createHost({ devMode: true });
      const moduleReferences = [
        {
          syntax: "dynamicImport",
          semantics: "import",
          isTypeOnly: false,
          rawText: "`./${name}.vue`",
          finiteSpecifiers: ["./Foo.vue"],
          analyzability: "finiteSet",
          spanStart: 0,
          spanEnd: 12,
          exprSpanStart: 0,
          exprSpanEnd: 12,
        },
      ];

      expect((host as any).collectResolvableModuleReferenceSpecifiers(moduleReferences)).toEqual([
        "./Foo.vue",
      ]);
      expect(
        (host as any).resolveKnownModuleReferenceDependencies(
          "src/App.vue",
          moduleReferences,
          ["src/Foo.vue"],
          [".vue"],
        ),
      ).toEqual(["src/Foo.vue"]);

      expect(mockHostCollectResolvableModuleReferenceSpecifiers).toHaveBeenCalledWith(
        moduleReferences,
      );
      expect(mockHostResolveKnownModuleReferenceDependencies).toHaveBeenCalledWith(
        "src/App.vue",
        moduleReferences,
        ["src/Foo.vue"],
        [".vue"],
      );
    });
  });
});
