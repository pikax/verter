// Hermetic invocation of the pinned official Vue compiler
// (`@vue/compiler-sfc`, backed by `@vue/compiler-dom` /
// `@vue/compiler-vapor` / `@vue/compiler-ssr`). Never writes an
// expectation; produces a raw compilation artifact for the caller.
//
// Compiler is loaded dynamically from the isolated per-domain install
// (oracle-install.mjs) — never workspace resolution, never a static
// top-level import. The validation gate runs before any oracle compiler
// line is evaluated.

import { parseModule } from "./normalize.mjs";
import { validateVueFragments } from "./fragments.mjs";
import { VUE_DOMAIN } from "./domain-pin.mjs";
import { PackageDriftError } from "./package-pin.mjs";
import { ensureOracleDomain, oracleRequire } from "./oracle-install.mjs";
import { composeAssembledModuleMap } from "./sourcemap.mjs";

/**
 * Contract-observable diagnostic fields: kind, code, message, source
 * identity, start and end spans.
 */
function toDiagnostic(kind, error, filename) {
  return {
    kind,
    code: error?.code ?? null,
    message: String(error?.message ?? error),
    source: filename,
    start: error?.loc?.start
      ? { line: error.loc.start.line ?? null, column: error.loc.start.column ?? null }
      : null,
    end: error?.loc?.end
      ? { line: error.loc.end.line ?? null, column: error.loc.end.column ?? null }
      : null,
  };
}

/** Runs the isolated-install validation gate without loading any compiler. */
export function assertVuePinned() {
  ensureOracleDomain("vue");
}

let compilerSfc = null;

/**
 * Load `@vue/compiler-sfc` from the validated isolated install. The gate
 * (`ensureOracleDomain` inside `oracleRequire`) has passed before the
 * compiler is evaluated. Vue routes Node `import` and `require` to the
 * same CJS dist (`node` export condition); transitive deps resolve from
 * the realized committed closure, not the workspace store.
 */
function vueCompilerSfc() {
  if (compilerSfc === null) {
    const loaded = oracleRequire("vue", "@vue/compiler-sfc");
    // Loaded-module identity: the compiler in use attests its version
    // against the domain pin — a workspace-hoisted load refuses here.
    if (loaded.version !== VUE_DOMAIN.packageVersion) {
      throw new PackageDriftError(
        `loaded @vue/compiler-sfc reports version ${loaded.version}, pinned ${VUE_DOMAIN.packageVersion}`,
        {
          expected: VUE_DOMAIN.packageVersion,
          actual: loaded.version,
          layer: "loaded-module-identity",
        },
      );
    }
    compilerSfc = loaded;
  }
  return compilerSfc;
}

/** Version the loaded oracle compiler attests — for identity self-tests. */
export function vueOracleCompilerVersion() {
  return vueCompilerSfc().version;
}

/**
 * @typedef {"vdom"|"vapor"|"ssr"} VueBackend
 */

/**
 * Asset-URL resolution official bundler tooling passes in build mode —
 * this harness's posture: offline, no user `template.transformAssetUrls`,
 * no dev server. Authority (verified verbatim; not covered by the pinned
 * oracle domain): @vitejs/plugin-vue@6.0.7, dist/index.mjs:193,
 * `else assetUrlOptions = { includeAbsolute: true }`, assigned onto
 * `transformAssetUrls` at :202 and passed to compileTemplate at :223.
 *
 * Passed explicitly rather than letting compileTemplate fall through to
 * the compiler default (`includeAbsolute: false` at compiler-sfc.cjs.js
 * :1737–1739 / :3305). Otherwise an absolute asset URL stays an inert
 * string attribute instead of a module import.
 */
export const VUE_BUILD_TRANSFORM_ASSET_URLS = Object.freeze({ includeAbsolute: true });

/**
 * `compileTemplate` invocation contract. Every caller that drives the
 * pinned compiler's template half (production path and composition
 * self-test fragment rebuild) builds options here so the two cannot drift
 * into "the same call, one copy silently narrower" — how
 * `transformAssetUrls` went missing from the production call.
 *
 * Deliberately not passed (no committed fixture exercises them; a guess
 * is worse than an omission; each fails loud if a future fixture reaches
 * it): `compilerOptions.expressionPlugins` (official pushes `"typescript"`
 * for `lang="ts"`/`"tsx"` — no fixture has a TS script) and
 * `preprocessLang` / `preprocessOptions` (no `<template lang="…">`). Adding
 * a fixture in either class means extending this builder, not patching a
 * call site.
 *
 * @param {{ descriptor: object, filename: string, ssr: boolean, vapor: boolean,
 *   isProd: boolean, sourceMap: boolean, scriptBindings: object|undefined }} input
 */
export function vueTemplateCompileOptions({
  descriptor,
  filename,
  ssr,
  vapor,
  isProd,
  sourceMap,
  scriptBindings,
}) {
  return {
    source: descriptor.template.content,
    filename,
    id: filename,
    scoped: descriptor.styles.some((style) => style.scoped),
    slotted: descriptor.slotted,
    isProd,
    ssr,
    vapor,
    // Official bundler tooling passes the descriptor's v-bind() css-vars
    // (@vitejs/plugin-vue@6.0.7, dist/index.mjs:222). SSR relocates css-vars
    // into the render half's `_cssVars`/`_mergeProps` merge only when
    // compileTemplate sees the inventory here.
    ssrCssVars: descriptor.cssVars,
    // Official build-mode asset resolution, passed explicitly — never the
    // compiler's narrower default. See VUE_BUILD_TRANSFORM_ASSET_URLS.
    transformAssetUrls: VUE_BUILD_TRANSFORM_ASSET_URLS,
    // Descriptor block map chains the render fragment's original
    // coordinates to whole-fixture-file positions. Without it, fragment
    // map originals are template-block-relative and the published map
    // mis-anchors.
    inMap: sourceMap ? (descriptor.template.map ?? undefined) : undefined,
    compilerOptions: {
      mode: "module",
      bindingMetadata: scriptBindings,
    },
  };
}

/**
 * Compile one independently-authored Vue SFC fixture with the official
 * pinned compiler.
 *
 * @param {string} source raw `.vue` SFC text
 * @param {string} filename a stable, corpus-relative filename (used only as
 *   a source-map/diagnostic label — never read from disk)
 * @param {{ backend: VueBackend, sourceMap: boolean, isProd: boolean }} options
 * @returns {{
 *   code: string, map: object|null, diagnostics: Array<object>,
 *   backend: VueBackend, bindingMetadata: object|null,
 * }}
 */
export function compileVueFixture(source, filename, options) {
  assertVuePinned();
  const { parse, compileScript, compileTemplate } = vueCompilerSfc();
  const { backend, sourceMap, isProd } = options;
  const ssr = backend === "ssr";
  const vapor = backend === "vapor";
  const diagnostics = [];

  const { descriptor, errors: parseErrors } = parse(source, { filename, sourceMap });
  for (const error of parseErrors) {
    diagnostics.push(toDiagnostic("parse-error", error, filename));
  }
  if (diagnostics.length > 0) {
    return { code: null, map: null, diagnostics, backend, bindingMetadata: null };
  }

  let bindingMetadata = null;
  let scriptCode = null;
  let scriptMap = null;
  // Absent, not empty: official tooling passes
  // `bindingMetadata: resolvedScript ? resolvedScript.bindings : void 0`
  // (@vitejs/plugin-vue@6.0.7, dist/index.mjs:229). compiler-core's
  // render-arity branch is truthy-gated (`options.bindingMetadata &&
  // !options.inline`); `{}` for a script-less SFC would emit a 6-parameter
  // render official never emits.
  let scriptBindings;
  const hasScriptSetup = Boolean(descriptor.scriptSetup);

  if (hasScriptSetup || descriptor.script) {
    try {
      // Profile must reach every official compile step, not only
      // compileTemplate. compileScript derives script-half backend
      // semantics (`vapor = sfc.vapor || options.vapor`,
      // `ssr = options.templateOptions?.ssr`): vapor gates `__vapor: true`
      // / `defineVaporComponent`; ssr gates client-only injection such as
      // `useCssVars`. It also reads `options.isProd` (scoped css-vars
      // hashing, TS type-declared prop erasure).
      const compiled = compileScript(descriptor, {
        id: filename,
        inlineTemplate: false,
        sourceMap,
        isProd,
        vapor,
        templateOptions: { ssr },
      });
      scriptCode = compiled.content;
      scriptMap = compiled.map ?? null;
      bindingMetadata = compiled.bindings ?? {};
      // Threaded verbatim as official tooling threads
      // `resolvedScript.bindings` — never widened to `{}` (truthiness
      // reason above). On every reachable path the pinned compiler returns
      // a truthy bindings object here; the tested case is the script-less
      // one above, where the binding is absent.
      scriptBindings = compiled.bindings;
    } catch (error) {
      diagnostics.push(toDiagnostic("script-error", error, filename));
      return { code: null, map: null, diagnostics, backend, bindingMetadata: null };
    }
  }

  const templateResult = compileTemplate(
    vueTemplateCompileOptions({
      descriptor,
      filename,
      ssr,
      vapor,
      isProd,
      sourceMap,
      scriptBindings,
    }),
  );
  for (const error of templateResult.errors ?? []) {
    diagnostics.push(toDiagnostic("template-error", error, filename));
  }
  if (diagnostics.some((d) => d.kind === "template-error")) {
    return { code: null, map: null, diagnostics, backend, bindingMetadata };
  }

  const assembly = assembleAndValidate({
    scriptCode,
    renderCode: templateResult.code,
    ssr,
    vapor,
  });
  for (const fragment of assembly.fragmentDiagnostics) {
    diagnostics.push({ ...fragment, source: filename });
  }
  if (assembly.fragmentDiagnostics.length > 0) {
    return { code: null, map: null, diagnostics, backend, bindingMetadata };
  }

  // Published map describes the published assembled module: each official
  // fragment map is re-anchored by assembly geometry. See sourcemap.mjs.
  let map = null;
  if (sourceMap) {
    map = composeAssembledModuleMap(
      assembly.parts.map((part) => ({
        ...part,
        map:
          part.role === "script"
            ? scriptMap
            : part.role === "render"
              ? (templateResult.map ?? null)
              : null,
      })),
    );
  }

  return {
    code: assembly.code,
    map,
    diagnostics,
    backend,
    bindingMetadata,
  };
}

/**
 * Validate every fragment's own syntactic contract (fragments.mjs) before
 * assembling. Returns fragment diagnostics and assembled text separately:
 * assembly parseability is not fragment well-formedness, and a valid
 * fragment set can still produce an unparseable assembly.
 *
 * When any fragment is invalid, no assembly is attempted (`code` is null).
 * Assembling around a known-invalid fragment is the fail-open the textual
 * assembler used to permit.
 *
 * @returns {{ code: string|null, fragmentDiagnostics: Array<object>,
 *   parts: Array<object>|null }} `parts` is the assembly geometry
 *   (role/preEditCode/postEditCode/edit per fragment) map composition
 *   consumes — see sourcemap.mjs `composeAssembledModuleMap`.
 */
export function assembleAndValidate({ scriptCode, renderCode, ssr, vapor }) {
  const validation = validateVueFragments({ scriptCode, renderCode, ssr });
  const fragmentDiagnostics = validation.fragments
    .filter((f) => !f.parseOk || !f.shapeOk)
    .map((f) => ({
      kind: "fragment-error",
      code: f.parseOk ? "fragment-shape" : "fragment-parse",
      message: `${f.kind} fragment invalid: ${f.error}`,
      start: null,
      end: null,
    }));
  const assembled = validation.ok
    ? assembleNonInline({ scriptCode, renderCode, ssr, vapor })
    : null;
  return {
    code: assembled?.code ?? null,
    fragmentDiagnostics,
    parts: assembled?.parts ?? null,
  };
}

/**
 * Rewrite a fragment by syntax location, never text search: parse, locate
 * the statement node, replace only its exact `[from, to)` span. A string
 * literal that contains the target's source text is not the located
 * statement.
 */
function spliceAt(code, from, to, replacement) {
  return code.slice(0, from) + replacement + code.slice(to);
}

/**
 * Locate the module's `ExportDefaultDeclaration` and rewrite only that
 * node's export keywords to a `const` binding. Throws if not exactly one —
 * silent fallback would re-open unanchored rewrite.
 */
function rebindDefaultExport(scriptCode, bindingName) {
  const ast = parseModule(scriptCode, "script-fragment-rebind");
  const defaults = ast.body.filter((s) => s.type === "ExportDefaultDeclaration");
  if (defaults.length !== 1) {
    throw new Error(
      `script fragment must declare exactly one default export to assemble, found ${defaults.length}`,
    );
  }
  const [node] = defaults;
  // Replace the `export default` keyword span; leave the declaration
  // byte-identical.
  const replacement = `const ${bindingName} = `;
  return {
    code: spliceAt(scriptCode, node.start, node.declaration.start, replacement),
    edit: {
      start: node.start,
      end: node.declaration.start,
      replacementLength: replacement.length,
    },
  };
}

/**
 * Locate the exported render/ssrRender function declaration and strip only
 * its `export` keyword span. @returns {{ code, functionName }}
 */
function unexportRenderFunction(renderCode) {
  const ast = parseModule(renderCode, "render-fragment-rebind");
  const exported = ast.body.filter(
    (s) =>
      s.type === "ExportNamedDeclaration" &&
      s.declaration?.type === "FunctionDeclaration" &&
      (s.declaration.id?.name === "render" || s.declaration.id?.name === "ssrRender"),
  );
  if (exported.length !== 1) {
    throw new Error(
      `render fragment must export exactly one render/ssrRender function declaration to assemble, found ${exported.length}`,
    );
  }
  const [node] = exported;
  return {
    code: spliceAt(renderCode, node.start, node.declaration.start, ""),
    edit: { start: node.start, end: node.declaration.start, replacementLength: 0 },
    functionName: node.declaration.id.name,
  };
}

/**
 * Assemble the official non-inline module shape: `_sfc_main` plus a
 * separate render function as `_sfc_main.render` / `_sfc_main.ssrRender`,
 * matching `@vitejs/plugin-vue` assembly. Both rewrites are syntax-located;
 * a string literal containing `"export default"` is never touched.
 */
function assembleNonInline({ scriptCode, renderCode, ssr, vapor }) {
  const renderProp = ssr ? "ssrRender" : "render";
  // Scriptless SFCs get a synthesized component object; vapor assembly
  // attaches the runtime-interop marker there (no compileScript output).
  // Authority (not covered by the pinned oracle domain):
  // @vitejs/plugin-vue@6.0.7, dist/index.mjs:1424, genScriptCode:
  //   let scriptCode = `const ${scriptIdentifier} = { ${descriptor.vapor ? "__vapor: true" : ""} }`;
  // Marker necessity is proven against the pinned runtime (vapor-interop
  // mount controls); the emitted string is pinned by a literal regression.
  const rebound = scriptCode
    ? rebindDefaultExport(scriptCode, "_sfc_main")
    : { code: vapor ? "const _sfc_main = { __vapor: true }" : "const _sfc_main = {}", edit: null };
  const render = unexportRenderFunction(renderCode);
  const footer = [
    `_sfc_main.${renderProp} = ${render.functionName}`,
    "export default _sfc_main",
  ].join("\n");
  const parts = [
    {
      role: "script",
      preEditCode: scriptCode ?? rebound.code,
      postEditCode: rebound.code,
      edit: rebound.edit,
    },
    { role: "render", preEditCode: renderCode, postEditCode: render.code, edit: render.edit },
    { role: "footer", preEditCode: footer, postEditCode: footer, edit: null },
  ];
  return { code: parts.map((part) => part.postEditCode).join("\n"), parts };
}
