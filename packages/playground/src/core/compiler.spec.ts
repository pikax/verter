/**
 * @ai-generated - Tests for compiler pure functions.
 */
import { describe, it, expect } from "vitest";
import { mergeRenderIntoComponent, formatDiagnostics } from "./compiler";

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

  // @ai-generated - Regression: template-only SFC (no script block) must define __sfc__
  // The host produces only a render function + imports for template-only components.
  // mergeRenderIntoComponent must create const __sfc__ = {} when no component object exists.
  it("creates __sfc__ for template-only SFC (no script, only render function)", () => {
    const code = `import { createElementVNode as _createElementVNode, openBlock as _openBlock } from "vue"
function render(_ctx, _cache, $props, $setup, $data, $options) {
return (_openBlock(), _createElementVNode("div", null, "hello"))
}`;

    const result = mergeRenderIntoComponent(code);

    // Must define __sfc__ before referencing it
    expect(result).toContain("const __sfc__ = {}");
    expect(result).toContain("__sfc__.render = render;");
    expect(result).toContain("export default __sfc__");

    // __sfc__ definition must come before its first usage
    const defIdx = result.indexOf("const __sfc__ = {}");
    const renderIdx = result.indexOf("__sfc__.render = render;");
    const exportIdx = result.indexOf("export default __sfc__");
    expect(defIdx).toBeLessThan(renderIdx);
    expect(defIdx).toBeLessThan(exportIdx);
  });

  // @ai-generated - Regression: render-only code (no component object, no export default)
  // produces valid output with __sfc__ defined before usage
  it("produces valid output for bare render function without any component object", () => {
    const code = `function render(_ctx, _cache) {
return "hello"
}`;

    const result = mergeRenderIntoComponent(code);

    expect(result).toContain("const __sfc__ = {}");
    expect(result).toContain("__sfc__.render = render;");
    expect(result).toContain("export default __sfc__");
  });

  // @ai-generated - Template-only component with scoped styles: the host returns
  // a synthetic script (with __sfc__ + __scopeId + export) concatenated with
  // the template (import + render function). mergeRenderIntoComponent must
  // insert render attachment before the export default, preserving __scopeId.
  it("handles template-only SFC with scoped styles (synthetic script + template)", () => {
    // This simulates the exact concatenation the playground does:
    // assembledJs = script.code + "\n" + template.code
    const scriptCode = `const __sfc__ = {};
__sfc__.__scopeId = "data-v-0d04bfeb";
export default __sfc__;
`;
    const templateCode = `import { createElementVNode as _createElementVNode, openBlock as _openBlock } from "vue"
function render(_ctx, _cache, $props, $setup, $data, $options) {
return (_openBlock(), _createElementVNode("div", { class: "dashboard" }, "hello"))
}`;
    const code = scriptCode + "\n" + templateCode;

    const result = mergeRenderIntoComponent(code);

    // __scopeId must be preserved
    expect(result).toContain('__sfc__.__scopeId = "data-v-0d04bfeb"');
    // render must be attached
    expect(result).toContain("__sfc__.render = render;");
    // export must exist
    expect(result).toContain("export default __sfc__");
    // Only one const __sfc__ definition
    const sfcMatches = result.match(/const __sfc__/g);
    expect(sfcMatches).toHaveLength(1);

    // Order: __scopeId before render attachment before export
    const scopeIdx = result.indexOf("__scopeId");
    const renderIdx = result.indexOf("__sfc__.render = render;");
    const exportIdx = result.indexOf("export default __sfc__");
    expect(scopeIdx).toBeLessThan(renderIdx);
    expect(renderIdx).toBeLessThan(exportIdx);
  });
});
