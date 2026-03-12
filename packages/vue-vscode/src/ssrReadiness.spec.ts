import { describe, it, expect } from "vitest";
import { computeSsrReadiness } from "./ssrReadiness";
import type { FileAnalysisSnapshot } from "@verter/language-shared";

function makeAnalysis(overrides: Partial<FileAnalysisSnapshot> = {}): FileAnalysisSnapshot {
  return {
    imports: [],
    bindings: [],
    macros: [],
    macroTypeDeps: [],
    scriptFlags: 0,
    styles: [],
    template: null,
    vueApiCalls: [],
    domQueryCalls: [],
    cssVarManipulations: [],
    ...overrides,
  } as FileAnalysisSnapshot;
}

describe("computeSsrReadiness", () => {
  it("returns 100 for a clean component", () => {
    const result = computeSsrReadiness(makeAnalysis());
    expect(result.score).toBe(100);
    expect(result.issues).toHaveLength(0);
  });

  it("deducts 15 for each client-only lifecycle hook", () => {
    const result = computeSsrReadiness(
      makeAnalysis({
        vueApiCalls: [
          { api: "OnMounted", spanStart: 0, spanEnd: 10 },
          { api: "OnUpdated", spanStart: 20, spanEnd: 30 },
        ],
      }),
    );
    expect(result.score).toBe(70);
    expect(result.issues).toHaveLength(2);
    expect(result.issues[0]!.type).toBe("client-only-lifecycle");
    expect(result.issues[0]!.severity).toBe("error");
  });

  it("deducts 20 for each DOM query", () => {
    const result = computeSsrReadiness(
      makeAnalysis({
        domQueryCalls: [
          {
            kind: "querySelector",
            selectorText: ".foo",
            spanStart: 0,
            spanEnd: 10,
            argSpanStart: 5,
            argSpanEnd: 9,
          },
        ],
      }),
    );
    expect(result.score).toBe(80);
    expect(result.issues).toHaveLength(1);
    expect(result.issues[0]!.type).toBe("dom-query");
  });

  it("deducts 10 for CSS var manipulations", () => {
    const result = computeSsrReadiness(
      makeAnalysis({
        cssVarManipulations: [
          { kind: "setProperty", varName: "--color", spanStart: 0, spanEnd: 10 },
        ],
      }),
    );
    expect(result.score).toBe(90);
    expect(result.issues).toHaveLength(1);
    expect(result.issues[0]!.type).toBe("css-var-manipulation");
    expect(result.issues[0]!.severity).toBe("warning");
  });

  it("deducts 5 for async setup without onServerPrefetch", () => {
    const result = computeSsrReadiness(
      makeAnalysis({
        scriptFlags: 1, // ASYNC_SETUP
      }),
    );
    expect(result.score).toBe(95);
    expect(result.issues).toHaveLength(1);
    expect(result.issues[0]!.type).toBe("missing-server-prefetch");
    expect(result.issues[0]!.severity).toBe("info");
  });

  it("gives +5 bonus for onServerPrefetch", () => {
    const result = computeSsrReadiness(
      makeAnalysis({
        vueApiCalls: [{ api: "OnServerPrefetch", spanStart: 0, spanEnd: 10 }],
      }),
    );
    // 100 + 5 = 105, clamped to 100
    expect(result.score).toBe(100);
    expect(result.issues).toHaveLength(0);
  });

  it("does not deduct for async setup when onServerPrefetch is present", () => {
    const result = computeSsrReadiness(
      makeAnalysis({
        scriptFlags: 1, // ASYNC_SETUP
        vueApiCalls: [{ api: "OnServerPrefetch", spanStart: 0, spanEnd: 10 }],
      }),
    );
    // No -5 for async, +5 for prefetch = 105, clamped to 100
    expect(result.score).toBe(100);
  });

  it("deducts 5 for each useTemplateRef", () => {
    const result = computeSsrReadiness(
      makeAnalysis({
        vueApiCalls: [
          { api: "UseTemplateRef", spanStart: 0, spanEnd: 10 },
          { api: "UseTemplateRef", spanStart: 20, spanEnd: 30 },
        ],
      }),
    );
    expect(result.score).toBe(90);
    expect(result.issues).toHaveLength(2);
    expect(result.issues[0]!.type).toBe("template-ref");
  });

  it("clamps score to 0 when many issues exist", () => {
    const result = computeSsrReadiness(
      makeAnalysis({
        vueApiCalls: [
          { api: "OnMounted", spanStart: 0, spanEnd: 10 },
          { api: "OnUpdated", spanStart: 0, spanEnd: 10 },
          { api: "OnBeforeMount", spanStart: 0, spanEnd: 10 },
          { api: "OnBeforeUnmount", spanStart: 0, spanEnd: 10 },
          { api: "OnActivated", spanStart: 0, spanEnd: 10 },
          { api: "OnDeactivated", spanStart: 0, spanEnd: 10 },
        ],
        domQueryCalls: [
          {
            kind: "querySelector",
            selectorText: ".a",
            spanStart: 0,
            spanEnd: 10,
            argSpanStart: 0,
            argSpanEnd: 10,
          },
        ],
      }),
    );
    // 100 - 6*15 - 20 = -10 → clamped to 0
    expect(result.score).toBe(0);
    expect(result.issues.length).toBeGreaterThan(0);
  });

  it("does not include non-client-only hooks as issues", () => {
    const result = computeSsrReadiness(
      makeAnalysis({
        vueApiCalls: [
          { api: "OnServerPrefetch", spanStart: 0, spanEnd: 10 },
          { api: "OnErrorCaptured", spanStart: 0, spanEnd: 10 },
        ],
      }),
    );
    // OnErrorCaptured and OnServerPrefetch are NOT client-only
    // +5 bonus for server prefetch = 105, clamped to 100
    expect(result.score).toBe(100);
    expect(result.issues).toHaveLength(0);
  });
});
