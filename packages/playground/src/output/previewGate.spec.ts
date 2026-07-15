/**
 * Discriminating tests for the runtime-preview gate. Output.vue renders the
 * live <Preview> (which selects the framework-owned mount path) ONLY when
 * `supportsRuntimePreview(store.effectiveLanguage)` is true; otherwise it
 * renders <PreviewUnsupported> and never reaches a mount adapter.
 *
 * @vitest-environment happy-dom
 */
import { describe, it, expect, vi } from "vitest";

vi.mock("../core/compiler", () => ({
  initCompilers: vi.fn().mockResolvedValue(undefined),
  compileFile: vi.fn().mockResolvedValue({
    verterNewJs: null,
    parseDurationMs: null,
    scriptMs: null,
    templateMs: null,
    styleMs: null,
    tsxMs: null,
    tscMs: null,
    lintMs: null,
  }),
  relintFile: vi.fn().mockReturnValue(0),
  switchWasmVersion: vi.fn().mockResolvedValue(undefined),
}));

import { useStore, type Store } from "../core/store";
import { supportsRuntimePreview, PREVIEW_RUNTIME_FRAMEWORK_IDS } from "../core/frameworks";

describe("runtime-preview gate", () => {
  it("both shipped browser runtimes support live preview", () => {
    expect(supportsRuntimePreview("vue")).toBe(true);
    expect(supportsRuntimePreview("svelte")).toBe(true);
    expect(supportsRuntimePreview(null)).toBe(false);
  });

  it("the preview-runtime registry includes the Svelte browser mount adapter", () => {
    expect(PREVIEW_RUNTIME_FRAMEWORK_IDS).toEqual(["vue", "svelte"]);
  });

  it("a svelte project gates on the Svelte live preview", async () => {
    const store: Store = useStore();
    await store.selectFramework("svelte");
    expect(store.effectiveLanguage).toBe("svelte");
    // Output.vue uses exactly this predicate to choose <Preview> vs <PreviewUnsupported>.
    expect(supportsRuntimePreview(store.effectiveLanguage)).toBe(true);
  });

  it("a vue project gates ON the live preview", () => {
    const store: Store = useStore();
    expect(store.effectiveLanguage).toBe("vue");
    expect(supportsRuntimePreview(store.effectiveLanguage)).toBe(true);
  });
});
