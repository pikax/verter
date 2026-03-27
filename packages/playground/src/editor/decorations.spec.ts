/**
 * @ai-generated - Unit tests for the decorations module.
 * Tests pure functions that compute Monaco decoration arrays from analysis data.
 */
import { describe, it, expect } from "vitest";
import {
  computeBindingDecorations,
  computeBindingInlayHints,
  computeCssClassDecorations,
  computeCodeLenses,
  getDecorationStyles,
} from "./decorations";
import type { FileAnalysis, AnalysisBinding } from "../core/types";

function makeAnalysis(overrides: Partial<FileAnalysis> = {}): FileAnalysis {
  return {
    imports: [],
    bindings: [],
    macros: [],
    macroTypeDeps: [],
    scriptFlags: 0,
    styles: [],
    template: null,
    ...overrides,
  };
}

describe("computeBindingDecorations", () => {
  it("decorates ref binding at span position", () => {
    const source = '<script setup lang="ts">\nconst count = ref(0)\n</script>';
    const analysis = makeAnalysis({
      bindings: [
        {
          name: "count",
          kind: "Const",
          isReactive: true,
          reactivityKind: "Ref",
          initializer: null,
          spanStart: 31,
          spanEnd: 36,
        },
      ],
    });
    const result = computeBindingDecorations(source, analysis);
    expect(result.length).toBe(1);
    expect(result[0].className).toBe("verter-ref");
    expect(result[0].start).toBe(31);
    expect(result[0].end).toBe(36);
    expect(result[0].hoverMessage).toContain("ref");
  });

  it("decorates computed binding", () => {
    const source = '<script setup lang="ts">\nconst doubled = computed(() => 0)\n</script>';
    const analysis = makeAnalysis({
      bindings: [
        {
          name: "doubled",
          kind: "Const",
          isReactive: true,
          reactivityKind: "Computed",
          initializer: null,
          spanStart: 31,
          spanEnd: 38,
        },
      ],
    });
    const result = computeBindingDecorations(source, analysis);
    expect(result.length).toBe(1);
    expect(result[0].className).toBe("verter-computed");
  });

  it("decorates function binding", () => {
    const source = '<script setup lang="ts">\nfunction increment() {}\n</script>';
    const analysis = makeAnalysis({
      bindings: [
        {
          name: "increment",
          kind: "Function",
          isReactive: false,
          initializer: null,
          spanStart: 34,
          spanEnd: 43,
        },
      ],
    });
    const result = computeBindingDecorations(source, analysis);
    expect(result.length).toBe(1);
    expect(result[0].className).toBe("verter-function");
  });

  it("also decorates template binding occurrences", () => {
    const source =
      '<script setup lang="ts">\nconst count = ref(0)\n</script>\n<template>{{ count }}</template>';
    const analysis = makeAnalysis({
      bindings: [
        {
          name: "count",
          kind: "Const",
          isReactive: true,
          reactivityKind: "Ref",
          initializer: null,
          spanStart: 31,
          spanEnd: 36,
        },
      ],
      template: {
        components: [],
        bindingOccurrences: [
          { name: "count", spanStart: 70, spanEnd: 75, usageKind: "Interpolation" },
        ],
        definedSlots: [],
      },
    });
    const result = computeBindingDecorations(source, analysis);
    // 1 script declaration + 1 template occurrence
    expect(result.length).toBe(2);
    expect(result[0].start).toBe(31); // script
    expect(result[1].start).toBe(70); // template
    expect(result[1].className).toBe("verter-ref");
  });

  it("skips ___VERTER___ prefixed bindings", () => {
    const source = '<script setup lang="ts">\nconst ___VERTER___internal = 1\n</script>';
    const analysis = makeAnalysis({
      bindings: [
        {
          name: "___VERTER___internal",
          kind: "Const",
          isReactive: false,
          initializer: null,
          spanStart: 31,
          spanEnd: 51,
        },
      ],
    });
    const result = computeBindingDecorations(source, analysis);
    expect(result).toEqual([]);
  });

  it("skips bindings with no reactivity or kind style", () => {
    const source = '<script setup lang="ts">\nconst x = 1\n</script>';
    const analysis = makeAnalysis({
      bindings: [
        {
          name: "x",
          kind: "Const",
          isReactive: false,
          initializer: null,
          spanStart: 31,
          spanEnd: 32,
        },
      ],
    });
    const result = computeBindingDecorations(source, analysis);
    expect(result).toEqual([]);
  });
});

describe("computeBindingInlayHints", () => {
  it("produces type hint for ref binding", () => {
    const analysis = makeAnalysis({
      bindings: [
        {
          name: "count",
          kind: "Const",
          isReactive: true,
          reactivityKind: "Ref",
          initializer: {
            FunctionCall: { callee: "ref", calleeImportSource: "vue", vueApi: "Ref" },
          },
          spanStart: 31,
          spanEnd: 36,
        },
      ],
    });
    const hints = computeBindingInlayHints(analysis);
    expect(hints.length).toBe(1);
    expect(hints[0].label).toContain("Ref");
    expect(hints[0].kind).toBe("type");
    expect(hints[0].position).toBe(36); // after binding name
  });

  it("no hint for explicitly typed binding", () => {
    const analysis = makeAnalysis({
      bindings: [
        {
          name: "count",
          kind: "Const",
          isReactive: true,
          reactivityKind: "Ref",
          typeAnnotation: "Ref<number>",
          initializer: {
            FunctionCall: { callee: "ref", calleeImportSource: "vue", vueApi: "Ref" },
          },
          spanStart: 31,
          spanEnd: 36,
        },
      ],
    });
    const hints = computeBindingInlayHints(analysis);
    expect(hints).toEqual([]);
  });

  it("no hint for plain const", () => {
    const analysis = makeAnalysis({
      bindings: [
        {
          name: "x",
          kind: "Const",
          isReactive: false,
          reactivityKind: "None",
          initializer: { Literal: { kind: "Number" } },
          spanStart: 31,
          spanEnd: 32,
        },
      ],
    });
    const hints = computeBindingInlayHints(analysis);
    expect(hints).toEqual([]);
  });

  it("produces ComputedRef hint for computed binding", () => {
    const analysis = makeAnalysis({
      bindings: [
        {
          name: "doubled",
          kind: "Const",
          isReactive: true,
          reactivityKind: "Computed",
          initializer: {
            FunctionCall: { callee: "computed", calleeImportSource: "vue", vueApi: "Computed" },
          },
          spanStart: 31,
          spanEnd: 38,
        },
      ],
    });
    const hints = computeBindingInlayHints(analysis);
    expect(hints.length).toBe(1);
    expect(hints[0].label).toContain("ComputedRef");
  });
});

describe("computeCssClassDecorations", () => {
  it("returns empty array when no styles", () => {
    const analysis = makeAnalysis();
    const result = computeCssClassDecorations(analysis);
    expect(result).toEqual([]);
  });

  it("returns decorations for CSS classes", () => {
    const analysis = makeAnalysis({
      styles: [
        {
          lang: "css",
          scoped: true,
          isModule: false,
          moduleName: null,
          vBinds: [],
          specialPseudos: [],
          flags: 0,
          css: {
            selectors: [],
            classes: [
              { name: "app", start: 100, end: 104 },
              { name: "btn", start: 200, end: 204 },
            ],
            ids: [],
            customProperties: [],
            atRules: [],
            ruleCount: 2,
          },
        },
      ],
    });
    const result = computeCssClassDecorations(analysis);
    expect(result.length).toBe(2);
    expect(result[0].start).toBe(100);
    expect(result[0].end).toBe(104);
    expect(result[0].className).toBe("verter-css-used");
  });
});

describe("computeCodeLenses", () => {
  it("creates lens for script setup block", () => {
    const source =
      '<script setup lang="ts">\nconst x = 1\n</script>\n<template><div></div></template>';
    const analysis = makeAnalysis({
      bindings: [{ name: "x", kind: "Const", isReactive: false, initializer: null }],
      imports: [
        {
          source: "vue",
          isTypeOnly: false,
          bindings: [{ name: "ref", isTypeOnly: false, vueApi: "ref" }],
        },
      ],
    });
    const lenses = computeCodeLenses(source, analysis);
    // Should have at least a script lens and a template lens
    expect(lenses.length).toBeGreaterThanOrEqual(2);
    const scriptLens = lenses.find((l) => l.title.includes("binding"));
    expect(scriptLens).toBeTruthy();
    expect(scriptLens!.title).toContain("1 binding");
    expect(scriptLens!.title).toContain("1 import");
  });

  it("creates lens for style block with scoped info", () => {
    const source =
      '<script setup lang="ts"></script>\n<template><div></div></template>\n<style scoped>\n.app { color: red; }\n</style>';
    const analysis = makeAnalysis({
      styles: [
        {
          lang: "css",
          scoped: true,
          isModule: false,
          moduleName: null,
          vBinds: [],
          specialPseudos: [],
          flags: 0,
          css: {
            selectors: [],
            classes: [{ name: "app", start: 0, end: 4 }],
            ids: [],
            customProperties: [],
            atRules: [],
            ruleCount: 1,
          },
        },
      ],
    });
    const lenses = computeCodeLenses(source, analysis);
    const styleLens = lenses.find((l) => l.title.includes("scoped"));
    expect(styleLens).toBeTruthy();
  });

  it("returns empty array for source with no blocks", () => {
    const source = "// just a comment";
    const analysis = makeAnalysis();
    const lenses = computeCodeLenses(source, analysis);
    expect(lenses).toEqual([]);
  });
});

describe("getDecorationStyles", () => {
  it("returns CSS string with all decoration classes", () => {
    const css = getDecorationStyles();
    expect(css).toContain(".verter-ref");
    expect(css).toContain(".verter-computed");
    expect(css).toContain(".verter-reactive");
    expect(css).toContain(".verter-mutable");
    expect(css).toContain(".verter-function");
    expect(css).toContain(".verter-class");
    expect(css).toContain(".verter-css-used");
    expect(css).toContain(".verter-css-unused");
  });
});
