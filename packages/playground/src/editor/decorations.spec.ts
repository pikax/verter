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
import type {
  FileAnalysis,
  AnalysisBinding,
  OrderedSfcStructure,
  StructureBlock,
} from "../core/types";

/** Structure ranges are UTF-8 BYTE offsets (production wire contract). */
function toBytes(source: string, utf16Offset: number): number {
  return new TextEncoder().encode(source.slice(0, utf16Offset)).length;
}

function structureFor(source: string): OrderedSfcStructure {
  const blocks: StructureBlock[] = [];
  const re = /<(template|script|style)\b[^>]*>/gi;
  let match: RegExpExecArray | null;
  while ((match = re.exec(source))) {
    const name = match[1].toLowerCase();
    const start = match.index + match[0].length;
    const close = source.indexOf(`</${name}>`, start);
    const end = close < 0 ? source.length : close;
    const range = (a: number, b: number) => ({
      sourceSpaceToken: "test",
      start: toBytes(source, a),
      end: toBytes(source, b),
    });
    blocks.push({
      kind: "section",
      markupRootTokens: [],
      section: {
        blockToken: `${name}-${blocks.length}`,
        role:
          name === "template"
            ? { kind: "templateHost" }
            : name === "script"
              ? { kind: "script", role: "instance", dialect: "typescript" }
              : {
                  kind: "style",
                  dialect: "css",
                  scoped: match[0].includes("scoped"),
                  module: "none",
                },
        openingRange: range(match.index, start),
        contentRange: range(start, end),
        fullRange: range(match.index, close < 0 ? source.length : close + name.length + 3),
        attributeInsertionAnchor: range(start - 1, start - 1),
      },
    });
  }
  return { schemaVersion: 1, artifactToken: "test", blocks, markupNodes: [] };
}

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
    const lenses = computeCodeLenses(source, analysis, structureFor(source));
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
          // The sealed token of the third structure block (script, template,
          // style) — the sole association key.
          blockToken: "style-2",
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
    const lenses = computeCodeLenses(source, analysis, structureFor(source));
    const styleLens = lenses.find((l) => l.title.includes("scoped"));
    expect(styleLens).toBeTruthy();
  });

  it("returns empty array for source with no blocks", () => {
    const source = "// just a comment";
    const analysis = makeAnalysis();
    const lenses = computeCodeLenses(source, analysis, structureFor(source));
    expect(lenses).toEqual([]);
  });

  it("token-maps style analyses onto structure blocks, never by ordinal", () => {
    const source =
      "<style>\n.first { color: red; }\n</style>\n<style scoped>\n.second { color: blue; }\n.third { color: green; }\n</style>";
    const structure = structureFor(source); // block tokens: style-0, style-1
    // The analyses arrive REVERSED relative to structure order; only the
    // opaque block token carries the association.
    const styleEntry = (blockToken: string, scoped: boolean, classNames: string[]) => ({
      lang: "css",
      scoped,
      isModule: false,
      moduleName: null,
      blockToken,
      vBinds: [],
      specialPseudos: [],
      flags: 0,
      css: {
        selectors: [],
        classes: classNames.map((name) => ({ name, start: 0, end: 4 })),
        ids: [],
        customProperties: [],
        atRules: [],
        ruleCount: classNames.length,
      },
    });
    const analysis = makeAnalysis({
      styles: [
        styleEntry("style-1", true, ["second", "third"]),
        styleEntry("style-0", false, ["first"]),
      ],
    });

    const lenses = computeCodeLenses(source, analysis, structure);
    const styleLenses = lenses.filter((lens) => /class|rule|scoped/.test(lens.title));
    expect(styleLenses.length).toBe(2);
    // Structure order is preserved; content is joined by token, not index.
    expect(styleLenses[0].title).toContain("1 class");
    expect(styleLenses[0].title).not.toContain("scoped");
    expect(styleLenses[1].title).toContain("2 classes");
    expect(styleLenses[1].title).toContain("scoped");
  });

  it("treats a missing token match as typed unavailable, never ordinal fallback", () => {
    const source = "<style>\n.a { color: red; }\n</style>";
    const analysis = makeAnalysis({
      styles: [
        {
          lang: "css",
          scoped: true,
          isModule: false,
          moduleName: null,
          // A stale artifact's token: no structure block carries it.
          blockToken: "stale-artifact-style-token",
          vBinds: [],
          specialPseudos: [],
          flags: 0,
          css: {
            selectors: [],
            classes: [{ name: "a", start: 0, end: 2 }],
            ids: [],
            customProperties: [],
            atRules: [],
            ruleCount: 1,
          },
        },
      ],
    });
    const lenses = computeCodeLenses(source, analysis, structureFor(source));
    expect(lenses.find((lens) => /class|rule|scoped/.test(lens.title))).toBeUndefined();
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

describe("computeCodeLenses UTF-8/UTF-16 conversion (B-48)", () => {
  it("computes lens lines from byte offsets converted to UTF-16 (astral + CRLF)", () => {
    const source =
      "<script setup>\r\n// \u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\r\nconst a = 1\r\n</script>\r\n<template>\r\n<div/>\r\n</template>";

    const lenses = computeCodeLenses(source, makeAnalysis(), structureFor(source));

    // The template block opens on line 5 (1-based). An unconverted BYTE
    // offset walks past the newline after `<template>` and mis-reports the
    // line.
    expect(lenses.some((lens) => lens.title === "template" && lens.line === 5)).toBe(true);
    expect(lenses.some((lens) => lens.title === "template" && lens.line !== 5)).toBe(false);
  });
});
