// Fragment-level syntactic validation, INDEPENDENT of assembled-module
// parsing (BF2 required exit: "generated fragment and assembled JavaScript
// parsing" — two distinct signals, not one).
//
// The two signals are genuinely independent:
//  - fragment validity is checked here, per fragment, BEFORE any assembly;
//    the syntax-located assembler (invoke-vue-oracle.mjs) refuses to build
//    an assembly around an invalid fragment at all (code: null), so an
//    assembled-parse result cannot even EXIST for an invalid fragment —
//    assembled-parse success never stands in for fragment validity;
//  - a fragment set can be fully VALID while the assembly still breaks
//    (e.g. a script fragment that itself declares a `_sfc_main` binding
//    collides with the assembler's rebind, yielding a redeclaration the
//    assembled-module parse catches), so fragment validity never stands in
//    for assembled-parse success either. Both are always checked.
//
// The REAL fragment inventory this harness produces (read from the golden
// generation pipeline, not assumed):
//
//  Vue (`invoke-vue-oracle.mjs` assembles per-SFC output from exactly two
//  official compiler products):
//   - "script": `compileScript`'s output — a JavaScript module whose
//     syntactic contract is: parses standalone as a module AND declares
//     exactly one default export (the component object the assembler
//     rebinds to `_sfc_main`).
//   - "render": `compileTemplate`'s output — a JavaScript module whose
//     syntactic contract is: parses standalone as a module AND exports a
//     function declaration named `render` (client backends) or `ssrRender`
//     (SSR backend), which the assembler rebinds and attaches.
//
//  Svelte (`invoke-svelte-oracle.mjs`): the official compiler emits ONE
//  JavaScript module (`result.js`) with no harness-side assembly — the
//  "module" fragment IS the assembled module, so its syntactic contract is
//  simply a standalone module parse. The compiler's separate `css` artifact
//  is not part of the golden comparison surface (goldens record js code,
//  map, diagnostics only) and carries no JavaScript syntactic contract.

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
