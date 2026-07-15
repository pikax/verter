/**
 * The COMPONENT-NAME PARITY corpus generator.
 *
 * For the sources `svelte@5.56.3` COMPILES, Verter deconflicts a component's `name`
 * option against the runtime-binding surface and REPRODUCES svelte's own emitted name
 * (`get_component_name` → `module.scope.generate`, reserving
 * `references ∪ declarations ∪ conflicts`). The in-crate projection
 * (`SvelteScopeProjection` + OXC `SemanticBuilder`) mirrors svelte's
 * `remove_typescript_nodes ∘ create_scopes` scope view so a plain value-space filter
 * yields svelte's runtime bindings, and `derive_component_name` then suffixes a `_N`
 * counter until the name collides with nothing. A source svelte REJECTS (a diagnostic
 * code) or CRASHES (uncoded) emits NO component and NO name — name-parity is VACUOUS
 * there, and this corpus records svelte's reject code / crash outcome instead of an
 * emitted name (Verter's disposition for those rows — a defensive erase, a preserved
 * known-gap, or a fail-closed refusal — is characterized in the Rust conformance
 * module, NOT claimed as svelte parity here).
 *
 * The reserved surface used to be PINNED by hand in the in-crate matrix (three exact
 * sets per case), authored by reading the projection's own output — so a buggy
 * projection and a matching wrong pin BOTH passed (self-confirming). This generator
 * removes that hazard the same way the parse-parity generator does: it PLANTS an
 * identifier per construct, sets `name=<planted identifier>`, runs the PINNED official
 * compiler OFFLINE (`loadPinnedCompiler`), and RECORDS SVELTE'S OWN OUTCOME —
 *   - compile → `{ kind: "compile", emitted_name }` = the component function name svelte
 *     emits (`function <NAME>($$anchor)`): the bare planted name (not reserved) or
 *     `<name>_1` (reserved).
 *   - throw WITH a diagnostic code → `{ kind: "reject", code }`.
 *   - throw WITHOUT a code (an uncoded compiler CRASH, e.g. a lone class index signature)
 *     → `{ kind: "crash" }`, but ONLY for a construct marked `expect_crash`. An unexpected
 *     uncoded crash on any other construct fails generation HARD — a crash is NEVER
 *     recorded as a bogus reject and a `reject_code` is NEVER fabricated.
 * The outcomes are NEVER hardcoded; svelte fills them, so a Verter regression that
 * drops a reserved name now REDs against svelte's pin instead of matching a stale hand
 * value. Each construct also probes a fixed bare NEGATIVE-CONTROL name
 * (`Zz9Unrelated`, absent from every source ⇒ must stay bare) so the committed matrix
 * discriminates a reserved name from a non-reserved one (a projection that over-reserved
 * everything would turn the negative control into `Zz9Unrelated_1` and RED).
 *
 * Each row carries an `axis` (the svelte handler / construct it exercises). A single
 * data file `crates/verter_compiler/tests/fixtures/svelte/name_parity_corpus.json` is
 * committed; the Rust matrix reads it hermetically (no live svelte at test time), and
 * an optional `svelte-oracle`-gated freshness rail re-runs this generator's `--check`.
 *
 * Sibling of `gen-svelte-parse-parity-corpus.mjs`; reuses the SHARED
 * `loadPinnedCompiler` / `SVELTE_ORACLE_VERSION` from `svelte-golden-lib.mjs` (the
 * single oracle pin).
 *
 * USAGE
 *   node scripts/gen-svelte-name-parity-corpus.mjs           # rewrite the corpus
 *   node scripts/gen-svelte-name-parity-corpus.mjs --check   # assert in sync (CI gate)
 */

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

import { loadPinnedCompiler, SVELTE_ORACLE_VERSION } from "./svelte-golden-lib.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(__dirname, "..");
const CORPUS_PATH = join(
  REPO_ROOT,
  "crates/verter_compiler/tests/fixtures/svelte/name_parity_corpus.json",
);

// The bare NEGATIVE-CONTROL name — absent from EVERY source, so svelte always emits it
// bare. Every construct probes it, so the matrix can discriminate a reserved planted
// name (→ `_1`) from a non-reserved one (→ bare) and catch an over-reserving projection.
const NEGATIVE_CONTROL = "Zz9Unrelated";

// ---------------------------------------------------------------------------
// The enumerated constructs.
//
// Each `{ axis, source, probes }` names a script snippet and the semantically-relevant
// identifiers to plant as the `name` option. `probes` are the POSITIVE names; the bare
// negative control is appended for every compile construct. The pinned compiler decides
// each source's disposition (compile / reject / uncoded crash) and — for a compile — the
// emitted name per planted name; these lists are the INPUT space, NOT a pre-judged verdict.
//
// A source svelte REJECTS (a hard TypeScript-feature error, an illegal default export) OR
// CRASHES uncoded on (only an `expect_crash`-marked construct) yields a SINGLE row: the
// `name` option cannot change WHETHER the module compiles, so probing further names would
// only repeat the same reject code / crash. `probes[0]` is recorded as that row's
// (informational) `requested_name`.
// ---------------------------------------------------------------------------

function constructCases() {
  return [
    // The universal `_` type-strip handler: a type annotation contributes no value name.
    {
      axis: "const_type_annotation",
      source: "const y: OnlyType = realX;",
      probes: ["y", "realX", "OnlyType"],
    },
    // Imports: a runtime default import binds its local; a whole-statement type import erases.
    { axis: "import_default", source: 'import Foo from "m";', probes: ["Foo"] },
    { axis: "import_type_named", source: 'import type { Bar } from "m";', probes: ["Bar"] },
    // Named exports: a value export binds; a type export erases.
    { axis: "export_const", source: "export const EN = 1;", probes: ["EN"] },
    { axis: "export_type_alias", source: "export type ENT = string;", probes: ["ENT"] },
    // Re-exports: `export * as ns` reserves `ns` (svelte binds the namespace re-export
    // name in its conflict domain); a type-only `export type *` binds nothing.
    { axis: "export_star_as_ns", source: 'export * as ns from "m";', probes: ["ns"] },
    { axis: "export_type_star", source: 'export type * from "m";', probes: [] },
    // Pure TS declarations svelte erases → the name is bare.
    { axis: "interface_decl", source: "interface Iface { n: number }", probes: ["Iface"] },
    { axis: "type_alias", source: "type Alias = number;", probes: ["Alias"] },
    // A type-only namespace compiles to bare; a value namespace is a svelte HARD error.
    {
      axis: "namespace_type_only",
      source: "namespace TypeNs { export type T = number; }",
      probes: ["TypeNs"],
    },
    {
      axis: "namespace_value",
      source: "namespace ValueNs { export const V = 1; }",
      probes: ["ValueNs", "V"],
    },
    // A lone bodiless overload signature erases.
    {
      axis: "function_lone_overload",
      source: "function loneOverload(a: number): void;",
      probes: ["loneOverload"],
    },
    // Variables: a runtime `const` binds; an ambient `declare const` erases.
    { axis: "const_value", source: "const cv = 1;", probes: ["cv"] },
    { axis: "declare_const", source: "declare const dcv: number;", probes: ["dcv"] },
    // Classes: a runtime class binds; an ambient `declare class` erases.
    { axis: "class_decl", source: "class Cls {}", probes: ["Cls"] },
    { axis: "declare_class", source: "declare class DCls {}", probes: ["DCls"] },
    // Enums: svelte HARD-ERRORS on EVERY enum — ambient/`declare` AND plain (the
    // `TSEnumDeclaration` handler is unconditional).
    { axis: "enum_declare_ambient", source: "declare enum AmbientE { A }", probes: ["AmbientE"] },
    { axis: "enum_plain", source: "enum PlainE { A }", probes: ["PlainE"] },
    // TS expression carriers UNWRAP to the inner runtime reference; the type operand does
    // not reserve.
    { axis: "as_expression", source: "const av = inner as Ty;", probes: ["av", "inner", "Ty"] },
    {
      axis: "satisfies_expression",
      source: "const sv = other satisfies Ty;",
      probes: ["sv", "other", "Ty"],
    },
    { axis: "non_null_expression", source: "const nn = thing!;", probes: ["nn", "thing"] },
    {
      axis: "instantiation_expression",
      source: "const iv = generic<Ty>;",
      probes: ["iv", "generic", "Ty"],
    },
    // `<T>x`: svelte compiles + reserves the inner ref; Verter fail-closes (the shared tsx
    // reparse rejects `<T>x` as JSX). Kept as a Verter-fail-closed row, NOT a name-parity row.
    {
      axis: "type_assertion_angle",
      source: "const ta = <Foo>realX;",
      probes: ["ta", "realX", "Foo"],
    },
    // A class EXPRESSION binds its id in the class-expression scope (reserves).
    { axis: "class_expression", source: "const ce = class NamedCe {};", probes: ["ce", "NamedCe"] },
    // A `declare` field is dropped (its computed key never visited); the class name survives.
    {
      axis: "class_declare_field",
      source: "class DF { declare [DK]: number; }",
      probes: ["DF", "DK"],
    },
    // A computed-key field is KEPT (its key is a real reference); the class name survives.
    { axis: "class_computed_field", source: "class KP { [K] = 1; }", probes: ["KP", "K"] },
    // An abstract method is erased whole (its param never binds); the class name survives.
    {
      axis: "abstract_method",
      source: "abstract class AM { abstract m(P: number): void; }",
      probes: ["AM", "P", "m"],
    },
    // An `accessor` field is a svelte HARD error.
    { axis: "class_accessor_field", source: "class AP { accessor [P] = 1; }", probes: ["AP", "P"] },
    // A ctor param-property is a svelte HARD error.
    {
      axis: "class_param_property",
      source: "class PP { constructor(public X: number) {} }",
      probes: ["PP", "X"],
    },
    // A class index signature CRASHES pinned svelte@5.56.3 (an uncoded TypeError, NOT a
    // typed diagnostic). Verter's projection defensively erases it (the class name still
    // reserves) — a crash-parity gap, distinct from the typed reject-parity gaps.
    {
      axis: "class_index_signature",
      source: "class C { [k: string]: number }",
      probes: ["C"],
      expect_crash: true,
    },
    // A decorator is a svelte HARD error; Verter's projection PRESERVES it (a known reject-
    // parity gap), so the decorated class + decorator expression survive.
    { axis: "decorator", source: "@dec class Foo {}", probes: ["Foo", "dec"] },
    // A default export is illegal in a svelte component (`module_illegal_default_export`);
    // Verter's projection PRESERVES the default-export class name (a known reject-parity gap).
    { axis: "export_default_class", source: "export default class Named {}", probes: ["Named"] },
    // `remove_this_param` drops a leading `this` param; the fn name + real params reserve.
    {
      axis: "function_expression_this",
      source: "const fe = function inner(this: ThisT, a) { return a; };",
      probes: ["fe", "inner", "a", "ThisT"],
    },
    {
      axis: "function_declaration_this",
      source: "function fd(this: ThisT, b) { return b; }",
      probes: ["fd", "b", "ThisT"],
    },
  ];
}

// ---------------------------------------------------------------------------
// Disposition via the pinned compiler
// ---------------------------------------------------------------------------

// Wrap a script snippet in a minimal instance-script component with a trivial template,
// matching the surface svelte's `name` deconfliction runs over.
function wrapComponent(source) {
  return `<script lang="ts">${source}</script><div></div>`;
}

// The component function name svelte emits for a compiled module — `export default
// function <NAME>($$anchor)` (svelte 5 client), falling back to the `$$anchor`-first
// function signature. Throws if no component function is found (an unexpected emission
// shape must fail generation loudly, never silently record a wrong name).
function officialEmittedName(js) {
  let m = js.match(/export\s+default\s+function\s+([A-Za-z_$][\w$]*)\s*\(/);
  if (m) return m[1];
  m = js.match(/function\s+([A-Za-z_$][\w$]*)\s*\(\s*\$\$anchor/);
  if (m) return m[1];
  throw new Error(`could not locate the emitted component function name in:\n${js.slice(0, 400)}`);
}

// Compile one wrapped source through the pinned CLIENT compiler with an explicit `name`
// option, returning svelte's outcome:
//   - compiles → { kind: "compile", emitted_name }
//   - throws with a diagnostic code → { kind: "reject", code }
//   - throws WITHOUT a code (an uncoded compiler crash) → { kind: "crash" }, but ONLY for a
//     construct explicitly marked `expectCrash`. An UNEXPECTED uncoded crash on any other
//     construct fails generation HARD (so a crash never silently enters the corpus as a
//     bogus reject, and a NEW crash surfaces loudly). We never fabricate a reject code.
function officialOutcome(compiler, source, name, expectCrash = false) {
  let js;
  try {
    const result = compiler.compile(wrapComponent(source), {
      generate: "client",
      dev: false,
      name,
      filename: "App.svelte",
    });
    js = result.js.code;
  } catch (err) {
    const code = err && err.code;
    if (!code) {
      if (expectCrash) {
        return { kind: "crash" };
      }
      throw new Error(
        `svelte threw WITHOUT a diagnostic code for name=${name} source ${JSON.stringify(source)}: ` +
          `${err && err.message}`,
      );
    }
    return { kind: "reject", code };
  }
  return { kind: "compile", emitted_name: officialEmittedName(js) };
}

// ---------------------------------------------------------------------------
// Corpus build
// ---------------------------------------------------------------------------

function buildRows(compiler) {
  const rows = [];
  const seen = new Set();
  for (const c of constructCases()) {
    if (seen.has(c.axis)) {
      throw new Error(`duplicate name-parity construct axis: ${c.axis}`);
    }
    seen.add(c.axis);

    // Determine the source disposition ONCE via the negative control — the `name` option
    // never changes WHETHER a module compiles, only the emitted component-function name.
    const disposition = officialOutcome(
      compiler,
      c.source,
      NEGATIVE_CONTROL,
      c.expect_crash === true,
    );
    if (disposition.kind === "reject" || disposition.kind === "crash") {
      // A reject/crash source: one row (name-parity is vacuous — the `name` option cannot
      // change WHETHER svelte rejects/crashes, so probing further names only repeats it).
      const requested = c.probes[0] ?? NEGATIVE_CONTROL;
      rows.push({
        axis: c.axis,
        source: c.source,
        requested_name: requested,
        outcome:
          disposition.kind === "crash"
            ? { kind: "crash" }
            : { kind: "reject", code: disposition.code },
      });
      continue;
    }
    // A compile source: one row per planted positive name + the negative control, each
    // carrying svelte's emitted component-function name.
    for (const name of [...c.probes, NEGATIVE_CONTROL]) {
      const outcome = officialOutcome(compiler, c.source, name);
      if (outcome.kind !== "compile") {
        throw new Error(
          `source ${JSON.stringify(c.source)} compiled under name=${NEGATIVE_CONTROL} but ` +
            `REJECTED under name=${name} (${outcome.code}) — the disposition must not depend on the name`,
        );
      }
      rows.push({
        axis: c.axis,
        source: c.source,
        requested_name: name,
        outcome,
      });
    }
  }
  return rows;
}

function corpusJson(rows) {
  const compileCount = rows.filter((r) => r.outcome.kind === "compile").length;
  const rejectCount = rows.filter((r) => r.outcome.kind === "reject").length;
  const crashCount = rows.filter((r) => r.outcome.kind === "crash").length;
  return `${JSON.stringify(
    {
      svelte_oracle_version: SVELTE_ORACLE_VERSION,
      total: rows.length,
      compile: compileCount,
      reject: rejectCount,
      crash: crashCount,
      rows,
    },
    null,
    2,
  )}\n`;
}

function writeMode(compiler) {
  const rows = buildRows(compiler);
  const json = corpusJson(rows);
  mkdirSync(dirname(CORPUS_PATH), { recursive: true });
  writeFileSync(CORPUS_PATH, json);
  const compileCount = rows.filter((r) => r.outcome.kind === "compile").length;
  const rejectCount = rows.filter((r) => r.outcome.kind === "reject").length;
  const crashCount = rows.filter((r) => r.outcome.kind === "crash").length;
  console.log(
    `gen-svelte-name-parity-corpus: wrote ${rows.length} row(s) ` +
      `(${compileCount} compile, ${rejectCount} reject, ${crashCount} crash) from ` +
      `svelte@${SVELTE_ORACLE_VERSION} into ${relative(REPO_ROOT, CORPUS_PATH)}`,
  );
}

function checkMode(compiler) {
  const fresh = corpusJson(buildRows(compiler));
  if (!existsSync(CORPUS_PATH)) {
    console.error(
      `gen-svelte-name-parity-corpus --check: MISSING corpus ${relative(REPO_ROOT, CORPUS_PATH)}.\n` +
        `Run \`node scripts/gen-svelte-name-parity-corpus.mjs\` to regenerate.`,
    );
    process.exit(1);
  }
  const onDisk = readFileSync(CORPUS_PATH, "utf8");
  if (onDisk !== fresh) {
    console.error(
      `gen-svelte-name-parity-corpus --check: the committed corpus ` +
        `(${relative(REPO_ROOT, CORPUS_PATH)}) DRIFTED from a fresh regeneration against ` +
        `svelte@${SVELTE_ORACLE_VERSION}.\n\n` +
        `Run \`node scripts/gen-svelte-name-parity-corpus.mjs\` to regenerate.`,
    );
    process.exit(1);
  }
  const total = JSON.parse(onDisk).total;
  console.log(
    `gen-svelte-name-parity-corpus --check: ${total} row(s) in sync with svelte@${SVELTE_ORACLE_VERSION}.`,
  );
}

function main() {
  const check = process.argv.includes("--check");
  const compiler = loadPinnedCompiler(REPO_ROOT);
  if (check) checkMode(compiler);
  else writeMode(compiler);
}

main();
