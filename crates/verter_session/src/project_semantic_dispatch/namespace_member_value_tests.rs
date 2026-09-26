//! Values a namespace member path or a cross-file global names: a
//! namespace's value member is its own declaration under its qualified
//! name, and a global declared in several files merges every file's
//! declaration. Once the value resolves, its merged declarations order as
//! any merged declaration's do: `SignaturesOfType` lists them in
//! declaration order, and call resolution tries a later declaration's
//! signatures first.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --noEmit` (strict by default):
//! `declare const v: <probe>; export const s: null = v;` read off the
//! TS2322 message.

use super::checker_probe_lane_tests::{mismatches, mismatches_in, with_probe, ProbeProject};
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node;

const NAMESPACES: &str = "\
declare namespace NS1 { function g(): 'one' }
export function callNs1() { return NS1.g(); }
declare namespace NC { const c: 'c'; namespace Inner { const d: 'd'; function h(): 'h'; namespace Deep { const e: 'e' } } }
export function readNc() { return NC.c; }
export function readNcInner() { return NC.Inner.d; }
export function callNcInner() { return NC.Inner.h(); }
export function readNcDeep() { return NC.Inner.Deep.e; }
declare function nf(): 'nf';
declare namespace nf { const v: 'nfv'; function w(): 'nfw' }
export function readNfV() { return nf.v; }
export function callNfW() { return nf.w(); }
export function callNf() { return nf(); }
declare class NK { static s: 'static' }
declare namespace NK { const k: 'nk'; function kf(): 'kf' }
export function readNkK() { return NK.k; }
export function callNkKf() { return NK.kf(); }
export function readNkS() { return NK.s; }
namespace Impl { export const x = 'impl' as const; export function y() { return 1 as const; } }
export function readImpl() { return Impl.x; }
export function callImpl() { return Impl.y(); }
declare namespace NO { function o(x: string): 'o-str'; function o(x: number): 'o-num' }
export function callNoNum() { return NO.o(1); }
";

/// A namespace member path reads the member's own declaration: a function,
/// a const, a member of a nested namespace, and a namespace merged into a
/// function or a class, read as a value, called, or through `typeof`.
///
/// Measured on TypeScript 7.0.2: `NS1.g()` is `"one"`, `NC.c` `"c"`,
/// `NC.Inner.d` `"d"`, `NC.Inner.h()` `"h"`, `NC.Inner.Deep.e` `"e"`,
/// `nf.v` `"nfv"`, `nf.w()`
/// `"nfw"`, `nf()` `"nf"`, `NK.k` `"nk"`, `NK.kf()` `"kf"`, `NK.s`
/// `"static"`, `Impl.x` `"impl"`, `Impl.y()` `1`, and `NO.o(1)` `"o-num"`;
/// the same through `typeof` (`ReturnType<typeof NO.o>` reads the last
/// overload, `"o-num"`).
///
/// Mutation: indexing a namespace's functions only when exported leaves
/// every `declare namespace` function row `Opaque(Miss)`; reading only the
/// first path segment as the qualified name leaves `NC.Inner.Deep.e`
/// unresolved (a two-segment prefix is retried as a root of its own);
/// reading a merged function's or class's own surface before its namespace
/// leaves `nf.*` and `NK.*` unresolved.
#[test]
fn a_namespace_member_reads_its_own_declaration() {
    let failures = mismatches(
        NAMESPACES,
        &[
            ("ReturnType<typeof callNs1>", "\"one\""),
            ("ReturnType<typeof readNc>", "\"c\""),
            ("ReturnType<typeof readNcInner>", "\"d\""),
            ("ReturnType<typeof callNcInner>", "\"h\""),
            ("ReturnType<typeof readNcDeep>", "\"e\""),
            ("ReturnType<typeof readNfV>", "\"nfv\""),
            ("ReturnType<typeof callNfW>", "\"nfw\""),
            ("ReturnType<typeof callNf>", "\"nf\""),
            ("ReturnType<typeof readNkK>", "\"nk\""),
            ("ReturnType<typeof callNkKf>", "\"kf\""),
            ("ReturnType<typeof readNkS>", "\"static\""),
            ("ReturnType<typeof readImpl>", "\"impl\""),
            ("ReturnType<typeof callImpl>", "1"),
            ("ReturnType<typeof callNoNum>", "\"o-num\""),
            ("ReturnType<typeof NS1.g>", "\"one\""),
            ("typeof NC.c", "\"c\""),
            ("typeof NC.Inner.d", "\"d\""),
            ("typeof NC.Inner.Deep.e", "\"e\""),
            ("ReturnType<typeof NC.Inner.h>", "\"h\""),
            ("ReturnType<typeof nf.w>", "\"nfw\""),
            ("typeof NK.k", "\"nk\""),
            ("ReturnType<typeof NK.kf>", "\"kf\""),
            ("typeof Impl.x", "\"impl\""),
            ("ReturnType<typeof NO.o>", "\"o-num\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const EXPORTS: &str = "\
declare namespace A { function f(): 1; export function g(): 2; const c: 3; }
namespace B { function hidden() { return 1 as const; } export function shown() { return 2 as const; } export namespace Inner { export const i = 'i' as const; } }
namespace C { declare function amb(): 'amb'; export declare function amb2(): 'amb2'; }
declare namespace D { namespace E { function e(): 'e' } }
declare namespace F { let l: 'l'; var v: 'v' }
";

/// An AMBIENT namespace exports every member, written `export` or not; an
/// ordinary one only the members it exports, `declare` or not.
///
/// Measured on TypeScript 7.0.2: `ReturnType<typeof A.f>` is `1`, `A.g`
/// returns `2`, `typeof A.c` is `3`; `B.shown` returns `2` and
/// `typeof B.Inner.i` is `"i"`; `C.amb2` returns `"amb2"`; `D.E.e` (a
/// namespace inside an ambient one) returns `"e"`; `typeof F.l` is `"l"`
/// and `typeof F.v` `"v"`. `typeof B.hidden` is TS2339 and `typeof C.amb`
/// TS2551: neither is exported, so neither resolves.
///
/// Mutation: indexing every namespace member regardless of export resolves
/// `B.hidden` and `C.amb`; not carrying ambience into a nested namespace
/// leaves `D.E.e` unresolved.
#[test]
fn an_ambient_namespace_exports_every_member() {
    let failures = mismatches(
        EXPORTS,
        &[
            ("ReturnType<typeof A.f>", "1"),
            ("ReturnType<typeof A.g>", "2"),
            ("typeof A.c", "3"),
            ("ReturnType<typeof B.shown>", "2"),
            ("typeof B.Inner.i", "\"i\""),
            ("ReturnType<typeof C.amb2>", "\"amb2\""),
            ("ReturnType<typeof D.E.e>", "\"e\""),
            ("typeof F.l", "\"l\""),
            ("typeof F.v", "\"v\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for probe in ["typeof B.hidden", "typeof C.amb"] {
        let measured = with_probe(EXPORTS, probe, |dispatch, node| {
            render_node(dispatch, node, 0)
        });
        assert_eq!(measured, "Opaque(Miss)", "`{probe}` is not exported");
    }
}

const MERGED_BLOCKS: &str = "\
declare namespace NS { function g(): 'ns1' }
declare namespace NS { function g(): 'ns2' }
export function callNs() { return NS.g(); }
declare namespace NB { function k(): 'k1'; }
declare namespace NB { function k(): 'k2'; function k(x: number): 'k2-num'; }
export function callNbK() { return NB.k(); }
export function callNbKNum() { return NB.k(1); }
declare namespace NT { function t(): 't1'; function t(): 't2'; }
export function callNtT() { return NT.t(); }
";

/// A namespace member declared in several blocks merges them: its
/// signatures list in declaration order, and a call tries a later block's
/// first. Overloads inside one block keep their own order.
///
/// Measured on TypeScript 7.0.2: `NS.g()` and `ReturnType<typeof NS.g>`
/// are `"ns2"`; `NB.k()` is `"k2"`, `NB.k(1)` `"k2-num"`, and
/// `ReturnType<typeof NB.k>` — the last signature — `"k2-num"`; `NT.t()`,
/// two overloads of one block, is `"t1"`.
///
/// Mutation: one flat overload list answers `NS.g()` with `"ns1"` and
/// `NB.k()` with `"k1"`; one group per signature answers `NT.t()` with
/// `"t2"`.
#[test]
fn namespace_blocks_merge_later_first() {
    let failures = mismatches(
        MERGED_BLOCKS,
        &[
            ("ReturnType<typeof callNs>", "\"ns2\""),
            ("ReturnType<typeof NS.g>", "\"ns2\""),
            ("ReturnType<typeof callNbK>", "\"k2\""),
            ("ReturnType<typeof callNbKNum>", "\"k2-num\""),
            ("ReturnType<typeof NB.k>", "\"k2-num\""),
            ("ReturnType<typeof callNtT>", "\"t1\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const GLOBAL_A: &str = "\
interface GS { (): 'ga' }
declare function gsf(x: string): 'gsf-a';
declare namespace GN { function gn(): 'gn-a' }
";
const GLOBAL_B: &str = "\
interface GS { (): 'gb' }
declare const gs: GS;
declare function gsf(x: string): 'gsf-b';
declare namespace GN { function gn(): 'gn-b'; const gc: 'gc' }
";
const GLOBAL_MODULE_A: &str =
    "declare global { interface GM { (): 'a-file' } function gmf(): 'gmf-a'; }\nexport {};\n";
const GLOBAL_MODULE_B: &str =
    "declare global { interface GM { (): 'b-file' } function gmf(): 'gmf-b'; }\nexport {};\n";
const GLOBAL_USE: &str = "\
export function callGs() { return gs(); }
export function callGsf() { return gsf('x'); }
export function callGn() { return GN.gn(); }
export function readGc() { return GN.gc; }
export function callGmf() { return gmf(); }
declare const gm: GM;
export function callGm() { return gm(); }
";

/// Every probe over the four global files, with the checker's print when
/// `script` (`a` or `b`) names the script declared later in program order
/// and `module` the later module.
fn global_rows(script: &str, module: &str) -> Vec<(&'static str, String)> {
    vec![
        ("ReturnType<GS>", format!("\"g{script}\"")),
        ("ReturnType<GM>", format!("\"{module}-file\"")),
        ("ReturnType<typeof gsf>", format!("\"gsf-{script}\"")),
        ("ReturnType<typeof GN.gn>", format!("\"gn-{script}\"")),
        ("typeof GN.gc", "\"gc\"".to_owned()),
        ("ReturnType<typeof gmf>", format!("\"gmf-{module}\"")),
        ("ReturnType<typeof callGs>", format!("\"g{script}\"")),
        ("ReturnType<typeof callGsf>", format!("\"gsf-{script}\"")),
        ("ReturnType<typeof callGn>", format!("\"gn-{script}\"")),
        ("ReturnType<typeof readGc>", "\"gc\"".to_owned()),
        ("ReturnType<typeof callGmf>", format!("\"gmf-{module}\"")),
        ("ReturnType<typeof callGm>", format!("\"{module}-file\"")),
    ]
}

/// A global interface, function or namespace member declared in several
/// SCRIPT files — or in several modules' `declare global` blocks — merges
/// every file's declaration, in the program's declaration order: its
/// signatures list the files in order, and a call tries a later file's
/// first, whichever file the reference is written in.
///
/// Measured on TypeScript 7.0.2 with the files in program order `ga.ts`,
/// `gb.ts`, `gma.ts`, `gmb.ts`: `gs()` and `ReturnType<GS>` are `"gb"`,
/// `gsf('x')` and `ReturnType<typeof gsf>` `"gsf-b"`, `GN.gn()` `"gn-b"`,
/// `GN.gc` `"gc"`, `gmf()` `"gmf-b"`, and `gm()` / `ReturnType<GM>`
/// `"b-file"`. With the two scripts' contents swapped (and the two modules'),
/// every answer names the other file: `"ga"`, `"gsf-a"`, `"gn-a"`,
/// `"gmf-a"`, `"a-file"`.
///
/// Mutation: listing the file a global is read from before the others
/// answers `gs()` with `"ga"` (its declaring script `gb.ts` first); one flat
/// overload list answers `gsf('x')` with `"gsf-a"`; refusing a function
/// declared in several files leaves `gsf`, `GN.gn` and `gmf` unresolved.
#[test]
fn a_global_declared_in_several_files_merges_them() {
    for (files, script, module) in [
        (
            [
                ("ga.ts", GLOBAL_A),
                ("gb.ts", GLOBAL_B),
                ("gma.ts", GLOBAL_MODULE_A),
                ("gmb.ts", GLOBAL_MODULE_B),
            ],
            "b",
            "b",
        ),
        (
            [
                ("ga.ts", GLOBAL_B),
                ("gb.ts", GLOBAL_A),
                ("gma.ts", GLOBAL_MODULE_B),
                ("gmb.ts", GLOBAL_MODULE_A),
            ],
            "a",
            "a",
        ),
    ] {
        let project = ProbeProject {
            files: &files,
            compiler_options: None,
            ambient_lib: None,
        };
        let rows = global_rows(script, module);
        let rows: Vec<(&str, &str)> = rows
            .iter()
            .map(|(probe, print)| (*probe, print.as_str()))
            .collect();
        let failures = mismatches_in(project, GLOBAL_USE, &rows);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

const GLOBAL_BLOCKS: &str = "\
declare global { function dg(): 'first'; }
declare global { function dg(): 'second'; function dg(x: number): 'second-num'; }
export function callDg() { return dg(); }
export function callDgNum() { return dg(1); }
";

/// Two `declare global` blocks of one module are two declarations of the
/// global: a call tries the later block's first.
///
/// Measured on TypeScript 7.0.2: `dg()` is `"second"`, `dg(1)`
/// `"second-num"`, and `ReturnType<typeof dg>` `"second-num"`.
///
/// Mutation: reading a module's global function as one declaration answers
/// `dg()` with `"first"`.
#[test]
fn declare_global_blocks_merge_later_first() {
    let failures = mismatches(
        GLOBAL_BLOCKS,
        &[
            ("ReturnType<typeof callDg>", "\"second\""),
            ("ReturnType<typeof callDgNum>", "\"second-num\""),
            ("ReturnType<typeof dg>", "\"second-num\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const NAMESPACE_FUNCTIONS: &str = "\
import { L } from './lib';
import * as LL from './lib';
export namespace N {
  export function lit() { return 1; }
  export function obj() { return { a: 1, b: \"s\" }; }
  export function decl(): 2 { return 2; }
  export function arr() { return [1, 2]; }
  export function callsLit() { return lit(); }
  export function usesPriv() { return priv(); }
  function priv() { return true; }
  export const k = 3;
  export function readK() { return k; }
  export namespace Inner { export function deep() { return \"d\" as const; } }
  export function cond(b: boolean) { return b ? 1 : \"x\"; }
  export const arrow = () => 5;
}
namespace G { export function g() { return 7n; } export function neg() { return -7n; } }
function t() { return 7n; }
export function callN() { return N.lit(); }
export function callInner() { return N.Inner.deep(); }
export function callG() { return G.g(); }
export function callGNeg() { return G.neg(); }
export function callT() { return t(); }
export function callL() { return L.lit(); }
export function callLL() { return LL.L.str(); }
";

const NAMESPACE_FUNCTIONS_LIB: &str = "\
export namespace L { export function lit() { return 1; } export function str() { return \"s\" as const; } }
";

/// A function declared in a namespace, exported or not, has an inferred
/// return as any function does: its body reads the namespace's own
/// members unqualified (a sibling function, a private one, an exported
/// `const`), and its literal returns widen as a bare literal's do — a
/// `bigint` and a signed literal among them.
///
/// Measured on TypeScript 7.0.2: `N.lit` returns `number`, `N.obj`
/// `{ a: number; b: string; }`, `N.decl` `2`, `N.arr` `number[]`,
/// `N.callsLit` `number`, `N.usesPriv` `boolean`, `N.readK` `number`,
/// `N.Inner.deep` `"d"`, `N.cond` `"x" | 1`, `N.arrow` `number`; `callN`
/// is `number`, `callInner` `"d"`, `callG`, `callGNeg` and `callT`
/// `bigint`, `callL` `number`, `callLL` `"s"`.
///
/// Mutation: indexing no function of an exported namespace leaves every
/// `N.*` row but the annotated `N.decl` a typed miss; reading a namespace
/// body's free name only at file scope leaves `N.readK` a typed miss;
/// keeping a `bigint` literal's type answers `callG` and `callT` `7n`.
#[test]
fn a_namespace_function_returns_its_inferred_type() {
    let files = [("lib.ts", NAMESPACE_FUNCTIONS_LIB)];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            compiler_options: None,
            ambient_lib: None,
        },
        NAMESPACE_FUNCTIONS,
        &[
            ("ReturnType<typeof N.lit>", "number"),
            ("ReturnType<typeof N.obj>", "{ a: number; b: string; }"),
            ("ReturnType<typeof N.decl>", "2"),
            ("ReturnType<typeof N.arr>", "number[]"),
            ("ReturnType<typeof N.callsLit>", "number"),
            ("ReturnType<typeof N.usesPriv>", "boolean"),
            ("ReturnType<typeof N.readK>", "number"),
            ("ReturnType<typeof N.Inner.deep>", "\"d\""),
            ("ReturnType<typeof N.cond>", "\"x\" | 1"),
            ("ReturnType<typeof N.arrow>", "number"),
            ("ReturnType<typeof callN>", "number"),
            ("ReturnType<typeof callInner>", "\"d\""),
            ("ReturnType<typeof callG>", "bigint"),
            ("ReturnType<typeof callGNeg>", "bigint"),
            ("ReturnType<typeof callT>", "bigint"),
            ("ReturnType<typeof callL>", "number"),
            ("ReturnType<typeof callLL>", "\"s\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const AUGMENTED_FUNCTION_MODULE: &str =
    "export function f(x: number): \"m-num\";\nexport function f(x: any): any { return x; }\n";
const AUGMENTING_FUNCTION_MODULE: &str = "\
import './m';
declare module './m' {
  function f(x: string): \"aug-str\";
  function f(x: boolean): \"aug-bool\";
}
export {};
";
const AUGMENTED_FUNCTION_USE: &str = "\
import { f } from './m';
import * as M from './m';
export function fs() { return f(\"a\"); }
export function fb() { return f(true); }
export function fn() { return f(1); }
export function mfs() { return M.f(\"a\"); }
";

/// A module's function merges the declarations a `declare module` block
/// augmenting that module adds, after its own: `typeof f` lists every
/// declaration's signatures in that order, so `ReturnType` and
/// `Parameters` read the augmentation's last one, and a call resolves
/// against all of them.
///
/// Measured on TypeScript 7.0.2 (`--module commonjs`, alike on the four
/// `strictNullChecks` × `noImplicitAny` settings): `ReturnType<typeof f>`
/// is `"aug-bool"`, `Parameters<typeof f>[0]` `boolean`, `fs` and `mfs`
/// `"aug-str"`, `fb` `"aug-bool"`, `fn` `"m-num"`.
///
/// Mutation: merging no augmenting declaration answers
/// `ReturnType<typeof f>`, `fs`, `fb` and `mfs` `"m-num"` and
/// `Parameters<typeof f>[0]` `number`.
#[test]
fn a_module_function_merges_its_augmentations() {
    let files = [
        ("m.ts", AUGMENTED_FUNCTION_MODULE),
        ("aug.ts", AUGMENTING_FUNCTION_MODULE),
    ];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            compiler_options: None,
            ambient_lib: None,
        },
        AUGMENTED_FUNCTION_USE,
        &[
            ("ReturnType<typeof f>", "\"aug-bool\""),
            ("Parameters<typeof f>[0]", "boolean"),
            ("ReturnType<typeof fs>", "\"aug-str\""),
            ("ReturnType<typeof fb>", "\"aug-bool\""),
            ("ReturnType<typeof fn>", "\"m-num\""),
            ("ReturnType<typeof mfs>", "\"aug-str\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const SCRIPT_OVERLOAD_ONE: &str = "declare function gf(x: number): \"s1-num\";\n";
const SCRIPT_OVERLOAD_TWO: &str =
    "declare function gf(x: string): \"s2-str\";\ndeclare function gf(x: boolean): \"s2-bool\";\n";
const SCRIPT_OVERLOAD_USE: &str = "\
export function gs() { return gf(\"a\"); }
export function gb() { return gf(true); }
export function gn() { return gf(1); }
";

/// A global function declared in several scripts is one function whose
/// signatures are every script's, in program order.
///
/// Measured on TypeScript 7.0.2 (alike on the four settings), the scripts
/// in the order listed: `ReturnType<typeof gf>` is `"s2-bool"`,
/// `Parameters<typeof gf>[0]` `boolean`, `gs` `"s2-str"`, `gb`
/// `"s2-bool"`, `gn` `"s1-num"`.
///
/// Mutation: reading the first script's declaration alone answers
/// `ReturnType<typeof gf>` `"s1-num"`.
#[test]
fn a_global_function_merges_every_scripts_overloads() {
    let files = [
        ("s1.ts", SCRIPT_OVERLOAD_ONE),
        ("s2.ts", SCRIPT_OVERLOAD_TWO),
    ];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            compiler_options: None,
            ambient_lib: None,
        },
        SCRIPT_OVERLOAD_USE,
        &[
            ("ReturnType<typeof gf>", "\"s2-bool\""),
            ("Parameters<typeof gf>[0]", "boolean"),
            ("ReturnType<typeof gs>", "\"s2-str\""),
            ("ReturnType<typeof gb>", "\"s2-bool\""),
            ("ReturnType<typeof gn>", "\"s1-num\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
