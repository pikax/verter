/**
 * @ai-generated - Tests for compiler pure functions.
 */
import { describe, it, expect } from "vitest";
import {
  mergeRenderIntoComponent,
  formatDiagnostics,
  formatImportSpecifier,
  assembleVerterResult,
  type VerterCompileResult,
} from "./compiler";

describe("formatDiagnostics", () => {
  it("returns empty array for undefined input", () => {
    expect(formatDiagnostics(undefined)).toEqual([]);
  });

  it("returns empty array for empty array input", () => {
    expect(formatDiagnostics([])).toEqual([]);
  });

  it("formats severity and message", () => {
    const result = formatDiagnostics([
      { severity: "error", message: "unexpected token" } as any,
    ]);
    expect(result).toEqual(["[error] unexpected token"]);
  });

  it("includes span locations when present", () => {
    const result = formatDiagnostics([
      { severity: "warning", message: "deprecated", spanStart: 10, spanEnd: 20 } as any,
    ]);
    expect(result).toEqual(["[warning] deprecated (10:20)"]);
  });

  it("uses spanStart for both positions when spanEnd is null", () => {
    const result = formatDiagnostics([
      { severity: "error", message: "missing", spanStart: 5, spanEnd: null } as any,
    ]);
    expect(result).toEqual(["[error] missing (5:5)"]);
  });

  it("omits location when spanStart is null", () => {
    const result = formatDiagnostics([
      { severity: "error", message: "generic error", spanStart: null } as any,
    ]);
    expect(result).toEqual(["[error] generic error"]);
  });

  it("handles multiple diagnostics", () => {
    const result = formatDiagnostics([
      { severity: "error", message: "first" } as any,
      { severity: "warning", message: "second", spanStart: 1, spanEnd: 2 } as any,
    ]);
    expect(result).toHaveLength(2);
    expect(result[0]).toBe("[error] first");
    expect(result[1]).toBe("[warning] second (1:2)");
  });
});

describe("formatImportSpecifier", () => {
  it("converts _prefixed names to 'name as _name' format", () => {
    expect(formatImportSpecifier("_createElementVNode")).toBe(
      "createElementVNode as _createElementVNode",
    );
  });

  it("converts single underscore prefix", () => {
    expect(formatImportSpecifier("_h")).toBe("h as _h");
  });

  it("leaves non-prefixed names unchanged", () => {
    expect(formatImportSpecifier("createApp")).toBe("createApp");
  });

  it("leaves single underscore unchanged", () => {
    // A single underscore has length 1, so name.length > 1 is false
    expect(formatImportSpecifier("_")).toBe("_");
  });
});

describe("assembleVerterResult", () => {
  function makeResult(overrides: Partial<VerterCompileResult> = {}): VerterCompileResult {
    return {
      script: null,
      template: null,
      styles: [],
      customBlocks: [],
      scopeId: "",
      errors: [],
      parseDurationMs: 0,
      totalDurationMs: 0,
      ...overrides,
    };
  }

  it("returns empty string for empty result", () => {
    expect(assembleVerterResult(makeResult())).toBe("");
  });

  it("includes script code", () => {
    const result = assembleVerterResult(
      makeResult({
        script: {
          code: "const x = 1;",
          durationMs: 0,
          sourceMap: "",
          setup: true,
          attrs: [],
        },
      }),
    );
    expect(result).toContain("const x = 1;");
  });

  it("includes template code", () => {
    const result = assembleVerterResult(
      makeResult({
        template: {
          code: "function render() {}",
          sourceMap: "",
          imports: [],
          durationMs: 0,
          attrs: [],
        },
      }),
    );
    expect(result).toContain("function render() {}");
  });

  it("generates import statement from template imports", () => {
    const result = assembleVerterResult(
      makeResult({
        template: {
          code: "function render() {}",
          sourceMap: "",
          imports: ["_createElementVNode", "_toDisplayString"],
          durationMs: 0,
          attrs: [],
        },
      }),
    );
    expect(result).toContain("import {");
    expect(result).toContain("createElementVNode as _createElementVNode");
    expect(result).toContain("toDisplayString as _toDisplayString");
    expect(result).toContain('from "vue"');
  });

  it("combines script and template", () => {
    const result = assembleVerterResult(
      makeResult({
        script: {
          code: "const x = 1;",
          durationMs: 0,
          sourceMap: "",
          setup: true,
          attrs: [],
        },
        template: {
          code: "function render() {}",
          sourceMap: "",
          imports: ["_h"],
          durationMs: 0,
          attrs: [],
        },
      }),
    );
    expect(result).toContain("const x = 1;");
    expect(result).toContain("function render() {}");
    expect(result).toContain("h as _h");
  });
});

describe("mergeRenderIntoComponent", () => {
  it("inserts render attachment before existing export default __sfc__ (scoped)", () => {
    const code = `const __sfc__ = /*@__PURE__*/{
__name: 'App',
setup(__props){ return {} }};
function render(_ctx,_cache) { return "hi" }
__sfc__.__scopeId = "data-v-a4f2eed6";
export default __sfc__;
`;
    const result = mergeRenderIntoComponent(code);

    expect(result).toContain("__sfc__.render = render;");
    expect(result).toContain('__sfc__.__scopeId = "data-v-a4f2eed6"');
    expect(result).toContain("export default __sfc__");
    const renderIdx = result.indexOf("__sfc__.render = render;");
    const exportIdx = result.indexOf("export default __sfc__");
    expect(renderIdx).toBeLessThan(exportIdx);
  });

  it("transforms export default to const __sfc__ (non-scoped)", () => {
    const code = `export default /*@__PURE__*/{
__name: 'App',
setup(__props){ return {} }};
function render(_ctx,_cache) { return "hi" }`;

    const result = mergeRenderIntoComponent(code);

    expect(result).toContain("const __sfc__ = ");
    expect(result).toContain("__sfc__.render = render;");
    expect(result).toContain("export default __sfc__");
    expect(result).not.toContain("__scopeId");
  });

  it("does not add render attachment when no render function", () => {
    const code = `export default /*@__PURE__*/{
__name: 'App',
setup(__props){ return {} }};`;

    const result = mergeRenderIntoComponent(code);

    expect(result).toContain("const __sfc__ = ");
    expect(result).toContain("export default __sfc__");
    expect(result).not.toContain("__sfc__.render");
  });

  it("handles empty input", () => {
    const result = mergeRenderIntoComponent("");
    expect(result).toContain("export default __sfc__");
  });

  it("does not double-transform when __sfc__ already exists", () => {
    const code = `const __sfc__ = { name: 'App' };
function render() { return "hi" }
export default __sfc__;`;

    const result = mergeRenderIntoComponent(code);

    const matches = result.match(/const __sfc__/g);
    expect(matches).toHaveLength(1);
    expect(result).toContain("__sfc__.render = render;");
  });

  it("preserves code between component and render function", () => {
    const code = `export default { name: 'App' };
const helper = "foo";
function render() { return "hi" }`;

    const result = mergeRenderIntoComponent(code);
    expect(result).toContain('const helper = "foo"');
    expect(result).toContain("__sfc__.render = render;");
  });

  it("only matches function render at line start", () => {
    const code = `export default { setup() { const fn = function render() {} } };`;
    const result = mergeRenderIntoComponent(code);
    expect(result).not.toContain("__sfc__.render = render;");
  });
});
