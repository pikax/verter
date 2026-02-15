/**
 * @ai-generated - Tests for generateMainModule with all HMR strategies.
 */
import { describe, it, expect } from "vitest";
import { generateMainModule } from "./main";
import type { ViteCodegenResult } from "@verter/native";

function makeResult(overrides: Partial<ViteCodegenResult> = {}): ViteCodegenResult {
  return {
    script: {
      code: "const __sfc__ = { name: 'App' }",
      imports: [],
      body_start_utf16: 0,
    },
    template: {
      code: "function render() { return null }",
      imports: [],
      body_start_utf16: 0,
    },
    styles: [],
    has_default_export: true,
    duration_ms: 1,
    ...overrides,
  };
}

describe("generateMainModule", () => {
  it("generates output with script + template", () => {
    const result = makeResult();
    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    expect(output).toContain("const _sfc_main = { name: 'App' }");
    expect(output).toContain("function render() { return null }");
    expect(output).toContain("_sfc_main.render = render");
    expect(output).toContain("export default _sfc_main");
  });

  it("generates Vite HMR code in development", () => {
    const result = makeResult();
    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: false,
      hmr: "vite",
    });

    expect(output).toContain("import.meta.hot");
    expect(output).toContain('_sfc_main.__hmrId = "abc12345"');
    expect(output).toContain("__VUE_HMR_RUNTIME__");
    expect(output).toContain("import.meta.hot.accept");
    expect(output).not.toContain("module.hot");
  });

  it("generates webpack HMR code in development", () => {
    const result = makeResult();
    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: false,
      hmr: "webpack",
    });

    expect(output).toContain("module.hot");
    expect(output).toContain('_sfc_main.__hmrId = "abc12345"');
    expect(output).toContain("module.hot.accept");
    expect(output).not.toContain("import.meta.hot");
  });

  it("omits HMR code when strategy is none", () => {
    const result = makeResult();
    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: false,
      hmr: "none",
    });

    expect(output).not.toContain("import.meta.hot");
    expect(output).not.toContain("module.hot");
    expect(output).not.toContain("Hot Module Replacement");
  });

  it("omits HMR code in production regardless of strategy", () => {
    const result = makeResult();
    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "vite",
    });

    expect(output).not.toContain("import.meta.hot");
    expect(output).not.toContain("module.hot");
  });

  it("omits HMR code in SSR mode", () => {
    const result = makeResult();
    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: true,
      isProd: false,
      hmr: "vite",
    });

    expect(output).not.toContain("import.meta.hot");
  });

  it("omits __file metadata in production", () => {
    const result = makeResult();
    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    expect(output).not.toContain("__file");
  });

  it("includes __file metadata in development", () => {
    const result = makeResult();
    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: false,
      hmr: "none",
    });

    expect(output).toContain("__file");
    expect(output).toContain("/path/to/App.vue");
    expect(output).toContain("_export_sfc");
  });

  it("adds __scopeId metadata for scoped styles", () => {
    const result = makeResult({
      styles: [
        {
          code: ".red { color: red }",
          scoped: true,
          is_module: false,
          module_classes: [],
        },
      ],
    });

    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    expect(output).toContain('["__scopeId", "data-v-abc12345"]');
    expect(output).toContain("_export_sfc");
  });

  it("generates style virtual module imports", () => {
    const result = makeResult({
      styles: [
        {
          code: ".red { color: red }",
          scoped: false,
          is_module: false,
          module_classes: [],
          lang: "css",
        },
        {
          code: ".blue { color: blue }",
          scoped: true,
          is_module: false,
          module_classes: [],
          lang: "scss",
        },
      ],
    });

    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    expect(output).toContain('import "/path/to/App.vue?');
    expect(output).toContain("type=style");
    expect(output).toContain("index=0");
    expect(output).toContain("index=1");
    expect(output).toContain("lang.css");
    expect(output).toContain("lang.scss");
  });

  it("generates named component export when metadata exists", () => {
    const result = makeResult({
      styles: [
        {
          code: ".red { color: red }",
          scoped: true,
          is_module: false,
          module_classes: [],
        },
      ],
    });

    const output = generateMainModule(result, {
      filename: "/path/to/MyComponent.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    expect(output).toContain("const MyComponent = /* @__PURE__ */ _export_sfc");
    expect(output).toContain("export default MyComponent");
  });

  it("handles filename with special characters", () => {
    const result = makeResult();
    const output = generateMainModule(result, {
      filename: "/path/to/404-page.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: false,
      hmr: "none",
    });

    expect(output).toContain("const _404_page = /* @__PURE__ */ _export_sfc");
    expect(output).toContain("export default _404_page");
  });

  it("handles result with no script", () => {
    const result = makeResult({ script: undefined, has_default_export: false });
    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    expect(output).toContain("function render()");
    expect(output).not.toContain("_sfc_main");
    expect(output).not.toContain("export default");
  });

  it("handles result with no template", () => {
    const result = makeResult({ template: undefined });
    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    expect(output).toContain("const _sfc_main");
    expect(output).not.toContain("_sfc_main.render");
    expect(output).not.toContain("function render()");
  });

  // ==================== Render function attachment ====================

  // @ai-generated - When the Rust compiler includes the render function in script.code
  // (with template: null), generateMainModule must still attach it to _sfc_main
  it("attaches render function when it is in script code and template is null", () => {
    const result = makeResult({
      script: {
        code: [
          "const __sfc__ = { setup() { return {} } }",
          "function render(_ctx, _cache, $props, $setup) { return null }",
        ].join("\n"),
        imports: [],
        body_start_utf16: 0,
      },
      template: undefined,
    });

    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    expect(output).toContain("_sfc_main.render = render");
  });

  // @ai-generated - When template is provided separately, render attachment still works
  it("attaches render function when template is a separate block", () => {
    const result = makeResult(); // default has both script and template
    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    expect(output).toContain("_sfc_main.render = render");
  });

  // @ai-generated - No render attachment when script has no render function and no template
  it("does not attach render when there is no render function", () => {
    const result = makeResult({
      script: {
        code: "const __sfc__ = { name: 'App' }",
        imports: [],
        body_start_utf16: 0,
      },
      template: undefined,
    });

    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    expect(output).not.toContain("_sfc_main.render = render");
  });

  // ==================== Duplicate export default regression ====================

  // @ai-generated - Reproduces the Preview.vue bug where "export default" in a comment
  // used to cause duplicate exports. With the new __sfc__ approach, `compile_for_vite_impl`
  // strips `export default __sfc__` and `__sfc__.__scopeId`, so script code arriving here
  // only contains the `const __sfc__` definition. Comments with "export default" are safe.
  it("handles 'export default' appearing in a comment without issues", () => {
    const result = makeResult({
      script: {
        code: [
          "const __sfc__ = { setup() {",
          "  // Transform: export default X -> window.__modules__[name].default = X",
          "  const x = 1",
          "} }",
        ].join("\n"),
        imports: [],
        body_start_utf16: 0,
      },
      template: undefined,
      styles: [{ code: ".red{}", scoped: true, is_module: false, module_classes: [] }],
    });

    const output = generateMainModule(result, {
      filename: "/path/to/Preview.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    // __sfc__ is renamed to _sfc_main; comment is left intact
    expect(output).toContain("const _sfc_main = { setup()");
    expect(output).toContain("// Transform: export default X ->");
    expect(output).toContain("export default Preview");
  });

  // @ai-generated - Script code with "export default" inside a string literal
  // With __sfc__ approach, string content is irrelevant — we only match `const __sfc__`
  it("handles 'export default' in a string literal without issues", () => {
    const result = makeResult({
      script: {
        code: 'const msg = "export default something";\nconst __sfc__ = { name: "App" }',
        imports: [],
        body_start_utf16: 0,
      },
      template: undefined,
    });

    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    // __sfc__ renamed, string literal left intact
    expect(output).toContain('const _sfc_main = { name: "App" }');
    expect(output).toContain('"export default something"');
    expect(output).toContain("export default _sfc_main");
  });

  // @ai-generated - Regression: output should always have exactly one export default
  it("produces exactly one export default in the output", () => {
    const result = makeResult();
    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    const exportDefaultCount = (output.match(/export default /g) || []).length;
    expect(exportDefaultCount).toBe(1);
  });

  // ==================== CSS Modules: __cssModules injection ====================

  // @ai-generated - Default CSS module injects __cssModules["$style"]
  it("injects __cssModules for default module style", () => {
    const result = makeResult({
      styles: [
        {
          code: ".btn_hash_0 { color: red }",
          scoped: false,
          is_module: true,
          module_classes: [["btn", "btn_hash_0"]],
        },
      ],
    });

    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    expect(output).toContain("__cssModules");
    expect(output).toContain('"$style"');
    expect(output).toContain('"btn"');
    expect(output).toContain('"btn_hash_0"');
  });

  // @ai-generated - Named CSS module uses custom name
  it("injects __cssModules with custom module name", () => {
    const result = makeResult({
      styles: [
        {
          code: ".card_hash_0 { display: flex }",
          scoped: false,
          is_module: true,
          module_classes: [["card", "card_hash_0"]],
          module_name: "classes",
        },
      ],
    });

    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    expect(output).toContain("__cssModules");
    expect(output).toContain('"classes"');
    expect(output).toContain('"card"');
    expect(output).toContain('"card_hash_0"');
  });

  // @ai-generated - No module styles means no __cssModules
  it("does not inject __cssModules without module styles", () => {
    const result = makeResult({
      styles: [
        {
          code: ".btn { color: red }",
          scoped: true,
          is_module: false,
          module_classes: [],
        },
      ],
    });

    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    expect(output).not.toContain("__cssModules");
  });

  // @ai-generated - Multiple module styles in __cssModules
  it("injects multiple modules into __cssModules", () => {
    const result = makeResult({
      styles: [
        {
          code: ".btn_hash_0 { color: red }",
          scoped: false,
          is_module: true,
          module_classes: [["btn", "btn_hash_0"]],
        },
        {
          code: ".card_hash_0 { display: flex }",
          scoped: false,
          is_module: true,
          module_classes: [["card", "card_hash_0"]],
          module_name: "classes",
        },
      ],
    });

    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    expect(output).toContain('"$style"');
    expect(output).toContain('"classes"');
  });

  // @ai-generated - __cssModules uses _export_sfc metadata prop
  it("attaches __cssModules via _export_sfc metadata", () => {
    const result = makeResult({
      styles: [
        {
          code: ".btn_hash_0 { color: red }",
          scoped: false,
          is_module: true,
          module_classes: [["btn", "btn_hash_0"]],
        },
      ],
    });

    const output = generateMainModule(result, {
      filename: "/path/to/App.vue",
      scopeId: "abc12345",
      ssr: false,
      isProd: true,
      hmr: "none",
    });

    expect(output).toContain("_export_sfc");
    expect(output).toContain("__cssModules");
  });
});
