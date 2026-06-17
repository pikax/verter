/**
 * Discriminating tests for descriptor-driven, framework-agnostic compilation.
 * Uses the `__setHostForTest` mock host so they run in the default gate without
 * loading WASM. These FAIL against a Vue-only `compileFile` (no svelte branch).
 */
import { describe, it, expect, vi } from "vitest";
import { compileFile, __setHostForTest } from "./compiler";
import { File } from "./types";

interface UpsertCall {
  inputId: string;
  fileKind: string;
}

function createMockHost(opts?: { virtualFileCode?: string; ideCode?: string }) {
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
    getPublicApi = vi.fn(() => ({ code: "export {};", sourceMap: "" }));
    lint = vi.fn(() => []);
  }

  return { MockHost, upsert, upsertCalls };
}

describe("compileFile — descriptor-driven framework dispatch", () => {
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
    } finally {
      teardown();
    }
  });
});
