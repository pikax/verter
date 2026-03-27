/**
 * @ai-generated — Tests for macro-type-hydration barrel walk using exportSignatures.
 *
 * These tests verify that the hydration logic uses `exportSignatures` from the
 * host analysis (not heuristic moduleReferences) to discover re-export sources
 * in barrel files.
 */
import { describe, it, expect, afterEach } from "vitest";
import { loadHost, resetHost } from "./compiler";

describe("macro-type-hydration export signature integration", () => {
  afterEach(() => {
    resetHost();
  });

  it("getAnalysis of barrel file includes exportSignatures with reexport metadata", () => {
    const host = loadHost();
    host.upsert({
      inputId: "/lib/index.ts",
      source:
        "export { default as Button } from './Button.vue';\nexport { helper } from './utils';",
      fileKind: "non_sfc",
    });

    const json = host.getAnalysis("/lib/index.ts");
    expect(json).toBeTruthy();
    const analysis = JSON.parse(json!);

    // Positive: exportSignatures present in analysis
    expect(analysis.exportSignatures).toBeDefined();
    expect(Array.isArray(analysis.exportSignatures)).toBe(true);

    const buttonSig = analysis.exportSignatures.find((s: any) => s.name === "Button");
    expect(buttonSig).toBeDefined();
    expect(buttonSig.reexportSource).toBe("./Button.vue");
    expect(buttonSig.reexportLocal).toBe("default");

    const helperSig = analysis.exportSignatures.find((s: any) => s.name === "helper");
    expect(helperSig).toBeDefined();
    expect(helperSig.reexportSource).toBe("./utils");
  });

  it("exportSignatures distinguishes type-only re-exports", () => {
    const host = loadHost();
    host.upsert({
      inputId: "/lib/barrel.ts",
      source:
        "export type { AnimationOptions } from './types';\nexport { animate } from './animate';",
      fileKind: "non_sfc",
    });

    const json = host.getAnalysis("/lib/barrel.ts");
    const analysis = JSON.parse(json!);

    const typeSig = analysis.exportSignatures.find((s: any) => s.name === "AnimationOptions");
    expect(typeSig).toBeDefined();
    expect(typeSig.isType).toBe(true);
    expect(typeSig.reexportSource).toBe("./types");

    const valueSig = analysis.exportSignatures.find((s: any) => s.name === "animate");
    expect(valueSig).toBeDefined();
    // Negative: value re-export is not type-only
    expect(valueSig.isType).toBe(false);
  });

  it("wildcard re-export produces star export signature", () => {
    const host = loadHost();
    host.upsert({
      inputId: "/lib/reexport.ts",
      source: "export * from './types';",
      fileKind: "non_sfc",
    });

    const json = host.getAnalysis("/lib/reexport.ts");
    const analysis = JSON.parse(json!);

    expect(analysis.exportSignatures).toBeDefined();
    // Wildcard re-export should produce a "*" signature
    const starSig = analysis.exportSignatures.find((s: any) => s.name === "*");
    expect(starSig).toBeDefined();
    expect(starSig.reexportSource).toBe("./types");
  });

  it("local exports have no reexport fields in analysis", () => {
    const host = loadHost();
    host.upsert({
      inputId: "/lib/local.ts",
      source: "export const VALUE = 1;\nexport interface Config { key: string }",
      fileKind: "non_sfc",
    });

    const json = host.getAnalysis("/lib/local.ts");
    const analysis = JSON.parse(json!);

    for (const sig of analysis.exportSignatures) {
      // Negative: local exports must not have reexport fields
      expect(sig.reexportSource).toBeUndefined();
      expect(sig.reexportLocal).toBeUndefined();
    }
  });

  it("hydration barrel walk extracts unique reexport sources from exportSignatures", () => {
    // This test verifies the logic that replaced the old moduleReferences heuristic.
    // The new code: (depAnalysis.exportSignatures ?? []).filter(sig => sig.reexportSource).map(...)
    const host = loadHost();
    host.upsert({
      inputId: "/lib/multi-barrel.ts",
      source: [
        "export { default as A } from './CompA.vue';",
        "export { default as B } from './CompB.vue';",
        "export { helper } from './CompA.vue';", // same source as A
        "export const LOCAL = true;",
      ].join("\n"),
      fileKind: "non_sfc",
    });

    const json = host.getAnalysis("/lib/multi-barrel.ts");
    const analysis = JSON.parse(json!);

    const reexportSources = (analysis.exportSignatures ?? [])
      .filter((sig: any) => sig.reexportSource)
      .map((sig: any) => sig.reexportSource);

    // Positive: both sources present
    expect(reexportSources).toContain("./CompA.vue");
    expect(reexportSources).toContain("./CompB.vue");

    // Dedup check (as done in hydration): unique sources
    const unique = [...new Set(reexportSources)];
    expect(unique).toHaveLength(2);

    // Negative: LOCAL is not a re-export
    const localSig = analysis.exportSignatures.find((s: any) => s.name === "LOCAL");
    expect(localSig).toBeDefined();
    expect(localSig.reexportSource).toBeUndefined();
  });
});
