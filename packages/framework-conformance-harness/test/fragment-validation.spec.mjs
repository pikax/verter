// Self-test: generated-FRAGMENT parse-failure detection, independent of
// assembled-module parsing (BF2 required exit).
//
// Fragment validity and assembled-module parseability are two independent
// signals; this suite proves the independence CONCRETELY in both
// directions with real assembler output:
//   direction 1 — a fragment is syntactically INVALID yet the assembled
//     module parses fine (the invalid fragment splices into a valid
//     statement), so assembled-parse success can never stand in for
//     fragment validity;
//   direction 2 — every fragment is VALID (parse + shape) yet assembly
//     itself produces an unparseable module, so fragment validity can never
//     stand in for assembled-parse success.
// Plus one malformed negative per declared fragment kind (the real
// inventory: Vue "script", Vue "render", Svelte "module" — enumerated from
// the golden generation pipeline in src/fragments.mjs).

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

import {
  checkSvelteFragment,
  checkVueFragment,
  validateVueFragments,
  VUE_FRAGMENT_KINDS,
  SVELTE_FRAGMENT_KINDS,
} from "../src/fragments.mjs";
import { assembleAndValidate, compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { compileSvelteFixture } from "../src/invoke-svelte-oracle.mjs";
import { checkParseValidity } from "../src/compare.mjs";
import { HARNESS_ROOT } from "../src/paths.mjs";

const VALID_RENDER = "export function render(_ctx) { return null }";

describe("fragment inventory", () => {
  it("the declared fragment kinds are exactly the pipeline's real inventory", () => {
    expect(VUE_FRAGMENT_KINDS).toEqual(["script", "render"]);
    expect(SVELTE_FRAGMENT_KINDS).toEqual(["module"]);
  });

  it("real official compiler output passes fragment validation for every kind", () => {
    const source = readFileSync(
      path.join(HARNESS_ROOT, "fixtures/vue/basic-interpolation.vue"),
      "utf8",
    );
    // compileVueFixture runs assembleAndValidate internally; zero
    // fragment-error diagnostics on real output proves the wiring is live
    // on the production path, not only in direct validator calls.
    for (const backend of ["vdom", "vapor", "ssr"]) {
      const artifact = compileVueFixture(source, "fixtures/vue/basic-interpolation.vue", {
        backend,
        sourceMap: false,
        isProd: false,
      });
      expect(artifact.code).toBeTruthy();
      expect(artifact.diagnostics.filter((d) => d.kind === "fragment-error")).toEqual([]);
    }
  });
});

describe("malformed negatives per fragment kind", () => {
  it("Vue script fragment: parse failure caught", () => {
    const result = checkVueFragment("script", "export default {{{ not js");
    expect(result.parseOk).toBe(false);
    expect(result.error).toBeTruthy();
  });

  it("Vue script fragment: shape violation caught (parses, but no default export)", () => {
    const result = checkVueFragment("script", "const _sfc_main = {}");
    expect(result.parseOk).toBe(true);
    expect(result.shapeOk).toBe(false);
    expect(result.error).toContain("default export");
  });

  it("Vue render fragment: parse failure caught", () => {
    const result = checkVueFragment("render", "export function render(", { ssr: false });
    expect(result.parseOk).toBe(false);
  });

  it("Vue render fragment: shape violation caught (parses, but wrong exported function name)", () => {
    const result = checkVueFragment("render", "export function notRender() {}", { ssr: false });
    expect(result.parseOk).toBe(true);
    expect(result.shapeOk).toBe(false);
    expect(result.error).toContain("render");
  });

  it("Vue render fragment: SSR backend requires ssrRender, not render", () => {
    const clientShaped = checkVueFragment("render", VALID_RENDER, { ssr: true });
    expect(clientShaped.shapeOk).toBe(false);
    const ssrShaped = checkVueFragment("render", "export function ssrRender() {}", { ssr: true });
    expect(ssrShaped.shapeOk).toBe(true);
  });

  it("Svelte module fragment: parse failure caught", () => {
    const result = checkSvelteFragment(
      "import * as $ from 'svelte/internal/client';\nvar root = $.from_html(`",
    );
    expect(result.parseOk).toBe(false);
  });

  it("Svelte module fragment: real official compiled output passes", () => {
    const source = readFileSync(
      path.join(HARNESS_ROOT, "fixtures/svelte/basic-runes.svelte"),
      "utf8",
    );
    const artifact = compileSvelteFixture(source, "fixtures/svelte/basic-runes.svelte", {
      generate: "client",
      runes: true,
      dev: false,
      sourceMap: false,
    });
    const result = checkSvelteFragment(artifact.code);
    expect(result.parseOk).toBe(true);
    expect(result.shapeOk).toBe(true);
  });
});

describe("fragment validity and assembled parse are INDEPENDENT signals", () => {
  it("direction 1: an INVALID script fragment is refused with fragment diagnostics and NO assembly is built around it", () => {
    // `export default` with no expression is not a valid module on its own.
    // The syntax-located assembler cannot (and must not) build an assembly
    // around a fragment it cannot parse — the MECHANISM fails closed:
    // fragment diagnostics report the precise defect and `code` is null,
    // so an assembled-parse "success" cannot even exist to stand in for
    // fragment validity.
    const brokenScript = "export default";
    expect(checkVueFragment("script", brokenScript).parseOk).toBe(false);
    const assembly = assembleAndValidate({
      scriptCode: brokenScript,
      renderCode: VALID_RENDER,
      ssr: false,
      vapor: false,
    });
    expect(assembly.fragmentDiagnostics.length).toBeGreaterThan(0);
    expect(assembly.fragmentDiagnostics[0].code).toBe("fragment-parse");
    expect(assembly.fragmentDiagnostics[0].message).toContain("script fragment invalid");
    expect(assembly.code).toBeNull(); // fail-closed: no assembly around an invalid fragment
  });

  it("direction 2: VALID fragments can still produce an UNPARSEABLE assembly (assembler-binding collision)", () => {
    // A script fragment that itself declares `_sfc_main` is a perfectly
    // valid module and satisfies the script shape contract (exactly one
    // default export), but the assembler's rebind introduces a SECOND
    // `const _sfc_main` declaration, so the assembly fails the
    // assembled-module parse (lexical redeclaration).
    const validButColliding = "const _sfc_main = 1;\nexport default _sfc_main;";
    const fragmentResult = checkVueFragment("script", validButColliding);
    expect(fragmentResult.parseOk).toBe(true);
    expect(fragmentResult.shapeOk).toBe(true);
    const assembly = assembleAndValidate({
      scriptCode: validButColliding,
      renderCode: VALID_RENDER,
      ssr: false,
      vapor: false,
    });
    expect(assembly.fragmentDiagnostics).toEqual([]); // fragments alone would MISS the defect
    const assembledParse = checkParseValidity(assembly.code, "assembled");
    expect(assembledParse.ok).toBe(false); // the assembled-parse oracle catches it
  });

  it("validateVueFragments reports each fragment kind independently", () => {
    const result = validateVueFragments({
      scriptCode: "export default {{{",
      renderCode: "export function notRender() {}",
      ssr: false,
    });
    expect(result.ok).toBe(false);
    expect(result.fragments.map((f) => f.kind)).toEqual(["script", "render"]);
    expect(result.fragments[0].parseOk).toBe(false);
    expect(result.fragments[1].parseOk).toBe(true);
    expect(result.fragments[1].shapeOk).toBe(false);
  });
});

describe("syntax-located assembly rewrites (never text search)", () => {
  // Regression (arbitration-demonstrated corruption): a source containing
  // the literal STRING "export default" — text, not export syntax. The
  // former unanchored textual replacement rewrote that first occurrence
  // (inside the string literal) and left the REAL default export untouched,
  // producing a duplicate default export that failed assembled parsing. The
  // syntax-located rewrite locates the actual ExportDefaultDeclaration AST
  // node and replaces only its span.
  const STRING_LITERAL_SCRIPT =
    'const banner = "export default";\nexport default { data: () => ({ banner }) };';

  it('a string literal containing "export default" is untouched; the real export IS rewritten (mechanism)', () => {
    const assembly = assembleAndValidate({
      scriptCode: STRING_LITERAL_SCRIPT,
      renderCode: VALID_RENDER,
      ssr: false,
      vapor: false,
    });
    expect(assembly.fragmentDiagnostics).toEqual([]);
    // The assembled module parses — no duplicate default export.
    const parsed = checkParseValidity(assembly.code, "assembled");
    expect(parsed.ok).toBe(true);
    // The string literal survived byte-identically…
    expect(assembly.code).toContain('const banner = "export default";');
    // …the real export was rebound…
    expect(assembly.code).toContain("const _sfc_main = { data: () => ({ banner }) };");
    // …and exactly ONE default export remains (the assembler's own).
    const defaults = parsed.ast.body.filter((s) => s.type === "ExportDefaultDeclaration");
    expect(defaults.length).toBe(1);
  });

  it("the same source drives cleanly through the compileVueFixture PRODUCTION path", () => {
    const sfc =
      "<script>\n" +
      'const banner = "export default";\n' +
      "export default { data: () => ({ banner }) };\n" +
      "</script>\n" +
      "<template><p>{{ banner }}</p></template>\n";
    const artifact = compileVueFixture(sfc, "fixtures/vue/string-literal-export-default.vue", {
      backend: "vdom",
      sourceMap: false,
      isProd: false,
    });
    expect(artifact.diagnostics).toEqual([]);
    expect(artifact.code).toBeTruthy();
    const parsed = checkParseValidity(artifact.code, "assembled");
    expect(parsed.ok).toBe(true);
    expect(artifact.code).toContain('"export default"'); // literal intact
    const defaults = parsed.ast.body.filter((s) => s.type === "ExportDefaultDeclaration");
    expect(defaults.length).toBe(1); // real export rewritten, not duplicated
  });

  it('the render-function rewrite is likewise syntax-located (a string containing "export function render" is untouched)', () => {
    const trickyRender =
      'const note = "export function render";\nexport function render(_ctx) { return note }';
    const assembly = assembleAndValidate({
      scriptCode: "export default {}",
      renderCode: trickyRender,
      ssr: false,
      vapor: false,
    });
    expect(assembly.fragmentDiagnostics).toEqual([]);
    const parsed = checkParseValidity(assembly.code, "assembled");
    expect(parsed.ok).toBe(true);
    expect(assembly.code).toContain('const note = "export function render";');
    // The real exported declaration lost only its `export` keyword.
    expect(assembly.code).toContain("function render(_ctx) { return note }");
  });
});
