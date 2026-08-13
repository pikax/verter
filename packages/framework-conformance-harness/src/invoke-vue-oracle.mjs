// Hermetic invocation of the pinned official Vue 3.6.0-rc.3 compiler
// (`@vue/compiler-sfc`, backed by `@vue/compiler-dom` / `@vue/compiler-vapor`
// / `@vue/compiler-ssr` for the VDOM/vapor/ssr backends respectively). This
// module never writes an expectation; it only produces a raw compilation
// artifact for the caller (golden generator or comparator) to consume.
//
// The compiler is loaded DYNAMICALLY from the isolated per-domain
// installation realized from the committed oracle lock (oracle-install.mjs)
// — never from workspace dependency resolution, and never via a static
// top-level import. The full validation gate (committed-evidence layers +
// realized-closure enumeration of the isolated install) runs before a
// single line of the oracle compiler is evaluated.

import { parseModule } from "./normalize.mjs";
import { validateVueFragments } from "./fragments.mjs";
import { VUE_DOMAIN } from "./domain-pin.mjs";
import { PackageDriftError } from "./package-pin.mjs";
import { ensureOracleDomain, oracleRequire } from "./oracle-install.mjs";

/**
 * Captures every contract-observable field the official compiler exposes on
 * a diagnostic: kind, code, message, source identity, start AND end spans.
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
 * Loads `@vue/compiler-sfc` from the validated isolated installation. The
 * gate (ensureOracleDomain inside oracleRequire) has passed before the
 * compiler module is evaluated. Vue routes Node `import` and `require` to
 * the same CJS dist artifact (its `node` export condition), so this is the
 * identical module shape either entry would load — with every transitive
 * dependency (postcss, @babel/parser, …) resolving from the realized
 * committed closure instead of the workspace store.
 */
function vueCompilerSfc() {
  if (compilerSfc === null) {
    const loaded = oracleRequire("vue", "@vue/compiler-sfc");
    // Loaded-module identity gate: the compiler ACTUALLY IN USE attests its
    // own version against the domain pin — a load that slipped through any
    // other resolution path (e.g. a workspace-hoisted @vue/compiler-sfc at
    // a different version) refuses here.
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

/** Version the LOADED oracle compiler attests — for identity self-tests. */
export function vueOracleCompilerVersion() {
  return vueCompilerSfc().version;
}

/**
 * @typedef {"vdom"|"vapor"|"ssr"} VueBackend
 */

/**
 * Compiles one independently-authored Vue SFC fixture with the official
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
  let scriptBindings = {};
  const hasScriptSetup = Boolean(descriptor.scriptSetup);

  if (hasScriptSetup || descriptor.script) {
    try {
      const compiled = compileScript(descriptor, {
        id: filename,
        inlineTemplate: false,
        sourceMap,
      });
      scriptCode = compiled.content;
      bindingMetadata = compiled.bindings ?? {};
      scriptBindings = bindingMetadata;
    } catch (error) {
      diagnostics.push(toDiagnostic("script-error", error, filename));
      return { code: null, map: null, diagnostics, backend, bindingMetadata: null };
    }
  }

  const templateResult = compileTemplate({
    source: descriptor.template.content,
    filename,
    id: filename,
    scoped: descriptor.styles.some((style) => style.scoped),
    slotted: descriptor.slotted,
    isProd,
    ssr,
    vapor,
    ssrCssVars: [],
    compilerOptions: {
      mode: "module",
      bindingMetadata: scriptBindings,
    },
  });
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

  return {
    code: assembly.code,
    map: sourceMap ? (templateResult.map ?? null) : null,
    diagnostics,
    backend,
    bindingMetadata,
  };
}

/**
 * Validates every fragment's OWN syntactic contract (fragments.mjs) before
 * assembling, and returns both signals — fragment diagnostics and the
 * assembled text — separately. Fragment validity and assembled-module
 * parseability are independent facts: a caller must never read "the
 * assembly parses" as "every fragment was well-formed", and a valid
 * fragment set can still produce an unparseable assembly (see fragments.mjs
 * for the concrete counterexample).
 *
 * When any fragment is invalid, NO assembly is attempted (`code` is null):
 * the syntax-located assembler's input contract is exactly the fragment
 * shape contract, and assembling around a known-invalid fragment is the
 * fail-open the textual assembler used to permit.
 *
 * @returns {{ code: string|null, fragmentDiagnostics: Array<object> }}
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
  return {
    code: validation.ok ? assembleNonInline({ scriptCode, renderCode, ssr, vapor }) : null,
    fragmentDiagnostics,
  };
}

/**
 * Rewrites a fragment by SYNTAX LOCATION, never text search: the fragment
 * is parsed, the target statement node is located in the AST, and only the
 * exact source span [from, to) that node identifies is replaced. A string
 * literal that happens to CONTAIN the target's source text is an ordinary
 * expression node, never the located statement, so it is untouchable by
 * construction.
 */
function spliceAt(code, from, to, replacement) {
  return code.slice(0, from) + replacement + code.slice(to);
}

/**
 * Locates the module's actual `ExportDefaultDeclaration` and rewrites ONLY
 * that node's export keywords to a `const` binding. Throws when the
 * fragment has no default export or more than one — the assembler's input
 * contract (fragments.mjs "script" shape) requires exactly one, and a
 * silent fallback here would re-open the unanchored-rewrite defect class.
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
  // Replace the `export default` keyword span — everything from the
  // statement's start up to its declaration expression — leaving the
  // declaration itself byte-identical.
  return spliceAt(scriptCode, node.start, node.declaration.start, `const ${bindingName} = `);
}

/**
 * Locates the exported render/ssrRender FUNCTION DECLARATION and strips
 * only its `export` keyword span. @returns {{ code, functionName }}
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
    functionName: node.declaration.id.name,
  };
}

/**
 * Assembles the official non-inline module shape: a component object
 * (`_sfc_main`, from `compileScript` or an empty object for template-only
 * SFCs) plus a SEPARATE render function attached as `_sfc_main.render` (or
 * `_sfc_main.ssrRender` for the SSR backend), matching the bundler-standard
 * SFC assembly official tooling (`@vitejs/plugin-vue`) produces.
 *
 * Both rewrites are syntax-located (see rebindDefaultExport /
 * unexportRenderFunction): the actual AST node is found and its exact span
 * replaced. A source that merely CONTAINS the text "export default" inside
 * a string literal is never touched by the rebind.
 */
function assembleNonInline({ scriptCode, renderCode, ssr }) {
  const renderProp = ssr ? "ssrRender" : "render";
  const componentDecl = scriptCode
    ? rebindDefaultExport(scriptCode, "_sfc_main")
    : "const _sfc_main = {}";
  const render = unexportRenderFunction(renderCode);
  return [
    componentDecl,
    render.code,
    `_sfc_main.${renderProp} = ${render.functionName}`,
    "export default _sfc_main",
  ].join("\n");
}
