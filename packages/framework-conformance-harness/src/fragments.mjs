// Fragment-level syntactic validation, independent of assembled-module
// parsing — two distinct signals:
//  - fragment validity is checked here, per fragment, before any assembly;
//    the syntax-located assembler (invoke-vue-oracle.mjs) refuses to build
//    around an invalid fragment (`code: null`), so assembled-parse success
//    never stands in for fragment validity;
//  - a valid fragment set can still break assembly (a script fragment that
//    declares `_sfc_main` collides with the assembler's rebind). Both
//    signals are always checked.
//
// Inventory (from the golden generation pipeline, not assumed):
//
//  Vue (`invoke-vue-oracle.mjs` assembles two official compiler products):
//   - "script": `compileScript` output — standalone module with exactly one
//     default export (rebound to `_sfc_main`).
//   - "render": `compileTemplate` output — standalone module exporting a
//     `render` / `ssrRender` function declaration.
//
//  Svelte (`invoke-svelte-oracle.mjs`): one JS module (`result.js`) with no
//  harness-side assembly — the "module" fragment is the assembled module.
//  The compiler's `css` artifact is not on the golden comparison surface.

import { parseModule } from "./normalize.mjs";

export const VUE_FRAGMENT_KINDS = ["script", "render"];
export const SVELTE_FRAGMENT_KINDS = ["module"];

/**
 * @returns {{ kind: string, parseOk: boolean, shapeOk: boolean, error: string|null }}
 */
export function checkVueFragment(kind, code, { ssr = false } = {}) {
  let ast;
  try {
    ast = parseModule(code, `${kind}-fragment`);
  } catch (error) {
    return { kind, parseOk: false, shapeOk: false, error: String(error.message ?? error) };
  }
  if (kind === "script") {
    const defaults = ast.body.filter((s) => s.type === "ExportDefaultDeclaration");
    if (defaults.length !== 1) {
      return {
        kind,
        parseOk: true,
        shapeOk: false,
        error: `script fragment must declare exactly one default export, found ${defaults.length}`,
      };
    }
    return { kind, parseOk: true, shapeOk: true, error: null };
  }
  if (kind === "render") {
    const expected = ssr ? "ssrRender" : "render";
    const exported = ast.body.some(
      (s) =>
        s.type === "ExportNamedDeclaration" &&
        s.declaration?.type === "FunctionDeclaration" &&
        s.declaration.id?.name === expected,
    );
    if (!exported) {
      return {
        kind,
        parseOk: true,
        shapeOk: false,
        error: `render fragment must export a function declaration named ${expected}`,
      };
    }
    return { kind, parseOk: true, shapeOk: true, error: null };
  }
  throw new Error(`unknown Vue fragment kind: ${kind}`);
}

/**
 * Validates every Vue fragment the assembler consumes. `scriptCode` is null
 * for template-only SFCs (the assembler synthesizes an empty component
 * object — there is no script fragment to validate in that case).
 *
 * @returns {{ ok: boolean, fragments: Array<{kind, parseOk, shapeOk, error}> }}
 */
export function validateVueFragments({ scriptCode, renderCode, ssr }) {
  const fragments = [];
  if (scriptCode !== null && scriptCode !== undefined)
    fragments.push(checkVueFragment("script", scriptCode));
  fragments.push(checkVueFragment("render", renderCode, { ssr }));
  return { ok: fragments.every((f) => f.parseOk && f.shapeOk), fragments };
}

/**
 * @returns {{ kind: "module", parseOk: boolean, shapeOk: boolean, error: string|null }}
 */
export function checkSvelteFragment(code) {
  try {
    parseModule(code, "svelte-module-fragment");
    return { kind: "module", parseOk: true, shapeOk: true, error: null };
  } catch (error) {
    return {
      kind: "module",
      parseOk: false,
      shapeOk: false,
      error: String(error.message ?? error),
    };
  }
}
