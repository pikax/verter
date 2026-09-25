//! A value imported from a module that only `declare module "…"` blocks
//! declare resolves to the block's declaration, as its types do: a named,
//! default or namespace import, an import assignment of a block assigning
//! `export =`, and the members a block's namespace, class or merged
//! function declares.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --declaration
//! --emitDeclarationOnly --strict --module commonjs`, each row asserted as
//! an exact type equality beside it; the four `strictNullChecks` ×
//! `noImplicitAny` settings answer alike.

use super::checker_probe_lane_tests::{mismatches_in, ProbeProject};

const AMBIENT: &str = "\
declare module \"amb\" {
  export function x(): \"x\";
  export function x(n: number): \"xn\";
  export interface T { t: \"t\" }
  export const k: \"k\";
  export let l: number;
  export class Cls { m(): \"cm\"; static s: \"cs\"; }
  export namespace AN { function f(): \"an-f\"; interface U { u: \"u\" } const c: \"an-c\"; }
  const hidden: \"h\";
  export default function dflt(): \"d\";
}
declare module \"amb2\" {
  function y(): \"y\";
  const z: \"z\";
}
declare module \"eqmod\" {
  function Q(): \"q\";
  namespace Q { const w: \"w\"; }
  export = Q;
}
";

const USE: &str = "\
import { x, k, l, Cls, AN, hidden } from 'amb';
import dflt from 'amb';
import type { T } from 'amb';
import * as A from 'amb';
import { y, z } from 'amb2';
import Q = require('eqmod');
import QD from 'eqmod';
export function cx() { return x(); }
export function cxn() { return x(1); }
export function rk() { return k; }
export function rl() { return l; }
export function cm() { const c = new Cls(); return c.m(); }
export function cs() { return Cls.s; }
export function rt(t: T) { return t.t; }
export function anf() { return AN.f(); }
export function anc() { return AN.c; }
export function df() { return dflt(); }
export function sx() { return A.x(); }
export function sk() { return A.k; }
export function sanf() { return A.AN.f(); }
export function cy() { return y(); }
export function rz() { return z; }
export function q() { return Q(); }
export function qw() { return Q.w; }
export function qd() { return QD(); }
export function h() { return hidden; }
";

/// Every import form of an ambient module's value reads the block's
/// declaration: overloads in order, a `let`'s declared type, a class's
/// instance and static members, a namespace's members, `default` the
/// block's `export default` declaration, and an import assignment (or a
/// default import) the value a block assigns with `export =`, whose merged
/// namespace declares its properties. A block without an export
/// declaration exports every declaration.
///
/// Measured on TypeScript 7.0.2: `cx` is `"x"`, `cxn` `"xn"`, `rk` `"k"`,
/// `rl` `number`, `cm` `"cm"`, `cs` `"cs"`, `rt` `"t"`, `anf` `"an-f"`,
/// `anc` `"an-c"`, `df` `"d"`, `sx` `"x"`, `sk` `"k"`, `sanf` `"an-f"`,
/// `cy` `"y"`, `rz` `"z"`, `q` `"q"`, `qw` `"w"`, `qd` `"q"`, `h` `"h"`;
/// `typeof k` is `"k"` and `ReturnType<typeof x>` (the last overload)
/// `"xn"`.
///
/// Mutation: resolving no import of an ambient module to the block's member
/// leaves every named, default and import-assignment row a typed miss while
/// `rt` (a type) still answers; addressing only a block's `declare global`
/// values by the declaring file's identity leaves every value row a typed
/// miss.
#[test]
fn an_ambient_module_value_resolves_to_its_declaration() {
    let files = [("amb.d.ts", AMBIENT)];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            compiler_options: None,
            ambient_lib: None,
        },
        USE,
        &[
            ("ReturnType<typeof cx>", "\"x\""),
            ("ReturnType<typeof cxn>", "\"xn\""),
            ("ReturnType<typeof rk>", "\"k\""),
            ("ReturnType<typeof rl>", "number"),
            ("ReturnType<typeof cm>", "\"cm\""),
            ("ReturnType<typeof cs>", "\"cs\""),
            ("ReturnType<typeof rt>", "\"t\""),
            ("ReturnType<typeof anf>", "\"an-f\""),
            ("ReturnType<typeof anc>", "\"an-c\""),
            ("ReturnType<typeof df>", "\"d\""),
            ("ReturnType<typeof sx>", "\"x\""),
            ("ReturnType<typeof sk>", "\"k\""),
            ("ReturnType<typeof sanf>", "\"an-f\""),
            ("ReturnType<typeof cy>", "\"y\""),
            ("ReturnType<typeof rz>", "\"z\""),
            ("ReturnType<typeof q>", "\"q\""),
            ("ReturnType<typeof qw>", "\"w\""),
            ("ReturnType<typeof qd>", "\"q\""),
            ("ReturnType<typeof h>", "\"h\""),
            ("typeof k", "\"k\""),
            ("ReturnType<typeof x>", "\"xn\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
