/**
 * @ai-generated - Tests for analysis helpers (hover + completion logic).
 * Tests pure functions without Monaco dependency.
 */
import { describe, it, expect } from "vitest";
import {
  hoverForWord,
  formatBindingHover,
  formatImportHover,
  formatMacroHover,
  collectCompletions,
  isOffsetInScriptBlock,
} from "./analysisHelpers";
import type { FileAnalysis } from "../core/types";

function makeAnalysis(overrides: Partial<FileAnalysis> = {}): FileAnalysis {
  return {
    imports: [],
    bindings: [],
    macros: [],
    macroTypeDeps: [],
    scriptFlags: 0,
    styles: [],
    ...overrides,
  };
}

describe("hoverForWord", () => {
  it("returns hover for a known binding", () => {
    const analysis = makeAnalysis({
      bindings: [
        {
          name: "count",
          kind: "Const",
          isReactive: true,
          reactivityKind: "Ref",
          typeAnnotation: null,
          initializer: {
            FunctionCall: { callee: "ref", calleeImportSource: "vue", vueApi: "Ref" },
          },
        },
      ],
    });
    const result = hoverForWord("count", analysis);
    expect(result).not.toBeNull();
    expect(result).toContain("const count");
    expect(result).toContain("ref");
    expect(result).toContain(".value");
  });

  it("returns hover for an import binding", () => {
    const analysis = makeAnalysis({
      imports: [
        {
          source: "vue",
          isTypeOnly: false,
          bindings: [{ name: "ref", isTypeOnly: false, vueApi: "Ref" }],
        },
      ],
    });
    const result = hoverForWord("ref", analysis);
    expect(result).not.toBeNull();
    expect(result).toContain("import");
    expect(result).toContain("'vue'");
    expect(result).toContain("Ref");
  });

  it("returns hover for a macro binding", () => {
    const analysis = makeAnalysis({
      macros: [
        {
          kind: "defineProps",
          isTypeBased: true,
          typeReferences: ["Props"],
          bindingName: "props",
        },
      ],
    });
    const result = hoverForWord("props", analysis);
    expect(result).not.toBeNull();
    expect(result).toContain("defineProps");
    expect(result).toContain("Props");
  });

  it("returns null for unknown word", () => {
    const analysis = makeAnalysis();
    expect(hoverForWord("unknown", analysis)).toBeNull();
  });

  it("prioritizes bindings over imports", () => {
    const analysis = makeAnalysis({
      bindings: [
        {
          name: "ref",
          kind: "Const",
          isReactive: false,
          typeAnnotation: null,
          initializer: null,
        },
      ],
      imports: [
        {
          source: "vue",
          isTypeOnly: false,
          bindings: [{ name: "ref", isTypeOnly: false, vueApi: "Ref" }],
        },
      ],
    });
    const result = hoverForWord("ref", analysis);
    expect(result).not.toBeNull();
    // Should show binding hover (const), not import hover
    expect(result).toContain("const ref");
    expect(result).not.toContain("import");
  });
});

describe("formatBindingHover", () => {
  it("shows type annotation", () => {
    const result = formatBindingHover({
      name: "count",
      kind: "Const",
      isReactive: false,
      typeAnnotation: "number",
      initializer: null,
    });
    expect(result).toContain("const count: number");
  });

  it("shows reactivity for computed", () => {
    const result = formatBindingHover({
      name: "doubled",
      kind: "Const",
      isReactive: true,
      reactivityKind: "Computed",
      typeAnnotation: null,
      initializer: null,
    });
    expect(result).toContain("computed");
    expect(result).toContain("read-only");
  });

  it("shows reactivity for reactive", () => {
    const result = formatBindingHover({
      name: "state",
      kind: "Const",
      isReactive: true,
      reactivityKind: "Reactive",
      typeAnnotation: null,
      initializer: null,
    });
    expect(result).toContain("reactive");
    expect(result).toContain("direct property access");
  });

  it("shows generic reactive for isReactive without specific kind", () => {
    const result = formatBindingHover({
      name: "x",
      kind: "Const",
      isReactive: true,
      typeAnnotation: null,
      initializer: null,
    });
    expect(result).toContain("*(reactive)*");
  });

  it("shows async function kind", () => {
    const result = formatBindingHover({
      name: "fetchData",
      kind: "AsyncFunction",
      isReactive: false,
      typeAnnotation: null,
      initializer: null,
    });
    expect(result).toContain("async function fetchData");
  });

  it("shows literal initializer", () => {
    const result = formatBindingHover({
      name: "x",
      kind: "Const",
      isReactive: false,
      typeAnnotation: null,
      initializer: { Literal: { kind: "string" } },
    });
    expect(result).toContain("Literal: string");
  });

  it("shows reference initializer", () => {
    const result = formatBindingHover({
      name: "y",
      kind: "Const",
      isReactive: false,
      typeAnnotation: null,
      initializer: { Reference: { name: "x" } },
    });
    expect(result).toContain("References `x`");
  });

  it("ignores Other initializer", () => {
    const result = formatBindingHover({
      name: "z",
      kind: "Const",
      isReactive: false,
      typeAnnotation: null,
      initializer: "Other",
    });
    expect(result).not.toContain("Initialized");
    expect(result).not.toContain("Literal");
    expect(result).not.toContain("References");
  });
});

describe("formatImportHover", () => {
  it("shows type-only import", () => {
    const result = formatImportHover(
      { name: "Props", isTypeOnly: true, vueApi: null },
      "./types",
    );
    expect(result).toContain("import type { Props }");
    expect(result).toContain("'./types'");
  });

  it("shows Vue API classification", () => {
    const result = formatImportHover(
      { name: "ref", isTypeOnly: false, vueApi: "Ref" },
      "vue",
    );
    expect(result).toContain("Vue API: `Ref`");
  });
});

describe("formatMacroHover", () => {
  it("shows macro without binding name", () => {
    const result = formatMacroHover({
      kind: "defineExpose",
      isTypeBased: false,
      typeReferences: [],
      bindingName: null,
    });
    expect(result).toContain("defineExpose()");
    expect(result).not.toContain("const");
  });

  it("shows type-based with inline type", () => {
    const result = formatMacroHover({
      kind: "defineEmits",
      isTypeBased: true,
      typeReferences: [],
      bindingName: "emit",
    });
    expect(result).toContain("Type-based: `<inline type>`");
  });
});

describe("collectCompletions", () => {
  it("collects bindings", () => {
    const analysis = makeAnalysis({
      bindings: [
        {
          name: "count",
          kind: "Const",
          isReactive: true,
          reactivityKind: "Ref",
          typeAnnotation: null,
          initializer: null,
        },
        {
          name: "increment",
          kind: "Function",
          isReactive: false,
          typeAnnotation: null,
          initializer: null,
        },
      ],
    });
    const items = collectCompletions(analysis, false);
    expect(items).toHaveLength(2);
    expect(items[0].label).toBe("count");
    expect(items[0].kind).toBe("Constant");
    expect(items[0].detail).toContain("const");
    expect(items[0].detail).toContain("ref");
    expect(items[1].label).toBe("increment");
    expect(items[1].kind).toBe("Function");
  });

  it("includes non-type imports", () => {
    const analysis = makeAnalysis({
      imports: [
        {
          source: "vue",
          isTypeOnly: false,
          bindings: [{ name: "ref", isTypeOnly: false, vueApi: "Ref" }],
        },
      ],
    });
    const items = collectCompletions(analysis, false);
    expect(items.some((i) => i.label === "ref")).toBe(true);
  });

  it("excludes type-only imports in template context", () => {
    const analysis = makeAnalysis({
      imports: [
        {
          source: "./types",
          isTypeOnly: true,
          bindings: [{ name: "Props", isTypeOnly: true, vueApi: null }],
        },
      ],
    });
    const items = collectCompletions(analysis, false);
    expect(items.some((i) => i.label === "Props")).toBe(false);
  });

  it("includes type-only imports in script context", () => {
    const analysis = makeAnalysis({
      imports: [
        {
          source: "./types",
          isTypeOnly: true,
          bindings: [{ name: "Props", isTypeOnly: true, vueApi: null }],
        },
      ],
    });
    const items = collectCompletions(analysis, true);
    expect(items.some((i) => i.label === "Props")).toBe(true);
    const propsItem = items.find((i) => i.label === "Props")!;
    expect(propsItem.kind).toBe("TypeParameter");
  });

  it("includes macro bindings", () => {
    const analysis = makeAnalysis({
      macros: [
        {
          kind: "defineProps",
          isTypeBased: false,
          typeReferences: [],
          bindingName: "props",
        },
      ],
    });
    const items = collectCompletions(analysis, false);
    expect(items.some((i) => i.label === "props")).toBe(true);
  });

  it("filters out ___VERTER___ internal symbols", () => {
    const analysis = makeAnalysis({
      bindings: [
        {
          name: "___VERTER___internal",
          kind: "Const",
          isReactive: false,
          typeAnnotation: null,
          initializer: null,
        },
        {
          name: "count",
          kind: "Const",
          isReactive: false,
          typeAnnotation: null,
          initializer: null,
        },
      ],
    });
    const items = collectCompletions(analysis, false);
    expect(items.some((i) => i.label === "___VERTER___internal")).toBe(false);
    expect(items.some((i) => i.label === "count")).toBe(true);
  });

  it("deduplicates by label", () => {
    const analysis = makeAnalysis({
      bindings: [
        {
          name: "count",
          kind: "Const",
          isReactive: false,
          typeAnnotation: null,
          initializer: null,
        },
      ],
      macros: [
        {
          kind: "defineProps",
          isTypeBased: false,
          typeReferences: [],
          bindingName: "count",
        },
      ],
    });
    const items = collectCompletions(analysis, false);
    const countItems = items.filter((i) => i.label === "count");
    expect(countItems).toHaveLength(1);
  });

  it("skips macros without binding name", () => {
    const analysis = makeAnalysis({
      macros: [
        {
          kind: "defineExpose",
          isTypeBased: false,
          typeReferences: [],
          bindingName: null,
        },
      ],
    });
    const items = collectCompletions(analysis, false);
    expect(items).toHaveLength(0);
  });
});

describe("isOffsetInScriptBlock", () => {
  it("returns true for offset inside script", () => {
    const source = '<script setup lang="ts">\nconst x = 1\n</script>';
    // "const x" starts at offset 25
    expect(isOffsetInScriptBlock(source, 25)).toBe(true);
  });

  it("returns false for offset in template", () => {
    const source = "<template>\n  <div/>\n</template>\n<script setup>\n</script>";
    expect(isOffsetInScriptBlock(source, 15)).toBe(false);
  });

  it("returns false for offset before any block", () => {
    const source = "<!-- comment -->\n<script setup>\n</script>";
    expect(isOffsetInScriptBlock(source, 5)).toBe(false);
  });

  it("handles multiple script blocks", () => {
    const source = "<script>\nexport default {}\n</script>\n<script setup>\nconst x = 1\n</script>";
    // Inside first script (offset 10 is inside "export default")
    expect(isOffsetInScriptBlock(source, 10)).toBe(true);
    // Inside second script (offset 52 is start of "const x = 1")
    expect(isOffsetInScriptBlock(source, 52)).toBe(true);
  });
});
