/**
 * @ai-generated - Tests for compiler pure functions.
 */
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import ts from "typescript";
import { mergeRenderIntoComponent, formatDiagnostics, applyTsxOutput } from "./compiler";
import { File } from "./types";

async function generateRealTsxOutput(vueSource: string): Promise<{ code: string; sourceMap: string }> {
  const thisDir = dirname(fileURLToPath(import.meta.url));
  const wasmJs = resolve(thisDir, "../../../wasm/wasm/verter_wasm.js");
  const wasmBin = resolve(thisDir, "../../../wasm/wasm/verter_wasm_bg.wasm");

  const wasmModule = (await import(pathToFileURL(wasmJs).href)) as any;
  const wasmBytes = readFileSync(wasmBin);
  await wasmModule.default({ module_or_path: wasmBytes });

  const host = new wasmModule.VerterHost({
    devMode: true,
    compileErrorPolicy: "devServeLastKnownGood",
    maxProfilesPerFile: 8,
  });

  const profile = { sourceMap: true, enableTypes: true, forceJs: true };
  host.upsert({
    inputId: "App.vue",
    source: vueSource,
    fileKind: "vue",
    aliases: [],
    compileProfile: profile,
  });

  host.getVirtualFile({
    rawId: "App.vue",
    compileProfile: profile,
  });

  const tsx = host.getTsx("App.vue", profile);
  if (!tsx?.code || !tsx?.sourceMap) {
    throw new Error("expected host.getTsx() to return code + sourceMap");
  }

  return { code: tsx.code, sourceMap: tsx.sourceMap };
}

const VUE_TYPE_STUB = `
declare module "vue" {
  export interface IntrinsicElementAttributes {
    div: {}
    button: {
      onClick?: (...args: unknown[]) => unknown
      onMouseenter?: (...args: unknown[]) => unknown
      "onTest-camel-case"?: (...args: unknown[]) => unknown
    }
  }
}
`;

const JSX_GLOBAL_STUB = `
import type { IntrinsicElementAttributes } from "vue"
declare global {
  namespace JSX {
    interface IntrinsicElements extends IntrinsicElementAttributes {}
  }
}
export {}
`;

function createTypecheckService(tsxCode: string) {
  const fileName = "/App.vue.tsx";
  const files = new Map<string, { version: number; content: string }>([
    [fileName, { version: 1, content: tsxCode }],
    ["/node_modules/vue/index.d.ts", { version: 1, content: VUE_TYPE_STUB }],
    ["/types/jsx-global.d.ts", { version: 1, content: JSX_GLOBAL_STUB }],
  ]);

  const options: ts.CompilerOptions = {
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    jsx: ts.JsxEmit.Preserve,
    strict: true,
    noEmit: true,
    skipLibCheck: true,
    lib: ["lib.es2022.d.ts", "lib.dom.d.ts"],
    types: [],
  };

  const host: ts.LanguageServiceHost = {
    getCompilationSettings: () => options,
    getScriptFileNames: () => [...files.keys()],
    getScriptVersion: (name) => String(files.get(name)?.version ?? 0),
    getScriptSnapshot: (name) => {
      const file = files.get(name);
      if (file) return ts.ScriptSnapshot.fromString(file.content);
      const content = ts.sys.readFile(name);
      if (content != null) return ts.ScriptSnapshot.fromString(content);
      return undefined;
    },
    getCurrentDirectory: () => "/",
    getDefaultLibFileName: (opts) => ts.getDefaultLibFilePath(opts),
    fileExists: (name) => files.has(name) || ts.sys.fileExists(name),
    readFile: (name) => files.get(name)?.content ?? ts.sys.readFile(name),
    readDirectory: ts.sys.readDirectory,
    directoryExists: ts.sys.directoryExists,
    getDirectories: ts.sys.getDirectories,
  };

  return {
    fileName,
    service: ts.createLanguageService(host),
  };
}

function collectTypeScriptDiagnostics(service: ts.LanguageService, fileName: string) {
  const syntactic = service.getSyntacticDiagnostics(fileName);
  const semantic = service.getSemanticDiagnostics(fileName);
  return [...syntactic, ...semantic].map((diag) =>
    ts.flattenDiagnosticMessageText(diag.messageText, "\n"),
  );
}

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

describe("applyTsxOutput", () => {
  it("stores TSX code and TSX source map separately from template map", () => {
    const file = new File("App.vue", "<template><div/></template>");
    file.compiled.verterSourceMap = '{"version":3,"mappings":"template"}';

    applyTsxOutput(file, {
      code: "tsx-code",
      sourceMap: '{"version":3,"mappings":"tsx"}',
    });

    expect(file.compiled.types).toBe("tsx-code");
    expect(file.compiled.typesSourceMap).toBe('{"version":3,"mappings":"tsx"}');
    expect(file.compiled.verterSourceMap).toBe('{"version":3,"mappings":"template"}');
  });

  it("clears TSX fields when output is unavailable", () => {
    const file = new File("App.vue", "<template><div/></template>");
    file.compiled.types = "old";
    file.compiled.typesSourceMap = '{"version":3,"mappings":"old"}';

    applyTsxOutput(file, null);

    expect(file.compiled.types).toBe("");
    expect(file.compiled.typesSourceMap).toBe("");
  });

  it("stores real host TSX output and source map unchanged", async () => {
    const file = new File("App.vue", "<template><div>{{ msg }}</div></template>");
    const vueCode = `<script setup lang=\"ts\">\\nconst msg: string = 'hello'\\n</script>\\n<template><div>{{ msg }}</div></template>`;
    const tsx = await generateRealTsxOutput(vueCode);

    applyTsxOutput(file, tsx);

    expect(file.compiled.types).toBe(tsx.code);
    expect(file.compiled.typesSourceMap).toBe(tsx.sourceMap);
    expect(file.compiled.typesSourceMap.length).toBeGreaterThan(0);
  });
});

describe("generated TSX TypeScript semantics", () => {
  it("type-checks v-on object syntax after key rewrite", async () => {
    const source = `<script setup lang="ts">
</script>
<template>
  <button v-on="{ click: () => {}, mouseenter: () => {} }" />
</template>`;

    const { code } = await generateRealTsxOutput(source);
    const { service, fileName } = createTypecheckService(code);
    const messages = collectTypeScriptDiagnostics(service, fileName);

    expect(messages).toEqual([]);
    expect(code).toContain("onClick");
    expect(code).toContain("onMouseenter");
  });

  it("provides member completions for template expressions", async () => {
    const source = `<template>
  <div>{{ Math.max(1, 2) }}</div>
</template>`;

    const { code } = await generateRealTsxOutput(source);
    const { service, fileName } = createTypecheckService(code);
    const dotOffset = code.indexOf(".max");
    expect(dotOffset).toBeGreaterThanOrEqual(0);

    const completions = service.getCompletionsAtPosition(fileName, dotOffset + 1, {});
    const completionNames = completions?.entries.map((entry) => entry.name) ?? [];
    expect(completionNames).toContain("max");
  });

  it("reports template type errors from generated TSX", async () => {
    const source = `<template>
  <div>{{ Math.notExistingMethod() }}</div>
</template>`;

    const { code } = await generateRealTsxOutput(source);
    const { service, fileName } = createTypecheckService(code);
    const messages = collectTypeScriptDiagnostics(service, fileName);

    expect(messages.some((message) => message.includes("notExistingMethod"))).toBe(true);
  });
});
