/**
 * @ai-generated - Tests for preview transform functions.
 * These test the conversion of ES module imports/exports to window.__modules__ assignments.
 */
import { describe, it, expect } from "vitest";
import { transformImportList, transformForPreview } from "./previewTransforms";

describe("transformImportList", () => {
  it("transforms 'x as y' to 'x: y'", () => {
    expect(transformImportList("x as y")).toBe("x: y");
  });

  it("handles multiple 'as' pairs", () => {
    expect(transformImportList("a as b, c as d")).toBe("a: b, c: d");
  });

  it("leaves non-aliased imports unchanged", () => {
    expect(transformImportList("ref, computed")).toBe("ref, computed");
  });

  it("handles mixed aliased and non-aliased", () => {
    expect(transformImportList("ref, computed as comp, watch")).toBe(
      "ref, computed: comp, watch",
    );
  });
});

describe("transformForPreview", () => {
  const mod = "./App.js";

  describe("Vue imports", () => {
    it("transforms named import from vue", () => {
      const code = `import { ref, computed } from 'vue'`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(`const { ref, computed } = window.Vue`);
    });

    it("transforms named import with alias from vue", () => {
      const code = `import { ref as myRef } from 'vue'`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(`const { ref: myRef } = window.Vue`);
    });

    it("transforms default import from vue", () => {
      const code = `import Vue from 'vue'`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(`const Vue = window.Vue`);
    });

    it("handles double-quoted vue import", () => {
      const code = `import { ref } from "vue"`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(`const { ref } = window.Vue`);
    });
  });

  describe("Local named imports", () => {
    it("transforms named import from .vue file", () => {
      const code = `import { helper } from './Utils.vue'`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(
        `const { helper } = window.__modules__["./Utils.js"]`,
      );
    });

    it("transforms named import from .ts file", () => {
      const code = `import { helper } from './utils.ts'`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(
        `const { helper } = window.__modules__["./utils.js"]`,
      );
    });

    it("keeps .js extension unchanged", () => {
      const code = `import { helper } from './utils.js'`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(
        `const { helper } = window.__modules__["./utils.js"]`,
      );
    });

    it("transforms named import with alias from local file", () => {
      const code = `import { foo as bar } from './Utils.vue'`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(
        `const { foo: bar } = window.__modules__["./Utils.js"]`,
      );
    });
  });

  describe("Local default imports", () => {
    it("transforms default import from .vue file", () => {
      const code = `import Child from './Child.vue'`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(
        `const Child = window.__modules__["./Child.js"].default`,
      );
    });

    it("transforms default import from .ts file", () => {
      const code = `import Utils from './utils.ts'`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(
        `const Utils = window.__modules__["./utils.js"].default`,
      );
    });
  });

  describe("Exports", () => {
    it("transforms export default", () => {
      const code = `export default __sfc__;`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(`window.__modules__["${mod}"].default = __sfc__;`);
    });

    it("transforms export function", () => {
      const code = `export function greet() { return "hi" }`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(
        `window.__modules__["${mod}"].greet = function greet() { return "hi" }`,
      );
    });

    it("transforms export const", () => {
      const code = `export const count = 0`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(`window.__modules__["${mod}"].count = 0`);
    });

    it("transforms export let", () => {
      const code = `export let count = 0`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(`window.__modules__["${mod}"].count = 0`);
    });

    it("transforms export var", () => {
      const code = `export var count = 0`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(`window.__modules__["${mod}"].count = 0`);
    });

    it("transforms export { x, y }", () => {
      const code = `export { foo, bar }`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(
        `Object.assign(window.__modules__["${mod}"], { foo: foo, bar: bar })`,
      );
    });

    it("transforms export { x as y }", () => {
      const code = `export { foo as myFoo, bar as myBar }`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(
        `Object.assign(window.__modules__["${mod}"], { myFoo: foo, myBar: bar })`,
      );
    });
  });

  describe("Non-transformed code", () => {
    it("does NOT transform standalone function render", () => {
      const code = `function render(_ctx, _cache) { return "hi" }`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(code);
    });

    it("preserves non-module code", () => {
      const code = `const x = 1;\nconsole.log(x);`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(code);
    });
  });

  describe("Extension replacement", () => {
    it("replaces .vue with .js in import paths", () => {
      const code = `import Child from './Child.vue'`;
      const result = transformForPreview(code, mod);
      expect(result).toContain("./Child.js");
      expect(result).not.toContain(".vue");
    });

    it("replaces .ts with .js in import paths", () => {
      const code = `import { fn } from './utils.ts'`;
      const result = transformForPreview(code, mod);
      expect(result).toContain("./utils.js");
      expect(result).not.toContain(".ts");
    });

    it("keeps .js imports as-is", () => {
      const code = `import { fn } from './utils.js'`;
      const result = transformForPreview(code, mod);
      expect(result).toContain("./utils.js");
    });
  });

  describe("Complex scenarios", () => {
    it("transforms a full compiled component output", () => {
      const code = [
        `import { ref } from 'vue'`,
        `import Child from './Child.vue'`,
        `const __sfc__ = { setup() { return {} } };`,
        `function render() { return "hi" }`,
        `__sfc__.render = render;`,
        `export default __sfc__;`,
      ].join("\n");

      const result = transformForPreview(code, mod);

      expect(result).toContain("const { ref } = window.Vue");
      expect(result).toContain('window.__modules__["./Child.js"].default');
      expect(result).toContain("function render() {");
      expect(result).toContain(`window.__modules__["${mod}"].default = __sfc__`);
    });
  });
});
