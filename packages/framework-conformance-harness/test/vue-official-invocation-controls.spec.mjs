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
// (@vue/compiler-sfc 3.6.0-rc.5, compiler-sfc.cjs.js): the JS branch emits
// `__vapor: true` whenever `vapor`; the TS branch wraps in
// `defineVaporComponent` when `vapor && !ssr`; `ssr` is read from
// `options.templateOptions?.ssr` and suppresses client-only cssVars script
// injection.

import { describe, expect, it, afterAll } from "vitest";
import ts from "typescript";

import {
  compileVueFixture,
  assembleAndValidate,
  VUE_BUILD_TRANSFORM_ASSET_URLS,
} from "../src/invoke-vue-oracle.mjs";
import { parseModule } from "../src/normalize.mjs";
import { oracleRequire } from "../src/oracle-install.mjs";
import { readGoldenSet } from "../src/golden-store.mjs";
import { GOLDENS_ROOT } from "../src/paths.mjs";
import { executeVueVaporInterop, cleanupScratch } from "../src/execute-vue-vapor.mjs";
import { executeVueSsr, cleanupScratch as cleanupSsrScratch } from "../src/execute-vue-runtime.mjs";
import { hydrateVue, cleanupHydrationScratch } from "../src/hydration.mjs";

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

/** Every module specifier the module imports from, in source order. */
function importedSources(ast) {
  return ast.body.filter((s) => s.type === "ImportDeclaration").map((s) => s.source.value);
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

// bindingMetadata ABSENCE for a script-less SFC. Official bundler tooling
// passes `bindingMetadata: resolvedScript ? resolvedScript.bindings : void 0`
// (@vitejs/plugin-vue@6.0.7, dist/index.mjs:229) — undefined, not an empty
// object, when the SFC has no script block at all. The distinction is
// observable because compiler-core's render-arity branch is TRUTHY-gated
// (pinned dist, @vue/compiler-core/dist/compiler-core.cjs.js:3500:
// `if (options.bindingMetadata && !options.inline) args.push("$props",
// "$setup", "$data", "$options")`), so an empty `{}` — truthy — emits a
// 6-parameter render function official never emits for a script-less SFC.

const SCRIPT_BEARING_FIXTURE = JS_FIXTURE;

/** Parameter names of the named function declaration `name` in a module. */
function functionParams(code, name, label) {
  const ast = parseModule(code, label);
  const declaration = ast.body.find((s) => s.type === "FunctionDeclaration" && s.id?.name === name);
  if (declaration === undefined) throw new Error(`module declares no function ${name}`);
  return declaration.params.map((p) => {
    expect(p.type).toBe("Identifier");
    return p.name;
  });
}

describe("script-less SFC — bindingMetadata is ABSENT at compileTemplate", () => {
  it("the script-less VDOM artifact's render function takes exactly (_ctx, _cache)", () => {
    const code = compileArm(SCRIPTLESS_FIXTURE, "vdom").code;
    expect(functionParams(code, "render", "scriptless-vdom-arity")).toEqual(["_ctx", "_cache"]);
  });

  it("the script-BEARING VDOM artifact keeps the extended 6-parameter signature (regression control)", () => {
    // The non-inline script-bearing path passes REAL bindings, so the same
    // truthy gate must still fire there — a fix that stopped threading
    // bindingMetadata altogether would break this arm.
    const code = compileArm(SCRIPT_BEARING_FIXTURE, "vdom").code;
    expect(functionParams(code, "render", "script-bearing-vdom-arity")).toEqual([
      "_ctx",
      "_cache",
      "$props",
      "$setup",
      "$data",
      "$options",
    ]);
  });

  it("the script-BEARING render half genuinely resolves setup bindings through the metadata", () => {
    // Proves the metadata is not merely PRESENT but semantically consumed:
    // with real bindings the interpolation compiles to `$setup.count`; with
    // an empty/absent map it would fall back to `_ctx.count`.
    const code = compileArm(SCRIPT_BEARING_FIXTURE, "vdom").code;
    expect(code).toContain("$setup.count");
    expect(code).not.toContain("_ctx.count");
  });

  it("the defective invocation — an EMPTY bindingMetadata object — flips the arity back to 6 (discrimination)", () => {
    // Reconstructs exactly what the harness produced when it defaulted
    // `scriptBindings` to `{}`: the identical compileTemplate call with a
    // truthy empty map. Executed on every run, so the positive control above
    // is never a trivially-satisfied assertion.
    const { parse, compileTemplate } = oracleRequire("vue", "@vue/compiler-sfc");
    const filename = "controls-scriptless-defective.vue";
    const { descriptor } = parse(SCRIPTLESS_FIXTURE, { filename, sourceMap: false });
    const templateArgs = (bindingMetadata) => ({
      source: descriptor.template.content,
      filename,
      id: filename,
      scoped: false,
      slotted: descriptor.slotted,
      isProd: false,
      ssr: false,
      vapor: false,
      ssrCssVars: descriptor.cssVars,
      compilerOptions: { mode: "module", bindingMetadata },
    });
    const defective = compileTemplate(templateArgs({}));
    const faithful = compileTemplate(templateArgs(undefined));
    expect(defective.errors ?? []).toEqual([]);
    expect(faithful.errors ?? []).toEqual([]);
    // The ONLY difference between the two arms is the arity of the emitted
    // render function.
    const paramsOf = (result, label) => {
      const ast = parseModule(result.code, label);
      const exported = ast.body.find(
        (s) => s.type === "ExportNamedDeclaration" && s.declaration?.type === "FunctionDeclaration",
      );
      return exported.declaration.params.map((p) => p.name);
    };
    expect(paramsOf(defective, "defective-arity")).toEqual([
      "_ctx",
      "_cache",
      "$props",
      "$setup",
      "$data",
      "$options",
    ]);
    expect(paramsOf(faithful, "faithful-arity")).toEqual(["_ctx", "_cache"]);
    // And the harness's own script-less output matches the FAITHFUL arm.
    expect(
      functionParams(compileArm(SCRIPTLESS_FIXTURE, "vdom").code, "render", "harness"),
    ).toEqual(paramsOf(faithful, "faithful-arity-again"));
  });

  it("the script-less SSR artifact's ssrRender takes exactly (_ctx, _push, _parent, _attrs)", () => {
    // The SSR backend consults the SAME truthy gate: the defective empty map
    // appended $props/$setup/$data/$options here too.
    const code = compileArm(SCRIPTLESS_FIXTURE, "ssr").code;
    expect(functionParams(code, "ssrRender", "scriptless-ssr-arity")).toEqual([
      "_ctx",
      "_push",
      "_parent",
      "_attrs",
    ]);
  });

  it("the VAPOR backend's own fixed signature is UNAFFECTED by the metadata gate (scope control)", () => {
    // Vapor codegen emits its own signature and never consults the
    // render-arity branch — verified directly against the pinned compiler on
    // both arms below. Pinning it here bounds the correction: a change that
    // altered the vapor signature would not be this fix.
    const { parse, compileTemplate } = oracleRequire("vue", "@vue/compiler-sfc");
    const filename = "controls-scriptless-vapor.vue";
    const { descriptor } = parse(SCRIPTLESS_FIXTURE, { filename, sourceMap: false });
    const vaporArm = (bindingMetadata) =>
      compileTemplate({
        source: descriptor.template.content,
        filename,
        id: filename,
        scoped: false,
        slotted: descriptor.slotted,
        isProd: false,
        ssr: false,
        vapor: true,
        ssrCssVars: descriptor.cssVars,
        compilerOptions: { mode: "module", bindingMetadata },
      }).code;
    expect(vaporArm({})).toBe(vaporArm(undefined));
    const params = functionParams(compileArm(SCRIPTLESS_FIXTURE, "vapor").code, "render", "vapor");
    expect(params).toEqual(["_ctx", "$props", "$emit", "$attrs", "$slots"]);
  });

  it("the corrected script-less artifacts still render and hydrate against the pinned runtime", async () => {
    // Behavioral backstop: the corrected arities are not merely a shape
    // change — the 4-param ssrRender must still server-render the fixture's
    // real markup, and the 2-param render must hydrate onto it without a
    // mismatch, through the official pinned runtime.
    const ssr = await executeVueSsr(compileArm(SCRIPTLESS_FIXTURE, "ssr").code);
    expect(ssr.error).toBeNull();
    expect(ssr.ok).toBe(true);
    expect(ssr.html).toBe('<p class="n">static</p>');
    const hydrated = await hydrateVue(ssr.html, compileArm(SCRIPTLESS_FIXTURE, "vdom").code);
    expect(hydrated.error).toBeNull();
    expect(hydrated.ok).toBe(true);
    expect(hydrated.mismatched).toBe(false);
    expect(hydrated.finalHtml).toBe('<p class="n">static</p>');
  });
});

// transformAssetUrls — the official BUILD-mode resolution. Official bundler
// tooling never leaves this option undefined: with no user-supplied option
// and NO dev server — this harness's own posture, an offline non-dev-server
// invocation — plugin-vue resolves `assetUrlOptions = { includeAbsolute:
// true }` (@vitejs/plugin-vue@6.0.7, dist/index.mjs:193; assigned to
// `transformAssetUrls` at :202 and passed to compileTemplate at :223).
//
// Omitting it falls through to the COMPILER's own bare default instead
// (pinned dist, @vue/compiler-sfc/dist/compiler-sfc.cjs.js:3305 selects the
// bare `[transformAssetUrl, transformSrcset]` pair, whose
// `defaultAssetUrlOptions` at :1737-1739 carries `includeAbsolute: false`),
// under which an ABSOLUTE asset URL stays an inert string attribute rather
// than becoming a module import — a materially different import graph from
// the one official build tooling emits for the same SFC.
//
// No committed fixture carries an asset URL today, so this axis is invisible
// in the published set; the controls below therefore drive the harness
// entry point (compileVueFixture) directly, so the divergence is caught at
// the invocation rather than only once an asset-bearing fixture is authored.

const ABSOLUTE_ASSET_FIXTURE = `<template><img src="/logo.png"><img srcset="/a.png 1x, /b.png 2x"></template>
`;

const RELATIVE_ASSET_FIXTURE = `<template><img src="./logo.png"></template>
`;

/** The asset specifiers (never `vue` itself) an artifact imports. */
function assetImports(code, label) {
  return importedSources(parseModule(code, label)).filter((s) => s !== "vue");
}

describe("template asset URLs — official build-mode transformAssetUrls resolution", () => {
  it("ABSOLUTE src/srcset URLs become module imports, as official build tooling emits", () => {
    const code = compileArm(ABSOLUTE_ASSET_FIXTURE, "vdom").code;
    expect(assetImports(code, "absolute-assets")).toEqual(["/logo.png", "/a.png", "/b.png"]);
    // …and the attributes are no longer inert literals: both node transforms
    // (src via transformAssetUrl, srcset via transformSrcset) fired.
    expect(code).not.toContain('src: "/logo.png"');
    expect(code).not.toContain('srcset: "/a.png 1x, /b.png 2x"');
  });

  it("a RELATIVE asset URL is imported under BOTH resolutions (scope control)", () => {
    // Bounds the correction: relative URLs transform regardless of
    // `includeAbsolute` (pinned dist :1788 short-circuits only when the URL
    // is non-relative AND includeAbsolute is false), so a change that
    // altered relative-URL handling would not be this fix.
    expect(assetImports(compileArm(RELATIVE_ASSET_FIXTURE, "vdom").code, "relative-asset")).toEqual(
      ["./logo.png"],
    );
  });

  it("the bare-default invocation leaves ABSOLUTE URLs inert (discrimination)", () => {
    // Reconstructs exactly what the harness produced while it passed no
    // `transformAssetUrls` at all, against the same pinned compiler, and
    // pins the harness's own output to the FAITHFUL arm. Executed on every
    // run, so the positive control above is never trivially satisfied.
    const { parse, compileTemplate } = oracleRequire("vue", "@vue/compiler-sfc");
    const filename = "controls-asset-urls.vue";
    const { descriptor } = parse(ABSOLUTE_ASSET_FIXTURE, { filename, sourceMap: false });
    const templateArgs = (transformAssetUrls) => ({
      source: descriptor.template.content,
      filename,
      id: filename,
      scoped: false,
      slotted: descriptor.slotted,
      isProd: false,
      ssr: false,
      vapor: false,
      ssrCssVars: descriptor.cssVars,
      transformAssetUrls,
      compilerOptions: { mode: "module", bindingMetadata: undefined },
    });
    const bareDefault = compileTemplate(templateArgs(undefined));
    const buildMode = compileTemplate(templateArgs({ includeAbsolute: true }));
    expect(bareDefault.errors ?? []).toEqual([]);
    expect(buildMode.errors ?? []).toEqual([]);

    // The defect is silent in every other respect — both arms compile
    // cleanly — and shows up exactly as a missing import graph.
    expect(assetImports(bareDefault.code, "bare-default-arm")).toEqual([]);
    expect(bareDefault.code).toContain('src: "/logo.png"');
    expect(bareDefault.code).toContain('srcset: "/a.png 1x, /b.png 2x"');
    expect(assetImports(buildMode.code, "build-mode-arm")).toEqual([
      "/logo.png",
      "/a.png",
      "/b.png",
    ]);

    // The harness's own artifact matches the faithful arm, not the default.
    expect(assetImports(compileArm(ABSOLUTE_ASSET_FIXTURE, "vdom").code, "harness-asset")).toEqual(
      assetImports(buildMode.code, "build-mode-arm-again"),
    );
  });

  it("EVERY published Vue golden records the resolution explicitly, not as an inherited default", () => {
    // The choice must be VISIBLE in generated records — a reader of a golden
    // must not have to know the compiler's own default to know which asset
    // resolution produced it (bin/generate-goldens.mjs, Vue `options`).
    expect(VUE_BUILD_TRANSFORM_ASSET_URLS).toEqual({ includeAbsolute: true });
    expect(Object.isFrozen(VUE_BUILD_TRANSFORM_ASSET_URLS)).toBe(true);
    const vueRecords = [...readGoldenSet(GOLDENS_ROOT).values()].filter(
      (record) => record.framework === "vue",
    );
    expect(vueRecords.length).toBeGreaterThan(0);
    for (const record of vueRecords) {
      expect(record.options.transformAssetUrls).toEqual({ includeAbsolute: true });
    }
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

// Runtime interop — the BEHAVIORAL check. A structural comparison of a
// candidate against a defective golden cannot catch a marker both sides
// lost; mounting through the real pinned runtime can: vaporInteropPlugin
// routes a child to the vapor mount path only when the component object
// carries `__vapor: true` (or a defineVaporComponent wrapper), so the
// marked artifact renders the fixture's real DOM and the unmarked one
// mis-renders through the VDOM path with runtime warnings. The mount
// itself is the shared production primitive (src/execute-vue-vapor.mjs) —
// the same one checkCandidate's vapor runtime axis drives.

afterAll(() => {
  cleanupScratch();
  cleanupSsrScratch();
  cleanupHydrationScratch();
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
