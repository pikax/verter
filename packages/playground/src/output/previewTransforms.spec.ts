/**
 * @ai-generated - Tests for preview transform functions.
 * These test the conversion of ES module imports/exports to window.__modules__ assignments.
 */
import { describe, it, expect, vi } from "vitest";
import {
  transformImportList,
  transformForPreview,
  collectSvelteRuntimeFlags,
  extractLocalImports,
  orderScriptsByDependency,
} from "./previewTransforms";

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
    expect(transformImportList("ref, computed as comp, watch")).toBe("ref, computed: comp, watch");
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

  describe("Svelte runtime imports", () => {
    it("collects each required runtime flag exactly once", () => {
      const code = [
        `import 'svelte/internal/flags/legacy';`,
        `import "svelte/internal/flags/async";`,
        `import 'svelte/internal/flags/legacy';`,
        `import 'svelte/internal/flags/tracing';`,
      ].join("\n");

      expect(collectSvelteRuntimeFlags(code)).toEqual(["legacy", "async", "tracing"]);
    });

    it("removes runtime-flag imports after the iframe preloads them", () => {
      const code = `import 'svelte/internal/flags/legacy';\nconst value = 1;`;
      expect(transformForPreview(code, mod)).toBe(`\nconst value = 1;`);
    });

    it("binds the official namespace import to the preloaded client runtime", () => {
      const code = `import * as $ from 'svelte/internal/client'`;
      expect(transformForPreview(code, mod)).toBe(`const $ = window.SvelteInternalClient`);
    });

    it("removes the disclose-version import after the iframe preloads it", () => {
      const code = `import 'svelte/internal/disclose-version';\nconst value = 1;`;
      expect(transformForPreview(code, mod)).toBe(`\nconst value = 1;`);
    });

    it("executes an official-shaped client module against the preloaded runtime", () => {
      const fromHtml = vi.fn(() => () => ({ nodeName: "H1" }));
      const windowObject = {
        SvelteInternalClient: { from_html: fromHtml },
        __modules__: { [mod]: {} as { default?: unknown } },
      };
      const compiled = [
        `import 'svelte/internal/disclose-version';`,
        `import * as $ from 'svelte/internal/client';`,
        `var root = $.from_html(\`<h1>hello</h1>\`);`,
        `export default function App($$anchor) { return root(); }`,
      ].join("\n");

      Function("window", transformForPreview(compiled, mod))(windowObject);

      expect(fromHtml).toHaveBeenCalledWith("<h1>hello</h1>");
      expect(typeof windowObject.__modules__[mod].default).toBe("function");
    });
  });

  describe("Local named imports", () => {
    it("transforms named import from .vue file", () => {
      const code = `import { helper } from './Utils.vue'`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(`const { helper } = window.__modules__["./Utils.js"]`);
    });

    it("transforms named import from .ts file", () => {
      const code = `import { helper } from './utils.ts'`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(`const { helper } = window.__modules__["./utils.js"]`);
    });

    it("keeps .js extension unchanged", () => {
      const code = `import { helper } from './utils.js'`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(`const { helper } = window.__modules__["./utils.js"]`);
    });

    it("transforms named import with alias from local file", () => {
      const code = `import { foo as bar } from './Utils.vue'`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(`const { foo: bar } = window.__modules__["./Utils.js"]`);
    });
  });

  describe("Local default imports", () => {
    it("transforms default import from .vue file", () => {
      const code = `import Child from './Child.vue'`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(`const Child = window.__modules__["./Child.js"].default`);
    });

    it("transforms default import from .ts file", () => {
      const code = `import Utils from './utils.ts'`;
      const result = transformForPreview(code, mod);
      expect(result).toBe(`const Utils = window.__modules__["./utils.js"].default`);
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
      expect(result).toBe(`window.__modules__["${mod}"].greet = function greet() { return "hi" }`);
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
      expect(result).toBe(`Object.assign(window.__modules__["${mod}"], { foo: foo, bar: bar })`);
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

describe("extractLocalImports", () => {
  it("extracts default import from .vue file", () => {
    const code = `const Comp = window.__modules__["./Comp.js"].default`;
    expect(extractLocalImports(code)).toEqual(["./Comp.js"]);
  });

  it("extracts named import from .ts file", () => {
    const code = `const { helper } = window.__modules__["./utils.js"]`;
    expect(extractLocalImports(code)).toEqual(["./utils.js"]);
  });

  it("ignores non-local imports (vue, node_modules)", () => {
    const code = `const { ref } = window.Vue\nconst x = 1`;
    expect(extractLocalImports(code)).toEqual([]);
  });

  it("extracts multiple imports", () => {
    const code = [
      `const Comp = window.__modules__["./Comp.js"].default`,
      `const { helper } = window.__modules__["./utils.js"]`,
    ].join("\n");
    expect(extractLocalImports(code)).toEqual(["./Comp.js", "./utils.js"]);
  });

  it("returns empty for no imports", () => {
    const code = `const x = 1;\nconsole.log(x);`;
    expect(extractLocalImports(code)).toEqual([]);
  });

  it("deduplicates repeated imports of the same module", () => {
    const code = [
      `const A = window.__modules__["./Comp.js"].default`,
      `const B = window.__modules__["./Comp.js"].default`,
    ].join("\n");
    expect(extractLocalImports(code)).toEqual(["./Comp.js"]);
  });
});

describe("orderScriptsByDependency", () => {
  it("puts dependency before dependent (single dep)", () => {
    const files: Record<string, string> = {
      "App.vue": `const Comp = window.__modules__["./Comp.js"].default`,
      "Comp.vue": `window.__modules__["./Comp.js"].default = {}`,
    };
    const result = orderScriptsByDependency(files, "App.vue");
    expect(result).toEqual(["Comp.vue", "App.vue"]);
  });

  it("handles diamond dependency: D before B,C before App", () => {
    const files: Record<string, string> = {
      "App.vue": [
        `const B = window.__modules__["./B.js"].default`,
        `const C = window.__modules__["./C.js"].default`,
      ].join("\n"),
      "B.vue": `const D = window.__modules__["./D.js"].default`,
      "C.vue": `const D = window.__modules__["./D.js"].default`,
      "D.vue": `window.__modules__["./D.js"].default = {}`,
    };
    const result = orderScriptsByDependency(files, "App.vue");
    // D must come before B and C; B and C must come before App
    expect(result.indexOf("D.vue")).toBeLessThan(result.indexOf("B.vue"));
    expect(result.indexOf("D.vue")).toBeLessThan(result.indexOf("C.vue"));
    expect(result.indexOf("B.vue")).toBeLessThan(result.indexOf("App.vue"));
    expect(result.indexOf("C.vue")).toBeLessThan(result.indexOf("App.vue"));
    expect(result).toHaveLength(4);
  });

  it("handles circular dependency gracefully", () => {
    const files: Record<string, string> = {
      "App.vue": `const A = window.__modules__["./A.js"].default`,
      "A.vue": `const App = window.__modules__["./App.js"].default`,
    };
    const result = orderScriptsByDependency(files, "App.vue");
    // Should not crash, should contain all files
    expect(result).toHaveLength(2);
    expect(result).toContain("App.vue");
    expect(result).toContain("A.vue");
  });

  it("handles single file", () => {
    const files: Record<string, string> = {
      "App.vue": `window.__modules__["./App.js"].default = {}`,
    };
    const result = orderScriptsByDependency(files, "App.vue");
    expect(result).toEqual(["App.vue"]);
  });

  it("excludes files with empty compiled JS", () => {
    const files: Record<string, string> = {
      "App.vue": `const Comp = window.__modules__["./Comp.js"].default`,
      "Comp.vue": `window.__modules__["./Comp.js"].default = {}`,
      "Empty.vue": "",
    };
    const result = orderScriptsByDependency(files, "App.vue");
    expect(result).toEqual(["Comp.vue", "App.vue"]);
    expect(result).not.toContain("Empty.vue");
  });
});
