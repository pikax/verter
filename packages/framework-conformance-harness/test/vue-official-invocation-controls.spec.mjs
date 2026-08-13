// Self-test: the official compileScript invocation carries the requested
// backend profile — controls at the HARNESS level (compileVueFixture output
// directly, not any compiler-consumer suite).
//
// Every positive control here is discriminating against a harness that
// derives ssr/vapor from the backend but hands them only to
// compileTemplate: under that defect the script half of every artifact is
// compiled as plain non-vapor, non-ssr output, so
//  - the JS vapor artifact loses its `__vapor: true` marker,
//  - the TS vapor artifact loses its `defineVaporComponent` wrapper,
//  - the SSR artifact keeps the client-only `useCssVars` script injection,
//  - and the runtime-interop mount renders WRONG DOM (the behavioral
//    check: a VDOM parent with vaporInteropPlugin routes the child by the
//    `__vapor` marker; an unmarked component with a vapor render function
//    takes the VDOM path and mis-renders with runtime warnings).
// The final describe block BUILDS that defective artifact shape through
// the official compiler and asserts the behavioral check rejects it — the
// discrimination proof is executed on every run, not only in review notes.
//
// All expectations were verified directly against the pinned dist
// (@vue/compiler-sfc 3.6.0-rc.3, compiler-sfc.cjs.js): the JS branch emits
// `__vapor: true` whenever `vapor`; the TS branch wraps in
// `defineVaporComponent` when `vapor && !ssr`; `ssr` is read from
// `options.templateOptions?.ssr` and suppresses client-only cssVars script
// injection.

import { describe, expect, it, afterAll } from "vitest";
import ts from "typescript";

import { compileVueFixture, assembleAndValidate } from "../src/invoke-vue-oracle.mjs";
import { parseModule } from "../src/normalize.mjs";
import { oracleRequire } from "../src/oracle-install.mjs";
import { executeVueVaporInterop, cleanupScratch } from "../src/execute-vue-vapor.mjs";

const JS_FIXTURE = `<script setup>
import { ref } from "vue";
const count = ref(3);
</script>
<template><p class="n">{{ count }}</p></template>
`;

const TS_FIXTURE = `<script setup lang="ts">
import { ref } from "vue";
const count = ref<number>(3);
</script>
<template><p class="n">{{ count }}</p></template>
`;

// v-bind() in style is the script-visible ssr surface: the official
// compiler injects a client-only `useCssVars` call into the script half
// UNLESS templateOptions.ssr is visible at compileScript time.
const CSSVARS_FIXTURE = `<script setup>
import { ref } from "vue";
const color = ref("red");
</script>
<template><div>{{ color }}</div></template>
<style>div { color: v-bind(color); }</style>
`;

function compileArm(source, backend) {
  const artifact = compileVueFixture(source, `controls-${backend}.vue`, {
    backend,
    sourceMap: false,
    isProd: false,
  });
  expect(artifact.diagnostics).toEqual([]);
  expect(artifact.code).not.toBeNull();
  return artifact;
}

/** The `_sfc_main` initializer node of an assembled module. */
function sfcMainInit(ast) {
  for (const stmt of ast.body) {
    if (stmt.type !== "VariableDeclaration") continue;
    for (const decl of stmt.declarations) {
      if (decl.id.type === "Identifier" && decl.id.name === "_sfc_main") return decl.init;
    }
  }
  throw new Error("assembled module declares no _sfc_main binding");
}

/** True when an ObjectExpression carries a literal `__vapor: true` member. */
function hasVaporTrueProperty(objectExpression) {
  expect(objectExpression.type).toBe("ObjectExpression");
  return objectExpression.properties.some(
    (p) =>
      p.type === "Property" &&
      (p.key.name === "__vapor" || p.key.value === "__vapor") &&
      p.value.type === "Literal" &&
      p.value.value === true,
  );
}

/** Local names bound from `import { x as y } from "vue"`, keyed by imported name. */
function vueImportedNames(ast) {
  const names = new Map();
  for (const stmt of ast.body) {
    if (stmt.type !== "ImportDeclaration" || stmt.source.value !== "vue") continue;
    for (const spec of stmt.specifiers) {
      if (spec.type === "ImportSpecifier") {
        names.set(spec.imported.name ?? spec.imported.value, spec.local.name);
      }
    }
  }
  return names;
}

describe("vapor backend — script-half markers (official invocation controls)", () => {
  it("JS <script setup> vapor artifact carries the literal `__vapor: true` member", () => {
    const ast = parseModule(compileArm(JS_FIXTURE, "vapor").code, "js-vapor-control");
    expect(hasVaporTrueProperty(sfcMainInit(ast))).toBe(true);
  });

  it("JS <script setup> VDOM artifact carries NO `__vapor` member (negative control)", () => {
    const ast = parseModule(compileArm(JS_FIXTURE, "vdom").code, "js-vdom-control");
    expect(hasVaporTrueProperty(sfcMainInit(ast))).toBe(false);
  });

  it("TS <script setup lang=ts> vapor artifact wraps in defineVaporComponent imported from vue", () => {
    const ast = parseModule(compileArm(TS_FIXTURE, "vapor").code, "ts-vapor-control");
    const imported = vueImportedNames(ast);
    expect(imported.has("defineVaporComponent")).toBe(true);
    const init = sfcMainInit(ast);
    expect(init.type).toBe("CallExpression");
    expect(init.callee.type).toBe("Identifier");
    expect(init.callee.name).toBe(imported.get("defineVaporComponent"));
  });

  it("TS VDOM artifact wraps in defineComponent, never defineVaporComponent (negative control)", () => {
    const ast = parseModule(compileArm(TS_FIXTURE, "vdom").code, "ts-vdom-control");
    const imported = vueImportedNames(ast);
    expect(imported.has("defineVaporComponent")).toBe(false);
    expect(imported.has("defineComponent")).toBe(true);
    const init = sfcMainInit(ast);
    expect(init.type).toBe("CallExpression");
    expect(init.callee.name).toBe(imported.get("defineComponent"));
  });
});

// A template-only SFC has no compileScript output to carry the marker;
// official bundler assembly (@vitejs/plugin-vue) attaches `__vapor: true`
// to the synthesized component object instead.
const SCRIPTLESS_FIXTURE = `<template><p class="n">static</p></template>
`;

describe("vapor backend — scriptless SFC synthesized-object marker", () => {
  it("a template-only vapor artifact's synthesized component object carries `__vapor: true`", () => {
    const ast = parseModule(compileArm(SCRIPTLESS_FIXTURE, "vapor").code, "scriptless-vapor");
    expect(hasVaporTrueProperty(sfcMainInit(ast))).toBe(true);
  });

  it("the synthesized VAPOR object is byte-for-byte the official bundler-assembly constant; the VDOM object matches up to cosmetic whitespace", () => {
    // Pins the LITERAL emitted strings against their recorded authority
    // (@vitejs/plugin-vue@6.0.7, dist/index.mjs:1424 — see
    // assembleNonInline). plugin-vue is outside the pinned oracle domain,
    // so this literal pin is what catches a change to the constant
    // structurally even without a dynamic plugin-vue pin. The vapor arm
    // is byte-identical to plugin-vue's emission; the non-vapor arm
    // differs only cosmetically (plugin-vue's template emits `{  }` —
    // two interior spaces from an empty interpolation — where the
    // harness emits `{}`), which Compiled-Output Conformance classifies
    // as out-of-contract whitespace, so that arm pins the HARNESS
    // constant, not official bytes.
    expect(compileArm(SCRIPTLESS_FIXTURE, "vapor").code.split("\n")[0]).toBe(
      "const _sfc_main = { __vapor: true }",
    );
    expect(compileArm(SCRIPTLESS_FIXTURE, "vdom").code.split("\n")[0]).toBe("const _sfc_main = {}");
  });

  it("a template-only VDOM artifact's synthesized object stays empty (negative control)", () => {
    const init = sfcMainInit(
      parseModule(compileArm(SCRIPTLESS_FIXTURE, "vdom").code, "scriptless-vdom"),
    );
    expect(init.type).toBe("ObjectExpression");
    expect(init.properties).toEqual([]);
  });

  it("the scriptless vapor artifact mounts correctly through vapor interop", async () => {
    const artifact = compileArm(SCRIPTLESS_FIXTURE, "vapor");
    const result = await mountThroughVaporInterop(artifact.code);
    expect(result.error).toBeNull();
    expect(result.component.__vapor).toBe(true);
    expect(result.html).toBe('<p class="n">static</p>');
    expect(result.warnings).toEqual([]);
  });
});

describe("prod axis — isProd visibility at compileScript", () => {
  // The scoped css-vars KEY is compileScript's own isProd-observable
  // surface (pinned dist genVarName: dev keys are `${id}-${raw}`, prod
  // keys are a `v`-prefixed hash of the same pair). The assertions pin the
  // KEY SHAPE on each arm rather than "dev output differs from prod
  // output": compileTemplate receives isProd independently, so a whole-
  // artifact inequality could stay green while compileScript silently
  // compiles dev-mode on both arms — exactly the omission this control
  // exists to catch.
  function cssVarsArm(isProd) {
    const artifact = compileVueFixture(CSSVARS_FIXTURE, "controls-vdom.vue", {
      backend: "vdom",
      sourceMap: false,
      isProd,
    });
    expect(artifact.diagnostics).toEqual([]);
    return artifact.code;
  }

  it("dev (isProd: false) publishes the readable `${id}-${raw}` css-vars key", () => {
    const dev = cssVarsArm(false);
    expect(dev).toContain('"controls-vdom.vue-color": (color.value)');
    expect(dev).not.toMatch(/"v[0-9a-f]{8}": \(color\.value\)/);
  });

  it("prod (isProd: true) publishes the hashed css-vars key, never the readable one", () => {
    const prod = cssVarsArm(true);
    expect(prod).toMatch(/"v[0-9a-f]{8}": \(color\.value\)/);
    expect(prod).not.toContain('"controls-vdom.vue-color"');
  });
});

describe("ssr backend — templateOptions.ssr visibility at compileScript", () => {
  it("the SSR artifact's script half omits the client-only useCssVars injection", () => {
    const ast = parseModule(compileArm(CSSVARS_FIXTURE, "ssr").code, "ssr-cssvars-control");
    expect(vueImportedNames(ast).has("useCssVars")).toBe(false);
  });

  it("the VDOM artifact of the same fixture DOES inject useCssVars (negative control)", () => {
    const ast = parseModule(compileArm(CSSVARS_FIXTURE, "vdom").code, "vdom-cssvars-control");
    expect(vueImportedNames(ast).has("useCssVars")).toBe(true);
  });

  it("the SSR artifact's RENDER half carries the relocated css-vars attrs merge", () => {
    // Pairs with the script-half absence control above: `useCssVars`
    // missing from the SSR script half is correct ONLY because the SSR
    // backend relocates v-bind() css-vars into the render half's attrs
    // merge — an artifact that silently DROPPED them entirely (a harness
    // passing `ssrCssVars: []` instead of the descriptor's own inventory,
    // as official tooling does) satisfies the absence control too. Shape
    // verified against the pinned dist: the render half declares the
    // `_cssVars` style object keyed by the css-var name and merges it
    // into the rendered attrs.
    const code = compileArm(CSSVARS_FIXTURE, "ssr").code;
    expect(code).toContain('":--controls-ssr.vue-color": ($setup.color)');
    expect(code).toContain("_ssrRenderAttrs(_mergeProps(_attrs, _cssVars))");
  });

  it("the VDOM render half carries NO ssr css-vars merge (negative control)", () => {
    const code = compileArm(CSSVARS_FIXTURE, "vdom").code;
    expect(code).not.toContain("_cssVars");
    expect(code).not.toContain("_ssrRenderAttrs");
  });
});

// ---------------------------------------------------------------------------
// Runtime interop — the BEHAVIORAL check. A structural comparison of a
// candidate against a defective golden cannot catch a marker both sides
// lost; mounting through the real pinned runtime can: vaporInteropPlugin
// routes a child to the vapor mount path only when the component object
// carries `__vapor: true` (or a defineVaporComponent wrapper), so the
// marked artifact renders the fixture's real DOM and the unmarked one
// mis-renders through the VDOM path with runtime warnings. The mount
// itself is the shared production primitive (src/execute-vue-vapor.mjs) —
// the same one checkCandidate's vapor runtime axis drives.
// ---------------------------------------------------------------------------

afterAll(() => {
  cleanupScratch();
});

const mountThroughVaporInterop = executeVueVaporInterop;

const EXPECTED_HTML = '<p class="n">3</p>';

describe("runtime interop — the pinned runtime observes the vapor marker", () => {
  it("the JS vapor artifact mounts through vapor interop to the fixture's real DOM, warning-free", async () => {
    const artifact = compileArm(JS_FIXTURE, "vapor");
    const result = await mountThroughVaporInterop(artifact.code);
    expect(result.error).toBeNull();
    expect(result.component.__vapor).toBe(true);
    expect(result.html).toBe(EXPECTED_HTML);
    expect(result.warnings).toEqual([]);
  });

  it("the TS vapor artifact's defineVaporComponent wrapper is observed as a vapor component", async () => {
    // The official compiler leaves TypeScript syntax in the script half
    // (type stripping belongs to the consuming bundler pipeline), so the
    // artifact is type-erased — nothing else — before execution.
    const artifact = compileArm(TS_FIXTURE, "vapor");
    const erased = ts.transpileModule(artifact.code, {
      compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
    }).outputText;
    const result = await mountThroughVaporInterop(erased);
    expect(result.error).toBeNull();
    expect(result.component.__vapor).toBe(true);
    expect(result.html).toBe(EXPECTED_HTML);
    expect(result.warnings).toEqual([]);
  });

  it("REJECTS the defective artifact shape: vapor render half with an unmarked script half", async () => {
    // Reconstruct exactly what an invocation that omits `vapor` at
    // compileScript produces: an unmarked script half beside a vapor
    // render half, assembled the ordinary way. The behavioral check must
    // refuse this shape — that discrimination is what makes the two
    // positive mounts above meaningful.
    const { parse, compileScript, compileTemplate } = oracleRequire("vue", "@vue/compiler-sfc");
    const filename = "controls-defective.vue";
    const { descriptor } = parse(JS_FIXTURE, { filename, sourceMap: false });
    const unmarkedScript = compileScript(descriptor, {
      id: filename,
      inlineTemplate: false,
      sourceMap: false,
    });
    const vaporRender = compileTemplate({
      source: descriptor.template.content,
      filename,
      id: filename,
      scoped: false,
      slotted: false,
      isProd: false,
      ssr: false,
      vapor: true,
      ssrCssVars: [],
      compilerOptions: { mode: "module", bindingMetadata: unmarkedScript.bindings },
    });
    expect(vaporRender.errors ?? []).toEqual([]);
    const assembly = assembleAndValidate({
      scriptCode: unmarkedScript.content,
      renderCode: vaporRender.code,
      ssr: false,
      vapor: true,
    });
    expect(assembly.fragmentDiagnostics).toEqual([]);
    expect(assembly.code).not.toBeNull();

    const result = await mountThroughVaporInterop(assembly.code);
    // The failure mode is pinned: a mis-ROUTED mount, not a mount throw —
    // an outright error would satisfy the wrong-DOM assertions for the
    // wrong reason.
    expect(result.error).toBeNull();
    expect(result.component.__vapor).toBeUndefined();
    // The unmarked component takes the VDOM path: wrong DOM, runtime warnings.
    expect(result.html).not.toBe(EXPECTED_HTML);
    expect(result.warnings.length).toBeGreaterThan(0);
    // Both observed pinned-runtime signals for this shape: the vapor render
    // helpers running without a vapor instance, and the VDOM render path
    // failing to see the setup bindings.
    expect(result.warnings.join("\n")).toMatch(/Vapor|renderEffect|not defined on instance/);
  });
});
