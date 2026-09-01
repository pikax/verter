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
