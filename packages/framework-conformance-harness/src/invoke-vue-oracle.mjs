// Hermetic invocation of the pinned official Vue 3.6.0-rc.3 compiler
// (`@vue/compiler-sfc`, backed by `@vue/compiler-dom` / `@vue/compiler-vapor`
// / `@vue/compiler-ssr` for the VDOM/vapor/ssr backends respectively). This
// module never writes an expectation; it only produces a raw compilation
// artifact for the caller (golden generator or comparator) to consume.
//
// Package pin is asserted on import — any drift from domain-pin.mjs throws
// before a single line of the oracle compiler runs.

import { parse, compileScript, compileTemplate } from "@vue/compiler-sfc";

import { VUE_DOMAIN } from "./domain-pin.mjs";
import { assertPackagesPinned } from "./package-pin.mjs";
import { EVIDENCE_LOCK_DIGESTS } from "./domain-pin.mjs";
import { HARNESS_ROOT, VUE_EVIDENCE_LOCK } from "./paths.mjs";

let pinned = false;
export function assertVuePinned() {
  if (pinned) return;
  assertPackagesPinned(
    VUE_DOMAIN,
    HARNESS_ROOT,
    VUE_EVIDENCE_LOCK,
    EVIDENCE_LOCK_DIGESTS.vuePackageLockSha256,
  );
  pinned = true;
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
  const { backend, sourceMap, isProd } = options;
  const ssr = backend === "ssr";
  const vapor = backend === "vapor";
  const diagnostics = [];

  const { descriptor, errors: parseErrors } = parse(source, { filename, sourceMap });
  for (const error of parseErrors) {
    diagnostics.push({ kind: "parse-error", message: String(error.message ?? error) });
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
      diagnostics.push({ kind: "script-error", message: String(error.message ?? error) });
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
    diagnostics.push({ kind: "template-error", message: String(error.message ?? error) });
  }
  if (diagnostics.some((d) => d.kind === "template-error")) {
    return { code: null, map: null, diagnostics, backend, bindingMetadata };
  }

  const assembled = assembleNonInline({ scriptCode, renderCode: templateResult.code, ssr, vapor });

  return {
    code: assembled,
    map: sourceMap ? (templateResult.map ?? null) : null,
    diagnostics,
    backend,
    bindingMetadata,
  };
}

/**
 * Assembles the official non-inline module shape: a component object
 * (`_sfc_main`, from `compileScript` or an empty object for template-only
 * SFCs) plus a SEPARATE render function attached as `_sfc_main.render` (or
 * `_sfc_main.ssrRender` for the SSR backend), matching the bundler-standard
 * SFC assembly official tooling (`@vitejs/plugin-vue`) produces.
 */
function assembleNonInline({ scriptCode, renderCode, ssr, vapor }) {
  const renderProp = ssr ? "ssrRender" : "render";
  const componentDecl = scriptCode
    ? scriptCode.replace("export default", "const _sfc_main =")
    : "const _sfc_main = {}";
  const renderExportName = vapor && !ssr ? "render" : ssr ? "ssrRender" : "render";
  const renderFnBody = renderCode.replace(
    /export function (?:render|ssrRender)/,
    `function ${renderExportName}`,
  );
  return [
    componentDecl,
    renderFnBody,
    `_sfc_main.${renderProp} = ${renderExportName}`,
    "export default _sfc_main",
  ].join("\n");
}
