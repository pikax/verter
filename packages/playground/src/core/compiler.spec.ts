/**
 * @ai-generated - Tests for compiler pure functions.
 */
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import ts from "typescript";
import {
  mergeRenderIntoComponent,
  formatDiagnostics,
  applyTsxOutput,
  resolveKnownModuleReferenceDependencies,
  type HostModuleReference,
} from "./compiler";
import { File } from "./types";
import { combineSourceMaps, lookupGenerated, lookupSource, parseMappings } from "./sourcemap";

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

  const profile = { sourceMap: true, target: "ide", forceJs: true };
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

  const tsx = host.getIde("App.vue", profile);
  if (!tsx?.code || !tsx?.sourceMap) {
    throw new Error("expected host.getIde() to return code + sourceMap");
  }

  return { code: tsx.code, sourceMap: tsx.sourceMap };
}

const VUE_TYPE_STUB = `
declare module "vue" {
  export interface IntrinsicElementAttributes {
    div: {
      onClick?: (event: MouseEvent) => unknown
      onMouseenter?: (event: MouseEvent) => unknown
      "onTest-camel-case"?: (...args: unknown[]) => unknown
    }
    button: {
      onClick?: (event: MouseEvent) => unknown
      onMouseenter?: (event: MouseEvent) => unknown
      "onTest-camel-case"?: (...args: unknown[]) => unknown
    }
    input: {
      onInput?: (event: InputEvent) => unknown
      onKeydown?: (event: KeyboardEvent) => unknown
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

// Must match VERTER_TYPES_AMBIENT_MODULE in crates/verter_core/src/ide/script.rs
const VERTER_TYPES_STUB = `
declare module "@verter/types" {
  export type Prettify<T> = T extends { (...args: any[]): any } ? T : { [K in keyof T]: T[K] } & {};
  export declare function enhanceElementWithProps<T, P>(el: T, props: P): T & P;
  export declare function shallowUnwrapRef<T>(obj: T): import("vue").ShallowUnwrapRef<T>;
  export type ExtractRenderComponent<T> = T extends { new (): infer I; } ? I extends { $props: any } ? T : I extends HTMLElement ? (props: {}) => I : I : T extends (...args: any) => infer R ? void extends R ? typeof import("vue").Comment : R extends Array<any> ? typeof import("vue").Fragment : HTMLElement : T extends HTMLElement ? (props: {}) => T : T extends keyof import("vue").NativeElements ? (props: import("vue").NativeElements[T]) => JSX.Element : (props: {}) => JSX.Element;
  export declare function extractRenderComponent<T extends string>(t: T): ExtractRenderComponent<T>;
  export declare function extractRenderComponent<T>(t: T): ExtractRenderComponent<T>;
  export declare function instantiateComponent<T, P>(comp: T, props: P): T extends { new (...args: any[]): infer I } ? I : T extends (...args: any[]) => infer R ? R : T;
  export type ExtractComponentProps<T> = T extends { new (): infer I } ? ExtractComponentProps<I> : T extends { $props: infer P } ? P : T extends HTMLElement ? import("vue").HTMLAttributes : T extends (p: infer P) => any ? P : {};
}
`;

function createTypecheckService(tsxCode: string) {
  const fileName = "/App.vue.tsx";
  const files = new Map<string, { version: number; content: string }>([
    [fileName, { version: 1, content: tsxCode }],
    ["/node_modules/vue/index.d.ts", { version: 1, content: VUE_TYPE_STUB }],
    ["/types/verter-types.d.ts", { version: 1, content: VERTER_TYPES_STUB }],
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

describe("resolveKnownModuleReferenceDependencies", () => {
  it("keeps exact and finite known files while skipping unknown dynamic references", () => {
    const files = {
      "src/App.vue": new File("src/App.vue"),
      "src/exact.ts": new File("src/exact.ts"),
      "src/components/Foo.vue": new File("src/components/Foo.vue"),
      "src/utils/index.ts": new File("src/utils/index.ts"),
    };
    const moduleReferences: HostModuleReference[] = [
      {
        syntax: "staticImport",
        semantics: "import",
        isTypeOnly: false,
        rawText: "'./exact'",
        literalSpecifier: "./exact",
        finiteSpecifiers: [],
        analyzability: "exact",
        spanStart: 0,
        spanEnd: 8,
        exprSpanStart: 0,
        exprSpanEnd: 8,
      },
      {
        syntax: "dynamicImport",
        semantics: "import",
        isTypeOnly: false,
        rawText: "`./${name}`",
        finiteSpecifiers: ["./components/Foo.vue", "./utils", "./exact"],
        analyzability: "finiteSet",
        spanStart: 10,
        spanEnd: 24,
        exprSpanStart: 10,
        exprSpanEnd: 24,
      },
      {
        syntax: "dynamicImport",
        semantics: "import",
        isTypeOnly: false,
        rawText: "`./${name}.vue`",
        finiteSpecifiers: [],
        staticPrefix: "./",
        analyzability: "unknownDynamic",
        spanStart: 26,
        spanEnd: 42,
        exprSpanStart: 26,
        exprSpanEnd: 42,
      },
    ];

    expect(
      resolveKnownModuleReferenceDependencies("src/App.vue", moduleReferences, files),
    ).toEqual(["src/exact.ts", "src/components/Foo.vue", "src/utils/index.ts"]);
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

describe("moduleReferences dependency syncing", () => {
  it("manual host sync resolves exact type dependencies from the in-memory file map", async () => {
    const host = await loadWasmHost();
    const uniqueId = `module-refs-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const app = new File(
      `fixtures/${uniqueId}/App.vue`,
      `<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div>ok</div></template>`,
    );
    const types = new File(
      `fixtures/${uniqueId}/types.ts`,
      `export interface Props {
  name: string
  count: number
}
`,
    );
    const files = {
      [app.filename]: app,
      [types.filename]: types,
    };
    const profile = { sourceMap: true, target: "bundler" };

    const upsert = host.upsert({
      inputId: app.filename,
      source: app.code,
      fileKind: "vue",
      aliases: [],
      compileProfile: profile,
    });
    const deps = resolveKnownModuleReferenceDependencies(
      app.filename,
      upsert.moduleReferences,
      files,
    );

    expect(deps).toEqual([types.filename]);

    host.upsert({
      inputId: types.filename,
      source: types.code,
      fileKind: "non_sfc",
      aliases: [],
      compileProfile: profile,
    });
    host.setImportDependencies(app.filename, deps);

    const main = host.getVirtualFile({
      rawId: app.filename,
      compileProfile: profile,
    });

    expect(main.diagnostics.diagnostics).toEqual([]);
    expect(main.code).toContain("name");
    expect(main.code).toContain("count");
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

  it("type-checks inline $event handlers with the correct event type", async () => {
    const source = `<script setup lang="ts">
</script>
<template>
  <button @click="$event.preventDefault()" />
</template>`;

    const { code } = await generateRealTsxOutput(source);
    const { service, fileName } = createTypecheckService(code);
    const messages = collectTypeScriptDiagnostics(service, fileName);
    const normalized = code.replace(/\s+/g, "");

    expect(messages).toEqual([]);
    expect(normalized).toContain("onClick={($event)=>{$event.preventDefault()}}");
  });

  it("generates $event handler for template-only SFC (no script block)", async () => {
    const source = `<template>
  <button @click="$event.preventDefault()" />
</template>`;

    const { code } = await generateRealTsxOutput(source);
    const normalized = code.replace(/\s+/g, "");

    expect(normalized).toContain("onClick={($event)=>{$event.preventDefault()}}");
  });

  it("emits v-if condition and event handler expressions for guarded branches", async () => {
    const source = `<script setup lang="ts">
const msg: string | number = Math.random() > 0.5 ? 'x' : 0
</script>
<template>
  <button v-if="typeof msg === 'string'" @click="msg.toLowerCase()" />
</template>`;

    const { code } = await generateRealTsxOutput(source);
    expect(code).toContain("typeof msg === 'string'");
    // The v-if guard wraps the handler: onClick={() => {if (!(guard)) { return undefined; } msg.toLowerCase()}}
    expect(code).toContain("msg.toLowerCase()");
    expect(code).toContain("onClick=");
  });

  it("generated TSX has no unused warnings for script setup with refs", async () => {
    const source = `<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
const message = ref('hello')
</script>
<template><div>{{ count }} {{ message }}</div></template>`;
    const { code } = await generateRealTsxOutput(source);
    const { service, fileName } = createTypecheckService(code);
    const messages = collectTypeScriptDiagnostics(service, fileName);
    // Filter to TS6133/6196 (unused variable) errors
    const unusedWarnings = messages.filter(
      (m) => /TS6133|TS6196/.test(m) || /declared but/.test(m),
    );
    expect(unusedWarnings).toEqual([]);
  });

  it("generated TSX has no unused warnings for Comp/Instance/TemplateBindingFN", async () => {
    const source = `<script setup lang="ts">
import { ref, getCurrentInstance } from 'vue'
const el = ref<HTMLDivElement>()
const instance = getCurrentInstance()
</script>
<template><div ref="el">{{ instance?.proxy }}</div></template>`;
    const { code } = await generateRealTsxOutput(source);
    const { service, fileName } = createTypecheckService(code);
    const messages = collectTypeScriptDiagnostics(service, fileName);
    const unusedWarnings = messages.filter((m) => /declared but/.test(m));
    expect(unusedWarnings).toEqual([]);
    // Positive: generated code uses export keyword
    expect(code).toContain("export function ___VERTER___TemplateBindingFN");
  });

  it("generated TSX without ref has no unused warnings", async () => {
    const source = `<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>`;
    const { code } = await generateRealTsxOutput(source);
    // No unused warnings
    const { service, fileName } = createTypecheckService(code);
    const messages = collectTypeScriptDiagnostics(service, fileName);
    const unusedWarnings = messages.filter((m) => /declared but/.test(m));
    expect(unusedWarnings).toEqual([]);
  });
});

// ── Combined source map integration tests (real WASM) ──────────

interface VirtualFile {
  code: string;
  sourceMap?: string;
  diagnostics: { diagnostics: Array<unknown> };
}

interface WasmHostUpsertResult {
  moduleReferences?: HostModuleReference[];
}

interface WasmHost {
  upsert(req: {
    inputId: string;
    source: string;
    fileKind: string;
    aliases: string[];
    compileProfile: Record<string, unknown>;
  }): WasmHostUpsertResult;
  getVirtualFile(query: {
    rawId: string;
    compileProfile?: Record<string, unknown>;
  }): VirtualFile;
  listVirtualFiles(canonicalId: string): Array<{ kind: string; index?: number }>;
  setImportDependencies(canonicalOrAlias: string, resolvedDeps: string[]): void;
}

async function loadWasmHost(): Promise<WasmHost> {
  const thisDir = dirname(fileURLToPath(import.meta.url));
  const wasmJs = resolve(thisDir, "../../../wasm/wasm/verter_wasm.js");
  const wasmBin = resolve(thisDir, "../../../wasm/wasm/verter_wasm_bg.wasm");

  const wasmModule = (await import(pathToFileURL(wasmJs).href)) as any;
  const wasmBytes = readFileSync(wasmBin);
  await wasmModule.default({ module_or_path: wasmBytes });

  return new wasmModule.VerterHost({
    devMode: true,
    compileErrorPolicy: "devServeLastKnownGood",
    maxProfilesPerFile: 8,
  }) as WasmHost;
}

/**
 * Compile a Vue SFC using the real WASM host and return the combined source map
 * alongside the final JS code.
 */
async function compileWithCombinedSourceMap(vueSource: string): Promise<{
  finalJs: string;
  combinedMap: string;
  scriptCode: string;
  templateCode: string;
}> {
  const host = await loadWasmHost();
  const profile = { sourceMap: true, target: "bundler" };

  host.upsert({
    inputId: "App.vue",
    source: vueSource,
    fileKind: "vue",
    aliases: [],
    compileProfile: profile,
  });

  const nodes = host.listVirtualFiles("App.vue");
  const nodeKinds = new Set(nodes.map((n) => n.kind));

  let assembledJs = "";
  let scriptCode = "";
  let scriptSourceMap = "";
  let templateCode = "";
  let templateSourceMap = "";

  if (nodeKinds.has("script")) {
    const script = host.getVirtualFile({
      rawId: "App.vue?vue&type=script",
      compileProfile: profile,
    });
    scriptCode = script.code;
    scriptSourceMap = script.sourceMap ?? "";
    assembledJs += script.code;
  }

  if (nodeKinds.has("template")) {
    const template = host.getVirtualFile({
      rawId: "App.vue?vue&type=template",
      compileProfile: profile,
    });
    if (assembledJs) assembledJs += "\n";
    assembledJs += template.code;
    templateCode = template.code;
    templateSourceMap = template.sourceMap ?? "";
  }

  if (!assembledJs && nodeKinds.has("main")) {
    const main = host.getVirtualFile({
      rawId: "App.vue",
      compileProfile: profile,
    });
    assembledJs = main.code;
  }

  const finalJs = mergeRenderIntoComponent(assembledJs);
  const combinedMap = combineSourceMaps({
    scriptMap: scriptSourceMap,
    scriptCode,
    templateMap: templateSourceMap,
    templateCode,
    vueSource,
    finalJs,
  });

  return { finalJs, combinedMap, scriptCode, templateCode };
}

// @ai-generated - Integration tests for combined source maps with real WASM output
describe("combined source map (WASM integration)", () => {
  it("maps generated lines within bounds of final JS", async () => {
    const source = `<script setup lang="ts">
const msg = "hello"
</script>
<template>
  <div>{{ msg }}</div>
</template>`;

    const { finalJs, combinedMap } = await compileWithCombinedSourceMap(source);
    expect(combinedMap).not.toBe("");

    const parsed = JSON.parse(combinedMap);
    const segments = parseMappings(parsed.mappings);
    const finalLineCount = finalJs.split("\n").length;
    const sourceLineCount = source.split("\n").length;

    // Every mapped generated line must be within bounds
    for (let genLine = 0; genLine < segments.length; genLine++) {
      expect(genLine).toBeLessThan(finalLineCount);
      for (const seg of segments[genLine]) {
        // Source line must be within the original SFC
        expect(seg[2]).toBeLessThan(sourceLineCount);
        expect(seg[2]).toBeGreaterThanOrEqual(0);
      }
    }
  });

  it("round-trips script positions: source→generated→source", async () => {
    const source = `<script setup lang="ts">
const msg = "hello"
const count = 42
</script>
<template>
  <div>{{ msg }}</div>
</template>`;

    const { finalJs, combinedMap } = await compileWithCombinedSourceMap(source);
    expect(combinedMap).not.toBe("");

    // The script content "const msg" is on source line 1
    // Find a mapping that points to source line 1
    const gen = lookupGenerated(combinedMap, 1, 0);
    if (gen) {
      // Verify the generated line contains the expected content
      const genLines = finalJs.split("\n");
      expect(gen.line).toBeLessThan(genLines.length);

      // Round-trip: generated → source should return to the same source line
      const src = lookupSource(combinedMap, gen.line, gen.col);
      expect(src).not.toBeNull();
      expect(src!.line).toBe(1);
    }
  });

  it("round-trips template positions: source→generated→source", async () => {
    const source = `<script setup lang="ts">
const msg = "hello"
</script>
<template>
  <div>{{ msg }}</div>
</template>`;

    const { finalJs, combinedMap } = await compileWithCombinedSourceMap(source);
    expect(combinedMap).not.toBe("");

    // The template content "<div>" is on source line 4
    const gen = lookupGenerated(combinedMap, 4, 2);
    if (gen) {
      const genLines = finalJs.split("\n");
      expect(gen.line).toBeLessThan(genLines.length);

      // Round-trip: generated → source should return to the same source line
      const src = lookupSource(combinedMap, gen.line, gen.col);
      expect(src).not.toBeNull();
      expect(src!.line).toBe(4);
    }
  });

  it("round-trips generated→source→generated for template region", async () => {
    const source = `<script setup lang="ts">
const msg = "hello"
</script>
<template>
  <div>{{ msg }}</div>
</template>`;

    const { finalJs, combinedMap } = await compileWithCombinedSourceMap(source);
    expect(combinedMap).not.toBe("");

    // Find a generated line in the template region (contains "render" or template output)
    const genLines = finalJs.split("\n");
    const renderLine = genLines.findIndex((l) => l.includes("function render"));
    if (renderLine >= 0) {
      const src = lookupSource(combinedMap, renderLine, 0);
      if (src) {
        // Round-trip back to generated
        const gen = lookupGenerated(combinedMap, src.line, src.col);
        expect(gen).not.toBeNull();
        expect(gen!.line).toBe(renderLine);
      }
    }
  });

  it("handles template-only SFC (no script block)", async () => {
    const source = `<template>
  <div>hello world</div>
</template>`;

    const { finalJs, combinedMap } = await compileWithCombinedSourceMap(source);
    // Template-only should still produce a valid source map
    if (combinedMap) {
      const parsed = JSON.parse(combinedMap);
      const segments = parseMappings(parsed.mappings);
      const finalLineCount = finalJs.split("\n").length;

      for (let genLine = 0; genLine < segments.length; genLine++) {
        expect(genLine).toBeLessThan(finalLineCount);
      }
    }
  });

  it("handles script-only SFC (no template block)", async () => {
    const source = `<script setup lang="ts">
const msg = "hello"
defineExpose({ msg })
</script>`;

    const { combinedMap } = await compileWithCombinedSourceMap(source);
    // Script-only — should still have valid map (may be empty if no source maps)
    if (combinedMap) {
      const parsed = JSON.parse(combinedMap);
      expect(parsed.version).toBe(3);
    }
  });

  it("source map code matches displayed code (file.compiled.js)", async () => {
    const source = `<script setup lang="ts">
const msg = "hello"
</script>
<template>
  <div>{{ msg }}</div>
</template>`;

    const { finalJs, combinedMap } = await compileWithCombinedSourceMap(source);
    expect(combinedMap).not.toBe("");

    // The combined map should have the right number of mapping groups for the final JS lines
    const parsed = JSON.parse(combinedMap);
    const mappingLines = parsed.mappings.split(";").length;
    const codeLines = finalJs.split("\n").length;
    expect(mappingLines).toBe(codeLines);
  });
});
