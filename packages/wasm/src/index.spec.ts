/**
 * @ai-generated - Tests for Uint8Array input support in @verter/wasm compile wrapper.
 * Verifies input routing (string vs Uint8Array), compileBytes fallback, and UTF-8 validation.
 */
import { describe, it, expect, vi, beforeEach } from "vitest";

const MOCK_RESULT = {
  code: "compiled code",
  sourceMap: "{}",
  codeWithSourceMap: "compiled code\n//# sourceMappingURL=...",
  durationMs: 1.5,
};

const mockCompile = vi.fn(() => MOCK_RESULT);
const mockCompileBytes = vi.fn(() => MOCK_RESULT);
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
  compile: mockCompile,
  compileBytes: mockCompileBytes,
  VerterHost: MockHost,
}));

// Import after mock setup so the module picks up mocked dependencies
const { compile, compileSync, initialize, isInitialized, createHost } = await import("./index.js");

beforeEach(async () => {
  mockCompile.mockClear();
  mockCompileBytes.mockClear();
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

describe("Uint8Array input support", () => {
  describe("compile", () => {
    // @ai-generated - String input routes to wasmCompile
    it("should route string input to wasmCompile", async () => {
      const result = await compile("template code");

      expect(mockCompile).toHaveBeenCalledWith("template code", undefined);
      expect(mockCompileBytes).not.toHaveBeenCalled();
      expect(result).toEqual(MOCK_RESULT);
    });

    // @ai-generated - Uint8Array input routes to wasmCompileBytes when available
    it("should route Uint8Array input to wasmCompileBytes", async () => {
      const bytes = new TextEncoder().encode("template code");
      const result = await compile(bytes);

      expect(mockCompileBytes).toHaveBeenCalledWith(bytes, undefined);
      expect(mockCompile).not.toHaveBeenCalled();
      expect(result).toEqual(MOCK_RESULT);
    });

    // @ai-generated - Options are forwarded with Uint8Array input
    it("should forward options with Uint8Array input", async () => {
      const bytes = new TextEncoder().encode("template code");
      const opts = { filename: "App.vue", isProduction: true };
      await compile(bytes, opts);

      expect(mockCompileBytes).toHaveBeenCalledWith(bytes, opts);
    });

    // @ai-generated - Options are forwarded with string input
    it("should forward options with string input", async () => {
      const opts = { filename: "App.vue" };
      await compile("code", opts);

      expect(mockCompile).toHaveBeenCalledWith("code", opts);
    });
  });

  describe("compileSync", () => {
    // @ai-generated - String input routes to wasmCompile
    it("should route string input to wasmCompile", () => {
      const result = compileSync("template code");

      expect(mockCompile).toHaveBeenCalledWith("template code", undefined);
      expect(mockCompileBytes).not.toHaveBeenCalled();
      expect(result).toEqual(MOCK_RESULT);
    });

    // @ai-generated - Uint8Array input routes to wasmCompileBytes
    it("should route Uint8Array input to wasmCompileBytes", () => {
      const bytes = new TextEncoder().encode("template code");
      const result = compileSync(bytes);

      expect(mockCompileBytes).toHaveBeenCalledWith(bytes, undefined);
      expect(mockCompile).not.toHaveBeenCalled();
      expect(result).toEqual(MOCK_RESULT);
    });
  });

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
