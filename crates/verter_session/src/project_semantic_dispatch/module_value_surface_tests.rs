//! Values a module surface exposes beyond its plain declarations: the
//! members a declaration file's namespaces export without `export`, the
//! `var`s a script hoists out of its blocks onto the global scope, and the
//! value a module assigns with `export =`, read through every import form.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --declaration
//! --emitDeclarationOnly --strict` (`--module commonjs` for the import
//! assignments), read off the emitted `.d.ts`; a row the checker rejects
//! quotes its error.

use super::checker_probe_lane_tests::{mismatches_in, with_probe_in, ProbeProject};
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node;

const LIB_DTS: &str = "\
export namespace EN { function f(): 1; const c: 2; namespace Inner { function g(): 3 } }
export namespace EX { function f(): 4; export function h(): 5; export {}; }
export declare namespace ED { function f(): 6; }
export function top(): 7;
declare function hidden(): 8;
export namespace EV { var v: 9; let l: 10; }
";
const GLOBAL_DTS: &str = "\
declare namespace GD { function f(): 12; namespace Inner { function g(): 13 } }
declare namespace GX { function f(): 14; export function h(): 15; export {}; }
";
const DTS_USE: &str = "\
import { EN, EX, ED, top } from './lib';
import * as L from './lib';
export function callEnF() { return EN.f(); }
export function readEnC() { return EN.c; }
export function callEnInner() { return EN.Inner.g(); }
export function callExH() { return EX.h(); }
export function callEdF() { return ED.f(); }
export function callTop() { return top(); }
export function readEvV() { return L.EV.v; }
export function readEvL() { return L.EV.l; }
export function callGdF() { return GD.f(); }
export function callGdInner() { return GD.Inner.g(); }
export function callGxH() { return GX.h(); }
export function callHidden() { return L.hidden(); }
";

fn dts_project() -> [(&'static str, &'static str); 2] {
    [("lib.d.ts", LIB_DTS), ("glob.d.ts", GLOBAL_DTS)]
}

/// Everything in a declaration file is ambient: a namespace there exports
/// each member without `export` — unless its body holds an export
/// declaration (`export {}`), which leaves only the members written
/// `export` exported. A declaration-file module without an export
/// declaration exports its top-level declarations the same way.
///
/// Measured on TypeScript 7.0.2: `callEnF` is `1`, `readEnC` `2`,
/// `callEnInner` `3`, `callExH` `5`, `callEdF` `6`, `callTop` `7`,
/// `readEvV` `9`, `readEvL` `10`, `callGdF` `12`, `callGdInner` `13`,
/// `callGxH` `15`, and `callHidden` (a top-level `declare function` the
/// module never writes `export` on) `8`. `typeof EX.f` and `typeof GX.f`
/// are TS2339 — `f` is not exported beside `export {}` — and read as the
/// typed miss.
///
/// Mutation: lowering a declaration file's statements as non-ambient
/// leaves `callEnF`, `readEnC`, `readEvV` and `callGdF` unresolved;
/// ignoring the export declaration resolves `typeof EX.f`; recording no
/// implicit module export leaves `callHidden` unresolved.
#[test]
fn a_declaration_file_namespace_is_ambient() {
    let files = dts_project();
    let project = ProbeProject {
        files: &files,
        compiler_options: None,
        ambient_lib: None,
    };
    let failures = mismatches_in(
        project,
        DTS_USE,
        &[
            ("ReturnType<typeof callEnF>", "1"),
            ("ReturnType<typeof readEnC>", "2"),
            ("ReturnType<typeof callEnInner>", "3"),
            ("ReturnType<typeof callExH>", "5"),
            ("ReturnType<typeof callEdF>", "6"),
            ("ReturnType<typeof callTop>", "7"),
            ("ReturnType<typeof readEvV>", "9"),
            ("ReturnType<typeof readEvL>", "10"),
            ("ReturnType<typeof callGdF>", "12"),
            ("ReturnType<typeof callGdInner>", "13"),
            ("ReturnType<typeof callGxH>", "15"),
            ("ReturnType<typeof callHidden>", "8"),
            ("ReturnType<typeof EN.f>", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for probe in ["typeof EX.f", "typeof GX.f"] {
        let measured = with_probe_in(project, DTS_USE, probe, |dispatch, node| {
            render_node(dispatch, node, 0)
        });
        assert_eq!(measured, "Opaque(Miss)", "`{probe}` is not exported");
    }
}

const HOISTING_SCRIPT: &str = "\
declare const cond: boolean;
if (cond) { var bvIf = 1; } else { var bvElse = \"e\"; }
{ var bvBlock: boolean = true; }
for (var bvFor = 0; bvFor < 1; bvFor++) { var bvForBody = \"fb\"; }
for (var bvIn in { a: 1 }) {}
for (var bvOf of [1, 2]) {}
while (cond) { var bvWhile = 1n; }
do { var bvDo = null as string | null; } while (false);
try { var bvTry = 1; } catch (e) { var bvCatch = \"c\"; } finally { var bvFinally = true; }
switch (1 as number) { case 1: var bvCase = 1; break; default: var bvDefault = \"d\"; }
lbl: { var bvLabel = 1; }
function notHoisted() { var inner = 1; return inner; }
if (cond) { let blockLet = 1; }
var dupVar = 1;
if (cond) { var dupVar = 2; }
";
/// A script whose only global is a `var` inside a block.
const NESTED_ONLY_SCRIPT: &str = "if (Math.random() > 0.5) { var onlyNested = \"n\"; }\n";
const HOISTING_USE: &str = "\
export function readIf() { return bvIf; }
export function readElse() { return bvElse; }
export function readBlock() { return bvBlock; }
export function readFor() { return bvFor; }
export function readForBody() { return bvForBody; }
export function readIn() { return bvIn; }
export function readOf() { return bvOf; }
export function readWhile() { return bvWhile; }
export function readDo() { return bvDo; }
export function readTry() { return bvTry; }
export function readCatch() { return bvCatch; }
export function readFinally() { return bvFinally; }
export function readCase() { return bvCase; }
export function readDefault() { return bvDefault; }
export function readLabel() { return bvLabel; }
export function readGtIf() { return globalThis.bvIf; }
export function readDup() { return dupVar; }
export function readOnlyNested() { return onlyNested; }
export function readInner() { return inner; }
export function readBlockLet() { return blockLet; }
";

/// A `var` a script declares inside a block — an `if` or `else` branch, a
/// bare block, any loop (its head included), a `try`, `catch` or
/// `finally`, a `switch` case, a labeled statement — is scoped to the
/// script's top level, and so is a global; a `var` inside a function body
/// and a block-scoped `let` are not. A `for…in` variable is a key
/// (`string`), a `for…of` variable an element of the iterated array.
///
/// Measured on TypeScript 7.0.2: `readIf` is `number`, `readElse`
/// `string`, `readBlock` `boolean`, `readFor` `number`, `readForBody`
/// `string`, `readIn` `string`, `readOf` `number`, `readWhile` `bigint`,
/// `readDo` `string | null`, `readTry` `number`, `readCatch` `string`,
/// `readFinally` `boolean`, `readCase` `number`, `readDefault` `string`,
/// `readLabel` `number`, `readGtIf` (through `globalThis`) `number`,
/// `readDup` `number`, `readOnlyNested` `string`; `readInner` and
/// `readBlockLet` are TS2304 and read as the typed miss.
///
/// Mutation: indexing no nested `var` leaves every row unresolved;
/// declaring no `for…in` key type reads `readIn` as the implicit `any`, and
/// no `for…of` element leaves `readOf` untyped; a scan that sees no nested `var` never ingests the
/// script holding only one (`readOnlyNested`).
#[test]
fn a_var_in_a_script_block_is_global() {
    let files = [
        ("script.ts", HOISTING_SCRIPT),
        ("nested-only.ts", NESTED_ONLY_SCRIPT),
    ];
    let project = ProbeProject {
        files: &files,
        compiler_options: None,
        ambient_lib: None,
    };
    let failures = mismatches_in(
        project,
        HOISTING_USE,
        &[
            ("ReturnType<typeof readIf>", "number"),
            ("ReturnType<typeof readElse>", "string"),
            ("ReturnType<typeof readBlock>", "boolean"),
            ("ReturnType<typeof readFor>", "number"),
            ("ReturnType<typeof readForBody>", "string"),
            ("ReturnType<typeof readIn>", "string"),
            ("ReturnType<typeof readOf>", "number"),
            ("ReturnType<typeof readWhile>", "bigint"),
            ("ReturnType<typeof readDo>", "string | null"),
            ("ReturnType<typeof readTry>", "number"),
            ("ReturnType<typeof readCatch>", "string"),
            ("ReturnType<typeof readFinally>", "boolean"),
            ("ReturnType<typeof readCase>", "number"),
            ("ReturnType<typeof readDefault>", "string"),
            ("ReturnType<typeof readLabel>", "number"),
            ("ReturnType<typeof readGtIf>", "number"),
            ("ReturnType<typeof readDup>", "number"),
            ("ReturnType<typeof readOnlyNested>", "string"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for probe in [
        "ReturnType<typeof readInner>",
        "ReturnType<typeof readBlockLet>",
    ] {
        let measured = with_probe_in(project, HOISTING_USE, probe, |dispatch, node| {
            render_node(dispatch, node, 0)
        });
        assert!(
            measured.starts_with("Opaque("),
            "`{probe}` reads no global, measured `{measured}`"
        );
    }
}

const MODULE_HOISTING: &str = "\
declare const c: boolean;
if (c) { var mv = 1; } else { for (var mi in { a: 1 }) {} }
export function readMv() { return mv; }
export function readMi() { return mi; }
";

/// A module hoists a block's `var` to its own top level the same way.
///
/// Measured on TypeScript 7.0.2: `readMv` is `number`, `readMi` `string`.
#[test]
fn a_var_in_a_module_block_is_module_scoped() {
    let failures = super::checker_probe_lane_tests::mismatches(
        MODULE_HOISTING,
        &[
            ("ReturnType<typeof readMv>", "number"),
            ("ReturnType<typeof readMi>", "string"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const NS_EQ: &str =
    "declare namespace N { function f(): \"n-f\"; const c: \"n-c\"; }\nexport = N;\n";
const FN_EQ: &str = "declare function F(): \"f-call\";\nexport = F;\n";
const CLS_EQ: &str = "declare class C { m(): \"c-m\"; static s: \"c-s\"; }\nexport = C;\n";
const MERGED_EQ: &str = "declare function M(): \"m-call\";\n\
declare namespace M { const c: \"m-c\"; function g(): \"m-g\"; }\nexport = M;\n";
const NSX_EQ: &str = "declare namespace NX { function f(): \"nx-f\"; const c: \"nx-c\"; \
interface T { t: \"nx-t\" } namespace Inner { function g(): \"nx-g\" } }\nexport = NX;\n";

fn export_equals_project() -> [(&'static str, &'static str); 6] {
    [
        ("ns.d.ts", NS_EQ),
        ("fn.d.ts", FN_EQ),
        ("cls.d.ts", CLS_EQ),
        ("merged.d.ts", MERGED_EQ),
        ("nsx.d.ts", NSX_EQ),
        ("val.d.ts", VALUE_EQ),
    ]
}

const REQUIRE_USE: &str = "\
import N = require('./ns');
import F = require('./fn');
import C = require('./cls');
import M = require('./merged');
import NXR = require('./nsx');
export function reqNsF() { return N.f(); }
export function reqNsC() { return N.c; }
export function reqFn() { return F(); }
export function reqCls() { const c = new C(); return c.m(); }
export function reqClsS() { return C.s; }
export function reqM() { return M(); }
export function reqMc() { return M.c; }
export function reqMg() { return M.g(); }
export function reqT(t: NXR.T) { return t.t; }
export function reqInner() { return NXR.Inner.g(); }
";

/// `import x = require("m")` binds the value `m` assigns with `export =`:
/// a namespace, a function, a class, a function merged with a namespace.
///
/// Measured on TypeScript 7.0.2 with `--module commonjs`: `reqNsF` is
/// `"n-f"`, `reqNsC` `"n-c"`, `reqFn` `"f-call"`, `reqCls` `"c-m"`,
/// `reqClsS` `"c-s"`, `reqM` `"m-call"`, `reqMc` `"m-c"`, `reqMg` `"m-g"`,
/// `reqT` (a type through the binding) `"nx-t"`, `reqInner` `"nx-g"`.
///
/// Mutation: recording no route for an import assignment leaves every row
/// unresolved; reading the binding as a namespace import leaves `reqFn`
/// and `reqM` uncallable.
#[test]
fn an_import_assignment_binds_the_assigned_value() {
    let files = export_equals_project();
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            compiler_options: None,
            ambient_lib: None,
        },
        REQUIRE_USE,
        &[
            ("ReturnType<typeof reqNsF>", "\"n-f\""),
            ("ReturnType<typeof reqNsC>", "\"n-c\""),
            ("ReturnType<typeof reqFn>", "\"f-call\""),
            ("ReturnType<typeof reqCls>", "\"c-m\""),
            ("ReturnType<typeof reqClsS>", "\"c-s\""),
            ("ReturnType<typeof reqM>", "\"m-call\""),
            ("ReturnType<typeof reqMc>", "\"m-c\""),
            ("ReturnType<typeof reqMg>", "\"m-g\""),
            ("ReturnType<typeof reqT>", "\"nx-t\""),
            ("ReturnType<typeof reqInner>", "\"nx-g\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const DEFAULT_AND_NAMED_USE: &str = "\
import N from './ns';
import F from './fn';
import C from './cls';
import M from './merged';
import { f, c, Inner } from './nsx';
import type { T } from './nsx';
export function dfNsF() { return N.f(); }
export function dfNsC() { return N.c; }
export function dfFn() { return F(); }
export function dfCls() { const k = new C(); return k.m(); }
export function dfClsS() { return C.s; }
export function dfM() { return M(); }
export function dfMc() { return M.c; }
export function dfMg() { return M.g(); }
export function namedF() { return f(); }
export function namedC() { return c; }
export function namedInner() { return Inner.g(); }
export function namedT(t: T) { return t.t; }
";

/// A default import of a module that assigns `export = X` is X itself
/// (TypeScript 7 always applies the interop), and a named import names one
/// of X's namespace members, as a value or a type.
///
/// Measured on TypeScript 7.0.2: `dfNsF` is `"n-f"`, `dfNsC` `"n-c"`,
/// `dfFn` `"f-call"`, `dfCls` `"c-m"`, `dfClsS` `"c-s"`, `dfM` `"m-call"`,
/// `dfMc` `"m-c"`, `dfMg` `"m-g"`, `namedF` `"nx-f"`, `namedC` `"nx-c"`,
/// `namedInner` `"nx-g"`, `namedT` `"nx-t"`.
///
/// Mutation: exporting no `default` from the assignment leaves every `df*`
/// row unresolved; exporting no namespace member leaves every `named*` row
/// unresolved.
#[test]
fn a_default_or_named_import_reads_the_assigned_value() {
    let files = export_equals_project();
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            compiler_options: None,
            ambient_lib: None,
        },
        DEFAULT_AND_NAMED_USE,
        &[
            ("ReturnType<typeof dfNsF>", "\"n-f\""),
            ("ReturnType<typeof dfNsC>", "\"n-c\""),
            ("ReturnType<typeof dfFn>", "\"f-call\""),
            ("ReturnType<typeof dfCls>", "\"c-m\""),
            ("ReturnType<typeof dfClsS>", "\"c-s\""),
            ("ReturnType<typeof dfM>", "\"m-call\""),
            ("ReturnType<typeof dfMc>", "\"m-c\""),
            ("ReturnType<typeof dfMg>", "\"m-g\""),
            ("ReturnType<typeof namedF>", "\"nx-f\""),
            ("ReturnType<typeof namedC>", "\"nx-c\""),
            ("ReturnType<typeof namedInner>", "\"nx-g\""),
            ("ReturnType<typeof namedT>", "\"nx-t\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const NAMESPACE_IMPORT_USE: &str = "\
import * as N from './ns';
import * as F from './fn';
import * as C from './cls';
import * as M from './merged';
import * as NXS from './nsx';
import * as VS from './val';
export function starNsF() { return N.f(); }
export function starNsC() { return N.c; }
export function starClsS() { return C.s; }
export function starMc() { return M.c; }
export function starMg() { return M.g(); }
export function starFnDefault() { return F.default(); }
export function starClsDefault() { const k = new C.default(); return k.m(); }
export function starMDefault() { return M.default(); }
export function starMDefaultC() { return M.default.c; }
export function starT(t: NXS.T) { return t.t; }
export function starInner() { return NXS.Inner.g(); }
export function starVa() { return VS.a; }
";

/// A namespace import of a module that assigns `export = X` reads X's
/// members, and its `default` is X when X can be called or constructed.
///
/// Measured on TypeScript 7.0.2: `starNsF` is `"n-f"`, `starNsC` `"n-c"`,
/// `starClsS` `"c-s"`, `starMc` `"m-c"`, `starMg` `"m-g"`, `starFnDefault`
/// `"f-call"`, `starClsDefault` `"c-m"`, `starMDefault` `"m-call"`,
/// `starMDefaultC` `"m-c"`, `starT` `"nx-t"`, `starInner` `"nx-g"`, `starVa`
/// (over `declare const V: { a: "v-a"; … }`) `"v-a"`. `typeof N.default` and
/// `typeof VS.default.a` are TS2339 — neither a namespace nor a value that
/// cannot be called or constructed has a `default` — and read as the typed
/// miss. Every row answers alike under the four `strictNullChecks` ×
/// `noImplicitAny` settings.
///
/// Mutation: reading `default` as a namespace member leaves every
/// `star*Default*` row unresolved; naming X by `default` whatever X is, in
/// the value lookup or the qualified-name lookup, reads
/// `typeof VS.default.a` as `"v-a"`; reading no member of X's own
/// leaves `starClsS` unresolved.
#[test]
fn a_namespace_import_reads_the_assigned_value_members() {
    let files = export_equals_project();
    let project = ProbeProject {
        files: &files,
        compiler_options: None,
        ambient_lib: None,
    };
    let failures = mismatches_in(
        project,
        NAMESPACE_IMPORT_USE,
        &[
            ("ReturnType<typeof starNsF>", "\"n-f\""),
            ("ReturnType<typeof starNsC>", "\"n-c\""),
            ("ReturnType<typeof starClsS>", "\"c-s\""),
            ("ReturnType<typeof starMc>", "\"m-c\""),
            ("ReturnType<typeof starMg>", "\"m-g\""),
            ("ReturnType<typeof starFnDefault>", "\"f-call\""),
            ("ReturnType<typeof starClsDefault>", "\"c-m\""),
            ("ReturnType<typeof starMDefault>", "\"m-call\""),
            ("ReturnType<typeof starMDefaultC>", "\"m-c\""),
            ("ReturnType<typeof starT>", "\"nx-t\""),
            ("ReturnType<typeof starInner>", "\"nx-g\""),
            ("ReturnType<typeof starVa>", "\"v-a\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for probe in ["typeof N.default", "typeof VS.default.a"] {
        let measured = with_probe_in(project, NAMESPACE_IMPORT_USE, probe, |dispatch, node| {
            render_node(dispatch, node, 0)
        });
        assert!(
            measured.starts_with("Opaque("),
            "`{probe}` names no `default`, measured `{measured}`"
        );
    }
}

const MERGED_DTS: &str = "\
export namespace EN { interface T { t: \"en-t\" } type A = \"en-a\"; namespace Inner { interface U { u: \"en-u\" } const k: \"en-k\"; } class K { m(): \"en-km\"; static s: \"en-ks\"; } }
export namespace EN { const second: \"en-second\"; }
export function MF(): \"mf-call\";
export namespace MF { const c: \"mf-c\"; interface T { t: \"mf-t\" } }
export class MC { m(): \"mc-m\"; }
export namespace MC { const c: \"mc-c\"; }
export namespace EX { interface Hidden { h: 1 } export interface Shown { s: \"ex-s\" } export {}; }
";
const MERGED_DTS_USE: &str = "\
import { EN, MF, MC, EX } from './merged';
import * as M from './merged';
export function readT(t: EN.T) { return t.t; }
export function readA(a: EN.A) { return a; }
export function readU(u: EN.Inner.U) { return u.u; }
export function readK() { return EN.Inner.k; }
export function readKm() { const k = new EN.K(); return k.m(); }
export function readKs() { return EN.K.s; }
export function readSecond() { return EN.second; }
export function callMf() { return MF(); }
export function readMfC() { return MF.c; }
export function readMfT(t: MF.T) { return t.t; }
export function readMcM() { const k = new MC(); return k.m(); }
export function readMcC() { return MC.c; }
export function readShown(s: EX.Shown) { return s.s; }
export function readStarU(u: M.EN.Inner.U) { return u.u; }
";

/// A declaration file's namespace exports its TYPES as it exports its
/// values — an interface, an alias, a nested namespace's members, a class
/// — through a named import of the namespace and through a namespace
/// import of the module, and a namespace merges with a function, a class
/// or another block of the same namespace. A member the namespace does not
/// export (beside `export {}`) is not visible from outside.
///
/// Measured on TypeScript 7.0.2: `readT` is `"en-t"`, `readA` `"en-a"`,
/// `readU` `"en-u"`, `readK` `"en-k"`, `readKm` `"en-km"`, `readKs`
/// `"en-ks"`, `readSecond` `"en-second"`, `callMf` `"mf-call"`, `readMfC`
/// `"mf-c"`, `readMfT` `"mf-t"`, `readMcM` `"mc-m"`, `readMcC` `"mc-c"`,
/// `readShown` `"ex-s"`, `readStarU` `"en-u"`; `EX.Hidden` is TS2694 and
/// reads as the typed miss. Every row answers alike under the four
/// `strictNullChecks` × `noImplicitAny` settings.
///
/// Mutation: resolving no namespace member through a named import leaves
/// `readT`, `readA`, `readU` and `readMfT` unresolved, and none through a
/// namespace import's export `readStarU`; recording no private member
/// makes `EX.Hidden['h']` read `1`.
#[test]
fn a_declaration_file_namespace_exports_its_types_and_merges() {
    let files = [("merged.d.ts", MERGED_DTS)];
    let project = ProbeProject {
        files: &files,
        compiler_options: None,
        ambient_lib: None,
    };
    let failures = mismatches_in(
        project,
        MERGED_DTS_USE,
        &[
            ("ReturnType<typeof readT>", "\"en-t\""),
            ("ReturnType<typeof readA>", "\"en-a\""),
            ("ReturnType<typeof readU>", "\"en-u\""),
            ("ReturnType<typeof readK>", "\"en-k\""),
            ("ReturnType<typeof readKm>", "\"en-km\""),
            ("ReturnType<typeof readKs>", "\"en-ks\""),
            ("ReturnType<typeof readSecond>", "\"en-second\""),
            ("ReturnType<typeof callMf>", "\"mf-call\""),
            ("ReturnType<typeof readMfC>", "\"mf-c\""),
            ("ReturnType<typeof readMfT>", "\"mf-t\""),
            ("ReturnType<typeof readMcM>", "\"mc-m\""),
            ("ReturnType<typeof readMcC>", "\"mc-c\""),
            ("ReturnType<typeof readShown>", "\"ex-s\""),
            ("ReturnType<typeof readStarU>", "\"en-u\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let measured = with_probe_in(
        project,
        MERGED_DTS_USE,
        "EX.Hidden['h']",
        |dispatch, node| render_node(dispatch, node, 0),
    );
    assert!(
        measured.starts_with("Opaque("),
        "`EX.Hidden` is not exported, measured `{measured}`"
    );
}

const NAMESPACE_MODULE: &str = "\
export namespace EN { export interface T { t: \"en-t\" } export type A = \"en-a\"; export namespace Inner { export interface U { u: \"en-u\" } } }
export namespace P { namespace In { export interface U { u: 1 } } interface H { h: 1 } export const x = 1; }
";
const NAMESPACE_MODULE_USE: &str = "\
import { EN, P } from './ns';
import * as L from './ns';
export function readT(t: EN.T) { return t.t; }
export function readA(a: EN.A) { return a; }
export function readU(u: EN.Inner.U) { return u.u; }
export function readLT(t: L.EN.T) { return t.t; }
export function readLU(u: L.EN.Inner.U) { return u.u; }
";

/// Outside a declaration file a namespace exports only the members written
/// `export`, and they are reached the same way.
///
/// Measured on TypeScript 7.0.2: `readT` is `"en-t"`, `readA` `"en-a"`,
/// `readU` `"en-u"`, `readLT` `"en-t"`, `readLU` `"en-u"`; `P.In.U` and
/// `P.H` are TS2694 — neither `In` nor `H` is exported — and read as the
/// typed miss. Every row answers alike under the four `strictNullChecks` ×
/// `noImplicitAny` settings.
///
/// Mutation: recording no private member makes `P.In.U['u']` read `1`.
#[test]
fn a_namespace_exports_the_types_it_writes_export() {
    let files = [("ns.ts", NAMESPACE_MODULE)];
    let project = ProbeProject {
        files: &files,
        compiler_options: None,
        ambient_lib: None,
    };
    let failures = mismatches_in(
        project,
        NAMESPACE_MODULE_USE,
        &[
            ("ReturnType<typeof readT>", "\"en-t\""),
            ("ReturnType<typeof readA>", "\"en-a\""),
            ("ReturnType<typeof readU>", "\"en-u\""),
            ("ReturnType<typeof readLT>", "\"en-t\""),
            ("ReturnType<typeof readLU>", "\"en-u\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for probe in ["P.In.U['u']", "P.H['h']"] {
        let measured = with_probe_in(project, NAMESPACE_MODULE_USE, probe, |dispatch, node| {
            render_node(dispatch, node, 0)
        });
        assert!(
            measured.starts_with("Opaque("),
            "`{probe}` is not exported, measured `{measured}`"
        );
    }
}

const TOP_LEVEL_SCRIPT: &str = "\
var topVar = 1;
var topAnn: \"lit\" = \"lit\";
var noInit;
for (var bvOfStr of \"ab\") {}
function topFn(): \"fn\" { return \"fn\"; }
do { var bvDo = null as string | null; } while (false);
";
const TOP_LEVEL_USE: &str = "\
export function readTop() { return topVar; }
export function readTopAnn() { return topAnn; }
export function readNoInit() { return noInit; }
export function readOfStr() { return bvOfStr; }
export function gtFn() { return globalThis.topFn(); }
export function readDo() { return bvDo; }
";

/// A script's `var` is a global: read bare, through `globalThis`, and in
/// type position through `typeof globalThis`. One declared without an
/// initializer or an annotation is the implicit `any`; a `for…of` over a
/// string is typed by its characters.
///
/// Measured on TypeScript 7.0.2: `readTop` is `number`, `readTopAnn`
/// `"lit"`, `readNoInit` `any` (TS7005 under `noImplicitAny`), `readOfStr`
/// `string`, `gtFn` `"fn"`, `typeof globalThis.topVar` `number` and
/// `typeof globalThis.topAnn` `"lit"`; `readDo` is `string | null` with
/// `strictNullChecks` and `string` without it. Every row answers alike
/// under `noImplicitAny` on and off.
///
/// Mutation: declaring nothing for a declarator without an initializer
/// leaves `readNoInit` a typed miss.
#[test]
fn a_script_var_is_a_global_read_every_way() {
    let files = [("top.ts", TOP_LEVEL_SCRIPT)];
    for (compiler_options, read_do) in [
        (None, "string | null"),
        (
            Some(r#"{ "strict": true, "strictNullChecks": false }"#),
            "string",
        ),
    ] {
        let failures = mismatches_in(
            ProbeProject {
                files: &files,
                compiler_options,
                ambient_lib: None,
            },
            TOP_LEVEL_USE,
            &[
                ("ReturnType<typeof readTop>", "number"),
                ("ReturnType<typeof readTopAnn>", "\"lit\""),
                ("ReturnType<typeof readNoInit>", "any"),
                ("ReturnType<typeof readOfStr>", "string"),
                ("ReturnType<typeof gtFn>", "\"fn\""),
                ("ReturnType<typeof readDo>", read_do),
                ("typeof globalThis.topVar", "number"),
                ("typeof globalThis.topAnn", "\"lit\""),
            ],
        );
        assert!(
            failures.is_empty(),
            "{compiler_options:?}:\n{}",
            failures.join("\n")
        );
    }
}

const VALUE_EQ: &str = "declare const V: { a: \"v-a\"; b(): \"v-b\" };\nexport = V;\n";

/// The value and type surfaces `import x = require("m")` and a default
/// import give of a module that assigns `export = X`: X's own type, its
/// members, its call and construct signatures.
///
/// Measured on TypeScript 7.0.2 with `--module commonjs`: `reqVa` is
/// `"v-a"`, `reqVb` `"v-b"`, `dfVa` `"v-a"`, `typeof V`
/// `{ a: "v-a"; b(): "v-b"; }`, `typeof F` `() => "f-call"`,
/// `ReturnType<typeof F>` `"f-call"`, `InstanceType<typeof C>['m']`
/// `() => "c-m"` and `(typeof C)['s']` `"c-s"`; every row answers alike
/// under the four `strictNullChecks` × `noImplicitAny` settings.
///
/// Mutation: binding an import assignment to the module's namespace
/// instead of X leaves `typeof F` and `typeof V` unresolved.
#[test]
fn an_assigned_value_is_the_surface_of_its_import() {
    let files = [
        ("cls.d.ts", CLS_EQ),
        ("fn.d.ts", FN_EQ),
        ("val.d.ts", VALUE_EQ),
    ];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            compiler_options: None,
            ambient_lib: None,
        },
        "import C = require('./cls');\nimport F = require('./fn');\nimport V = require('./val');\n\
         import VD from './val';\n\
         export function reqVa() { return V.a; }\nexport function reqVb() { return V.b(); }\n\
         export function dfVa() { return VD.a; }\n",
        &[
            ("ReturnType<typeof reqVa>", "\"v-a\""),
            ("ReturnType<typeof reqVb>", "\"v-b\""),
            ("ReturnType<typeof dfVa>", "\"v-a\""),
            ("typeof V", "{ a: \"v-a\"; b(): \"v-b\"; }"),
            ("typeof F", "() => \"f-call\""),
            ("ReturnType<typeof F>", "\"f-call\""),
            ("InstanceType<typeof C>['m']", "() => \"c-m\""),
            ("(typeof C)['s']", "\"c-s\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
